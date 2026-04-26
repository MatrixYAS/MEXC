// backend/src/main.rs
// Production version: Secure config from environment, error handling

use anyhow::Result;
use axum::{
    extract::State,
    routing::{get, post},
    Json, Router,
    response::sse::{Event, Sse},
    http::StatusCode,
};
use futures::StreamExt;
use std::convert::Infallible;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock, Mutex};
use tower_http::{cors::CorsLayer, services::ServeDir};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod data;
mod engine;
mod network;
mod persistence;
mod cron;
mod telemetry;

use crate::data::{Database, Opportunity, ApiKeys, ApiKeyRequest};
use crate::engine::MathEngine;
use crate::network::{NetworkManager, RestClient};
use crate::persistence::{SqlitePersistence, TradeLogger};
use crate::cron::{MaintenanceTask, CleanerTask};
use crate::telemetry::TelemetryCollector;
use crate::data::models::{HealthResponse, Telemetry};

// Updated AppState
#[derive(Clone)]
struct AppState {
    math_engine: Arc<MathEngine>,
    network_manager: Arc<NetworkManager>,
    trade_logger: Arc<TradeLogger>,
    telemetry_collector: Arc<TelemetryCollector>,
    ws_pool: Arc<Mutex<network::WssPool>>,
    opportunity_sender: broadcast::Sender<Opportunity>,
    api_keys: Arc<RwLock<Option<ApiKeys>>>,
    db: Arc<Database>,
    admin_password: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing from RUST_LOG environment variable
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!("🚀 Starting MEXC Ghost Hunter...");

    // Load database with data directory from environment
    let data_dir = std::env::var("DATA_DIR").unwrap_or_else(|_| "/data".to_string());
    tracing::info!("📁 Using data directory: {}", data_dir);

    let db = Arc::new(Database::new().await?);
    let math_engine = Arc::new(MathEngine::new());
    let network_manager = Arc::new(NetworkManager::new(Arc::clone(&math_engine)));
    let ws_pool = Arc::new(Mutex::new(network_manager.ws_pool.clone()));

    let sqlite_persistence = Arc::new(SqlitePersistence::new(Arc::clone(&db)));
    let trade_logger = Arc::new(TradeLogger::new(Arc::clone(&sqlite_persistence)));
    let telemetry_collector = Arc::new(TelemetryCollector::new(Arc::clone(&math_engine)));

    let (opportunity_sender, _) = broadcast::channel::<Opportunity>(100);
    let api_keys = Arc::new(RwLock::new(db.get_api_keys().await.ok().flatten()));

    // CRITICAL: Load admin password from environment, NEVER hardcode
    let admin_password = std::env::var("ADMIN_PASSWORD")
        .or_else(|_| std::env::var("GHOST_HUNTER_ADMIN_PASSWORD"))
        .expect("❌ ADMIN_PASSWORD environment variable not set. Set it before running: export ADMIN_PASSWORD='your_secure_password'");

    if admin_password.len() < 8 {
        tracing::warn!("⚠️ WARNING: Admin password is shorter than 8 characters. Consider using a stronger password.");
    }

    let state = AppState {
        math_engine: Arc::clone(&math_engine),
        network_manager: Arc::clone(&network_manager),
        trade_logger: Arc::clone(&trade_logger),
        telemetry_collector: Arc::clone(&telemetry_collector),
        ws_pool: Arc::clone(&ws_pool),
        opportunity_sender: opportunity_sender.clone(),
        api_keys,
        db: Arc::clone(&db),
        admin_password,
    };

    start_background_tasks(&state, opportunity_sender).await;

    let app = Router::new()
        .route("/api/health", get(health_handler))
        .route("/api/telemetry", get(telemetry_handler))
        .route("/api/opportunities", get(recent_opportunities_handler))
        .route("/api/whitelist", get(whitelist_handler))
        .route("/api/live-pulse", get(live_pulse_sse_handler))
        .route("/api/keys", post(save_api_keys_handler))
        .route("/api/keys", get(get_api_keys_handler))
        .route("/api/test-mexc-connection", post(test_mexc_connection_handler))
        .route("/api/login", post(login_handler))
        .route("/api/today-stats", get(today_stats_handler))
        .fallback_service(ServeDir::new("frontend/dist"))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let port = std::env::var("PORT").unwrap_or_else(|_| "8080".to_string());
    let addr = format!("0.0.0.0:{}", port);
    tracing::info!("✅ Server listening on http://{}", addr);
    tracing::info!("📊 Telemetry available at http://{}:PORT/api/health", "localhost");

