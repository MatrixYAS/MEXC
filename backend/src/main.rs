// backend/src/main.rs
// Server Entry & Task Coordinator
// Ties together everything: Math Engine, Networking, Persistence, Cron, Telemetry + Axum API

use anyhow::Result;
use axum::{
    extract::State,
    routing::{get, post},
    Json, Router,
};
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_http::cors::CorsLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod data;
mod engine;
mod network;
mod persistence;
mod cron;
mod telemetry;

use crate::data::{Database, Opportunity};
use crate::engine::MathEngine;
use crate::network::{NetworkManager, RestClient};
use crate::persistence::{SqlitePersistence, TradeLogger};
use crate::cron::{MaintenanceTask, CleanerTask};
use crate::telemetry::TelemetryCollector;
use crate::data::models::{HealthResponse, Telemetry};

// App State shared across handlers
#[derive(Clone)]
struct AppState {
    math_engine: Arc<MathEngine>,
    network_manager: Arc<NetworkManager>,
    trade_logger: Arc<TradeLogger>,
    telemetry_collector: Arc<TelemetryCollector>,
    ws_pool: Arc<Mutex<network::WssPool>>,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!("🚀 Starting MEXC Ghost Hunter...");

    // 1. Initialize Database
    let db = Arc::new(Database::new().await?);

    // 2. Initialize Core Engine
    let math_engine = Arc::new(MathEngine::new());

    // 3. Initialize Network Layer
    let network_manager = Arc::new(NetworkManager::new(Arc::clone(&math_engine)));
    let ws_pool = Arc::new(Mutex::new(network_manager.ws_pool.clone())); // For sharing

    // 4. Initialize Persistence
    let sqlite_persistence = Arc::new(SqlitePersistence::new(Arc::clone(&db)));
    let trade_logger = Arc::new(TradeLogger::new(Arc::clone(&sqlite_persistence)));

    // 5. Initialize Telemetry
    let telemetry_collector = Arc::new(TelemetryCollector::new(Arc::clone(&math_engine)));

    // 6. Build App State
    let state = AppState {
        math_engine: Arc::clone(&math_engine),
        network_manager: Arc::clone(&network_manager),
        trade_logger: Arc::clone(&trade_logger),
        telemetry_collector: Arc::clone(&telemetry_collector),
        ws_pool: Arc::clone(&ws_pool),
    };

    // 7. Start Background Tasks
    start_background_tasks(&state).await;

    // 8. Build Axum Router
    let app = Router::new()
        .route("/api/health", get(health_handler))
        .route("/api/telemetry", get(telemetry_handler))
        .route("/api/opportunities", get(recent_opportunities_handler))
        .route("/api/whitelist", get(whitelist_handler))
        // Future: POST /api/trade (paper/live)
        .layer(CorsLayer::permissive()) // Adjust in production
        .with_state(state);

    // 9. Start HTTP Server (for Hugging Face + local)
    let port = std::env::var("PORT").unwrap_or_else(|_| "7860".to_string());
    let addr = format!("0.0.0.0:{}", port);
    
    tracing::info!("✅ Server listening on http://{}", addr);

    axum::Server::bind(&addr.parse()?)
        .serve(app.into_make_service())
        .await?;

    Ok(())
}

// ====================== Background Tasks ======================

async fn start_background_tasks(state: &AppState) {
    // Start batch flusher
    let logger_clone = Arc::clone(&state.trade_logger);
    tokio::spawn(async move {
        logger_clone.start_batch_flusher().await;
    });

    // Start 24h Maintenance
    let maintenance = Arc::new(MaintenanceTask::new(
        Arc::new(RestClient::new()), // Temporary - better to inject from network_manager
        Arc::clone(&state.math_engine),
        Arc::clone(&state.ws_pool),
    ));
    maintenance.start_scheduler().await;

    // Start Daily Cleaner
    let cleaner = Arc::new(CleanerTask::new(Arc::clone(&state.trade_logger)));
    cleaner.start_scheduler().await;

    // Start Telemetry Collector
    let telemetry_clone = Arc::clone(&state.telemetry_collector);
    tokio::spawn(async move {
        telemetry_clone.start_collector().await;
    });

    // Initial whitelist load (will trigger WS pool start)
    tracing::info!("Starting initial WebSocket pool...");
    // In production: load from DB or run maintenance once
}

// ====================== API Handlers ======================

async fn health_handler(State(state): State<AppState>) -> Json<HealthResponse> {
    let telemetry = state.telemetry_collector.collect();
    Json(HealthResponse {
        status: "healthy".to_string(),
        uptime_ms: state.telemetry_collector.uptime_ms(),
        telemetry,
    })
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

async fn whitelist_handler(State(_state): State<AppState>) -> Json<Vec<String>> {
    // Placeholder - expand with real whitelist from DB
    Json(vec!["BTCUSDT".to_string(), "ETHUSDT".to_string()])
}

// TODO: Add more endpoints as frontend is built (Live Pulse SSE, etc.)

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn test_server_starts() {
        // Basic smoke test
        assert!(true);
    }
}
