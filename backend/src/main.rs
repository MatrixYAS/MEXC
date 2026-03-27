// backend/src/main.rs
// Updated with SSE, static serving, API keys, auth, and all changes from the guide

use anyhow::Result;
use axum::{
    extract::State,
    routing::{get, post},
    Json, Router,
    response::sse::{Event, Sse},
};
use axum::http::StatusCode;
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

// Updated AppState with all new requirements from the guide
#[derive(Clone)]
struct AppState {
    math_engine: Arc<MathEngine>,
    network_manager: Arc<NetworkManager>,
    trade_logger: Arc<TradeLogger>,
    telemetry_collector: Arc<TelemetryCollector>,
    ws_pool: Arc<Mutex<network::WssPool>>,
    opportunity_sender: broadcast::Sender<Opportunity>,   // For SSE Live Pulse
    api_keys: Arc<RwLock<Option<ApiKeys>>>,               // Secure API keys
    admin_password: String,                               // Simple auth
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!("🚀 Starting MEXC Ghost Hunter (with all guide fixes)...");

    let db = Arc::new(Database::new().await?);

    let math_engine = Arc::new(MathEngine::new());
    let network_manager = Arc::new(NetworkManager::new(Arc::clone(&math_engine)));
    let ws_pool = Arc::new(Mutex::new(network_manager.ws_pool.clone()));

    let sqlite_persistence = Arc::new(SqlitePersistence::new(Arc::clone(&db)));
    let trade_logger = Arc::new(TradeLogger::new(Arc::clone(&sqlite_persistence)));

    let telemetry_collector = Arc::new(TelemetryCollector::new(Arc::clone(&math_engine)));

    // Broadcast channel for real-time SSE
    let (opportunity_sender, _) = broadcast::channel::<Opportunity>(100);

    // Load existing API keys from DB
    let api_keys = Arc::new(RwLock::new(db.get_api_keys().await.ok()));

    let admin_password = std::env::var("ADMIN_PASSWORD")
        .unwrap_or_else(|_| "ghosthunter123".to_string());

    let state = AppState {
        math_engine: Arc::clone(&math_engine),
        network_manager: Arc::clone(&network_manager),
        trade_logger: Arc::clone(&trade_logger),
        telemetry_collector: Arc::clone(&telemetry_collector),
        ws_pool: Arc::clone(&ws_pool),
        opportunity_sender: opportunity_sender.clone(),
        api_keys,
        admin_password,
    };

    // Start background tasks
    start_background_tasks(&state, opportunity_sender).await;

    // Build router with ALL new routes from the guide
    let app = Router::new()
        .route("/api/health", get(health_handler))
        .route("/api/telemetry", get(telemetry_handler))
        .route("/api/opportunities", get(recent_opportunities_handler))
        .route("/api/whitelist", get(whitelist_handler))
        .route("/api/live-pulse", get(live_pulse_sse_handler))           // NEW SSE
        .route("/api/keys", post(save_api_keys_handler))                 // NEW
        .route("/api/keys", get(get_api_keys_handler))                   // NEW
        .route("/api/login", post(login_handler))                        // NEW
        .route("/api/today-stats", get(today_stats_handler))             // NEW
        .fallback_service(ServeDir::new("frontend/dist"))                // Static React files
        .layer(CorsLayer::permissive())
        .with_state(state);

    let port = std::env::var("PORT").unwrap_or_else(|_| "7860".to_string());
    let addr = format!("0.0.0.0:{}", port);
    tracing::info!("✅ Server listening on http://{}", addr);

    axum::Server::bind(&addr.parse()?)
        .serve(app.into_make_service())
        .await?;

    Ok(())
}

// ====================== BACKGROUND TASKS ======================
async fn start_background_tasks(state: &AppState, sender: broadcast::Sender<Opportunity>) {
    // Batch flusher
    let logger_clone = Arc::clone(&state.trade_logger);
    tokio::spawn(async move { logger_clone.start_batch_flusher().await; });

    // Maintenance + Cleaner + Telemetry (same as before)
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

    // Link TradeLogger to broadcast sender so verified opportunities are sent to SSE
    // (We will update TradeLogger in the next file)
    tracing::info!("SSE broadcast channel ready for Live Pulse");
}

// ====================== NEW HANDLERS FROM GUIDE ======================

// SSE for Live Pulse
async fn live_pulse_sse_handler(
    State(state): State<AppState>,
) -> Sse<impl futures::Stream<Item = Result<Event, Infallible>>> {
    let rx = state.opportunity_sender.subscribe();
    let stream = tokio_stream::wrappers::BroadcastStream::new(rx)
        .filter_map(|msg| async move { msg.ok() })
        .map(|opportunity| {
            Ok(Event::default().json_data(opportunity).unwrap())
        });

    Sse::new(stream)
}

// API Keys handlers (stub - full implementation in next steps)
async fn save_api_keys_handler(
    State(state): State<AppState>,
    Json(payload): Json<ApiKeyRequest>,
) -> Json<&'static str> {
    // TODO: encrypt + save (we'll add encryption in db.rs next)
    let mut keys = state.api_keys.write().await;
    *keys = Some(ApiKeys::new(payload.api_key, payload.secret_key));
    Json("API keys saved")
}

async fn get_api_keys_handler(State(state): State<AppState>) -> Json<bool> {
    let keys = state.api_keys.read().await;
    Json(keys.is_some())
}

// Simple login
async fn login_handler(
    State(state): State<AppState>,
    Json(payload): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    if payload["password"].as_str() == Some(&state.admin_password) {
        Ok(Json(serde_json::json!({ "token": "dummy_token" })))
    } else {
        Err((StatusCode::UNAUTHORIZED, "Invalid password".to_string()))
    }
}

// Today stats (stub - will be expanded when we update trade_logger)
async fn today_stats_handler(State(state): State<AppState>) -> Json<serde_json::Value> {
    // Placeholder for now
    Json(serde_json::json!({
        "gaps_found": 42,
        "avg_yield": 0.32,
        "total_potential": 13.7
    }))
}

// Keep existing handlers (health, telemetry, etc.)
async fn health_handler(State(state): State<AppState>) -> Json<HealthResponse> {
    let telemetry = state.telemetry_collector.collect();
    Json(HealthResponse { status: "healthy".to_string(), uptime_ms: state.telemetry_collector.uptime_ms(), telemetry })
}

async fn telemetry_handler(State(state): State<AppState>) -> Json<Telemetry> {
    Json(state.telemetry_collector.collect())
}

async fn recent_opportunities_handler(State(state): State<AppState>) -> Json<Vec<Opportunity>> {
    match state.trade_logger.get_recent(50).await {
        Ok(ops) => Json(ops),
        Err(_) => Json(vec![]),
    }
}

async fn whitelist_handler(_state: State<AppState>) -> Json<Vec<String>> {
    Json(vec!["BTCUSDT".to_string(), "ETHUSDT".to_string()])
}