    axum::Server::bind(&addr.parse()?)
        .serve(app.into_make_service())
        .await?;

    Ok(())
}

async fn start_background_tasks(state: &AppState, _sender: broadcast::Sender<Opportunity>) {
    let logger_clone = Arc::clone(&state.trade_logger);
    tokio::spawn(async move { logger_clone.start_batch_flusher().await; });

    let maintenance = Arc::new(MaintenanceTask::new(
        Arc::new(RestClient::new()),
        Arc::clone(&state.math_engine),
        Arc::clone(&state.ws_pool),
    ));
    maintenance.start_scheduler().await;

    let cleaner = Arc::new(CleanerTask::new(Arc::clone(&state.trade_logger)));
    cleaner.start_scheduler().await;

    let telemetry_clone = Arc::clone(&state.telemetry_collector);
    tokio::spawn(async move { telemetry_clone.start_collector().await; });
}

// SSE Handler
async fn live_pulse_sse_handler(
    State(state): State<AppState>,
) -> Sse<impl futures::Stream<Item = Result<Event, Infallible>>> {
    let rx = state.opportunity_sender.subscribe();
    let stream = tokio_stream::wrappers::BroadcastStream::new(rx)
        .filter_map(|msg| async move { msg.ok() })
        .map(|opportunity| Ok(Event::default().json_data(opportunity).unwrap()));

    Sse::new(stream)
}

// API Keys Handlers
async fn save_api_keys_handler(
    State(state): State<AppState>,
    Json(payload): Json<ApiKeyRequest>,
) -> Result<Json<&'static str>, (StatusCode, String)> {
    if let Err(e) = state.db.save_api_keys(payload).await {
        tracing::error!("Failed to save API keys: {}", e);
        return Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()));
    }
    tracing::info!("✅ API keys updated successfully");
    Ok(Json("API keys saved successfully"))
}

async fn get_api_keys_handler(State(state): State<AppState>) -> Json<bool> {
    let keys = state.api_keys.read().await;
    Json(keys.is_some())
}

async fn test_mexc_connection_handler(
    State(_state): State<AppState>,
    Json(_payload): Json<ApiKeyRequest>,
) -> Json<&'static str> {
    Json("Connection test passed (placeholder)")
}

async fn login_handler(
    State(state): State<AppState>,
    Json(payload): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let provided_password = payload["password"].as_str().unwrap_or("");
    
    if provided_password == state.admin_password {
        tracing::info!("✅ Login successful");
        Ok(Json(serde_json::json!({ "token": "authenticated" })))
    } else {
        tracing::warn!("❌ Failed login attempt");
        Err((StatusCode::UNAUTHORIZED, "Invalid password".to_string()))
    }
}

async fn today_stats_handler(State(state): State<AppState>) -> Json<serde_json::Value> {
    match state.trade_logger.get_today_analytics().await {
        Ok((gaps, avg, total)) => Json(serde_json::json!({
            "gaps_found": gaps,
            "avg_yield": avg,
            "total_potential": total
        })),
        Err(e) => {
            tracing::warn!("Failed to get today's analytics: {}", e);
            Json(serde_json::json!({
                "gaps_found": 0,
                "avg_yield": 0.0,
                "total_potential": 0.0
            }))
        }
    }
}

async fn health_handler(State(state): State<AppState>) -> Json<HealthResponse> {
    let telemetry = state.telemetry_collector.collect().await;
    Json(HealthResponse {
        status: "healthy".to_string(),
        uptime_ms: state.telemetry_collector.uptime_ms(),
        telemetry,
    })
}

async fn telemetry_handler(State(state): State<AppState>) -> Json<Telemetry> {
    Json(state.telemetry_collector.collect().await)
}

async fn recent_opportunities_handler(State(state): State<AppState>) -> Json<Vec<Opportunity>> {
    match state.trade_logger.get_recent(50).await {
        Ok(ops) => Json(ops),
        Err(e) => {
            tracing::warn!("Failed to fetch recent opportunities: {}", e);
            Json(vec![])
        }
    }
}

async fn whitelist_handler(State(state): State<AppState>) -> Json<Vec<String>> {
    let mut symbols: Vec<String> = state.math_engine.order_books
        .iter()
        .map(|entry| entry.key().clone())
        .collect();
    symbols.sort();
    Json(symbols)
}
