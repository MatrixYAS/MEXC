// backend/src/main.rs
// MEXC Ghost Hunter v2 — full rewrite per master plan.
//
// Architecture:
// - WS workers (10 shards x 30 symbols) stream 100ms depth into a lock-free DashMap.
// - A single scan task (Mutex<Scanner>) walks the cached USDT topology every tick,
//   recomputing value from live books (nothing ever trades on stale numbers).
// - Verified opportunities flow through a broadcast channel: SSE to UI + SQLite.
// - Maintenance (hourly) rebuilds the volume-ranked whitelist and swaps WS subs.
// - Settings are fully configurable in-app; credentials are verified before save.

use anyhow::Result;
use axum::{
    extract::State,
    routing::{get, post},
    Json, Router,
    response::sse::{Event, Sse},
    http::{StatusCode, HeaderMap},
};
use chrono::Utc;
use futures::StreamExt;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, Mutex, RwLock};
use tower_http::{cors::CorsLayer, services::ServeDir};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod cron;
mod data;
mod engine;
mod network;
mod persistence;
mod telemetry;

use crate::data::models::*;
use crate::data::Database;
use crate::engine::scanner::Scanner;
use crate::network::{RestClient, WssPool};
use crate::persistence::{SqlitePersistence, TradeLogger};
use crate::cron::{CleanerTask, MaintenanceTask};
use crate::telemetry::TelemetryCollector;

// JWT claims
#[derive(Debug, Serialize, Deserialize, Clone)]
struct Claims {
    sub: String,
    exp: usize,
}

#[derive(Clone)]
struct AppState {
    scanner: Arc<Mutex<Scanner>>,
    trade_logger: Arc<TradeLogger>,
    telemetry_collector: Arc<TelemetryCollector>,
    ws_pool: Arc<Mutex<WssPool>>,
    db: Arc<Database>,
    rest_client: Arc<RestClient>,
    opportunity_sender: broadcast::Sender<Opportunity>,
    admin_password: String,
    jwt_secret: String,
    settings: Arc<RwLock<SettingsSnapshot>>,
    math_engine: Arc<engine::MathEngine>,
}

fn check_auth(state: &AppState, headers: &HeaderMap) -> Result<(), (StatusCode, String)> {
    let auth = headers
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let token = auth.strip_prefix("Bearer ").unwrap_or("");
    let Ok(token_data) = decode::<Claims>(
        token,
        &DecodingKey::from_secret(state.jwt_secret.as_bytes()),
        &Validation::default(),
    ) else {
        return Err((StatusCode::UNAUTHORIZED, "Invalid or missing token".into()));
    };
    if token_data.claims.sub != "admin" {
        return Err((StatusCode::FORBIDDEN, "Not admin".into()));
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!("🚀 Starting MEXC Ghost Hunter v2...");

        let data_dir = std::env::var("DATA_DIR").unwrap_or_else(|_| "/data".to_string());
    tracing::info!("📁 Data directory: {}", data_dir);
    // Ensure the Database layer uses the configured data dir
    let _ = std::env::set_var("DATA_DIR", &data_dir);
    let db = Arc::new(Database::new().await?);

    // ---- Load (and hot-apply) persisted settings ----
    let settings = db.get_settings().await?;
    let shared_settings: Arc<RwLock<SettingsSnapshot>> = Arc::new(RwLock::new(settings.clone()));
    let envs = shared_settings.read().await.snapshot_to_envs();
    for (key, value) in envs {
        let _ = std::env::set_var(key, &value);
    }
    tracing::info!(
        "⚙️ Settings loaded: threshold={}% tick={}ms volume=${:.0} whitelist={} ret={}d paused={}",
        settings.min_profit_threshold * 100.0,
        settings.tick_interval_ms,
        settings.target_volume_usd,
        settings.max_whitelist,
        settings.retention_days,
        settings.scan_paused
    );

    let admin_password = std::env::var("ADMIN_PASSWORD")
        .or_else(|_| std::env::var("GHOST_HUNTER_ADMIN_PASSWORD"))
        .unwrap_or_else(|_e| {
            let auto = format!("ghosthunter-{}", uuid::Uuid::new_v4().to_string().split('-').next().unwrap());
            tracing::warn!("⚠️ No ADMIN_PASSWORD set — auto-generated one; set the secret for stability: {}", auto);
            auto
        });
    if admin_password.len() < 8 {
        tracing::warn!("⚠️ Admin password < 8 chars — consider a stronger password.");
    }
    let jwt_secret = std::env::var("JWT_SECRET")
        .unwrap_or_else(|_| format!("{}-jwt-secret", admin_password));

    let math_engine = Arc::new(engine::MathEngine::new());
    // Shared REST client lives on the WS pool so workers, reseeds, and API
    // handlers all draw from ONE global rate bucket (MEXC IP limit safety).
    let shared_rest = Arc::new(network::RestClient::new());
    let mut pool_inst = network::WssPool::new_with_rest(Arc::clone(&math_engine), Arc::clone(&shared_rest));
    let ws_pool: Arc<Mutex<network::WssPool>> = Arc::new(Mutex::new(pool_inst));

    // Seed the pool with an initial USDT-major set; maintenance replaces it within
    // the first hour (or immediately on the first run if we call it eagerly).
    let initial_symbols: Vec<String> = vec![
        "BTCUSDT", "ETHUSDT", "SOLUSDT", "DOGEUSDT", "XRPUSDT", "ADAUSDT",
        "PEPEUSDT", "SHIBUSDT", "AVAXUSDT", "TONUSDT", "TRXUSDT", "BNBUSDT",
        "SUIUSDT", "NEARUSDT", "APTUSDT", "OPUSDT", "ARBUSDT", "WIFUSDT",
    ]
    .into_iter()
    .map(String::from)
    .collect();

    let scanner = Arc::new(Mutex::new({
        let mut s = Scanner::new(Arc::clone(&math_engine));
        s.paused = shared_settings.read().await.scan_paused;
        s.settings = Some(Arc::clone(&shared_settings));
        s.validator.settings = Some(Arc::clone(&shared_settings));
        s
    }));

    // Calculator/validator read the same settings live (no restart needed).
    engine::calculator::attach_live_settings(Arc::clone(&shared_settings));

    let sqlite = Arc::new(SqlitePersistence::new(Arc::clone(&db)));
    let trade_logger = Arc::new(TradeLogger::new(sqlite));
    let telemetry_collector = Arc::new(TelemetryCollector::new(Arc::clone(&math_engine), Arc::clone(&scanner)));
    let rest_client = Arc::clone(&shared_rest);

    let (opportunity_sender, mut opportunity_receiver) = broadcast::channel::<Opportunity>(512);

    // ---- Background tasks ----
    // 1. Batch flusher for SQLite
    let logger_clone = Arc::clone(&trade_logger);
    tokio::spawn(async move { logger_clone.start_batch_flusher().await; });

    // 2. Persistence task: receive verified opportunities -> SQLite
    let logger_clone2 = Arc::clone(&trade_logger);
    tokio::spawn(async move {
        loop {
            match opportunity_receiver.recv().await {
                Ok(opp) => {
                    logger_clone2.log_verified_gap(opp).await;
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!("SSE receiver lagged, dropped {} opportunities from buffer (DB unaffected)", n);
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    // 3. WebSocket pool
    {
        let mut pool = ws_pool.lock().await;
        pool.set_symbols(initial_symbols.clone());
        pool.start().await;
    }

    // 4. Eager first maintenance: full whitelist + topology + scanner wiring
    //    (done after WS start so seamless_update works cleanly)
    let maintenance = Arc::new(MaintenanceTask::new(Arc::clone(&rest_client), Arc::clone(&db)));
    let reseed_task = Arc::new(network::ReseedTask::new(Arc::clone(&math_engine), Arc::clone(&shared_rest)));
    {
        let reseed = Arc::clone(&reseed_task);
        match maintenance.run(Arc::clone(&ws_pool), Arc::clone(&scanner), reseed.clone()).await {
            Ok(()) => tracing::info!("✅ Initial whitelist maintenance completed"),
            Err(e) => tracing::warn!("⚠️ Initial maintenance failed (will retry hourly): {}", e),
        }
    }
    tokio::spawn({
        let m = Arc::clone(&maintenance);
        let w = Arc::clone(&ws_pool);
        let s = Arc::clone(&scanner);
        let reseed = Arc::clone(&reseed_task);
        async move { m.start_scheduler(w, s, reseed).await }
    });
    // Start the shared REST re-seed scheduler (single task, serial, rate-limited)
    tokio::spawn({
        let reseed = Arc::clone(&reseed_task);
        async move { reseed.run().await }
    });

    // 5. Scan task — the single mutator of Scanner
    let scanner_clone = Arc::clone(&scanner);
    let sender_clone = opportunity_sender.clone();
    let telemetry_snapshot = Arc::clone(&telemetry_collector);
    tokio::spawn(async move {
        loop {
            let tick_start = std::time::Instant::now();
            {
                let mut locked = scanner_clone.lock().await;
                if locked.paused {
                    drop(locked);
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    continue;
                }
                let interval_ms = locked.tick_interval_ms;
                // Live settings: tick cadence + persistence requirement.
                if let Some(ref snap) = locked.settings {
                    let guard = futures::executor::block_on(snap.read());
                    let tick_ms = if guard.tick_interval_ms >= 50 {
                        Some(guard.tick_interval_ms)
                    } else {
                        None
                    };
                    let req_ticks = if guard.required_ticks > 0 {
                        Some(guard.required_ticks)
                    } else {
                        None
                    };
                    drop(guard);
                    if let Some(ms) = tick_ms {
                        locked.tick_interval_ms = ms;
                    }
                    if let Some(rt) = req_ticks {
                        locked.validator.required_ticks = rt;
                    }
                }
                let verified = locked.tick();
                for opp in verified {
                    if sender_clone.receiver_count() > 0 {
                        let _ = sender_clone.send(opp);
                    }
                }
                tokio::time::sleep(Duration::from_millis(interval_ms)).await;
            }
            let loop_ms = tick_start.elapsed().as_secs_f64() * 1000.0;
            // NOTE: loop_ms captured post-sleep; true math-loop timing approximated.
            tracing::trace!("scan tick ≈ {:.1}ms", loop_ms);
            telemetry_snapshot.set_math_loop_ms(loop_ms).await;
        }
    });

    // 6. Cleaner + telemetry
    let cleaner = Arc::new(CleanerTask::new(Arc::clone(&trade_logger)));
    tokio::spawn(async move { cleaner.start_scheduler().await });
    let telemetry_clone = Arc::clone(&telemetry_collector);
    tokio::spawn(async move { telemetry_clone.start_collector().await; });

    // ---- HTTP API ----
    let state = AppState {
        scanner: Arc::clone(&scanner),
        trade_logger: Arc::clone(&trade_logger),
        telemetry_collector: Arc::clone(&telemetry_collector),
        ws_pool: Arc::clone(&ws_pool),
        db: Arc::clone(&db),
        rest_client: Arc::clone(&rest_client),
        opportunity_sender: opportunity_sender.clone(),
        admin_password: admin_password.clone(),
        jwt_secret: jwt_secret.clone(),
        settings: shared_settings,
        math_engine,
    };

    let app = Router::new()
        // public
        .route("/api/health", get(health_handler))
        .route("/api/telemetry", get(telemetry_handler))
        .route("/api/live-pulse", get(live_pulse_sse_handler))
        .route("/api/whitelist", get(whitelist_handler))
        .route("/api/login", post(login_handler))
        // admin
        .route("/api/recent-opportunities", get(recent_opportunities_handler))
        .route("/api/all-opportunities", get(all_opportunities_handler))
        .route("/api/export-csv", get(export_csv_handler))
        .route("/api/today-stats", get(today_stats_handler))
        .route("/api/settings", get(get_settings_handler))
        .route("/api/settings", post(save_settings_handler))
        .route("/api/verify-keys", post(verify_keys_handler))
        .route("/api/keys", post(save_api_keys_handler))
        .route("/api/keys", get(get_api_keys_handler))
        .route("/api/keys/delete", post(delete_api_keys_handler))
        .route("/api/book/:sym", get(book_debug_handler))
        .route("/api/live-books", get(live_books_handler))
        .route("/api/key-tests", get(key_tests_handler))
        .fallback_service(ServeDir::new("frontend/dist"))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let port = std::env::var("PORT").unwrap_or_else(|_| "8080".to_string());
    let addr = format!("0.0.0.0:{}", port);
    tracing::info!("✅ Server listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app)
        .await?;

    Ok(())
}

// ============ Handlers ============

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

/// Temporary debug endpoint: dump the math engine's live book for a symbol.
/// Live books for up to 20 symbols: best bid/ask, last price, top 10 depth levels,
/// staleness — used by the dashboard opportunity detail page for live prices.
async fn live_books_handler(
    State(state): State<AppState>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Json<serde_json::Value> {
    let syms: Vec<String> = params
        .get("symbols")
        .map(|s| {
            s.split(',')
                .map(|p| p.trim().to_uppercase())
                .filter(|p| !p.is_empty())
                .take(20)
                .collect()
        })
        .unwrap_or_default();
    let out: Vec<serde_json::Value> = syms
        .iter()
        .map(|sym| {
            match state.math_engine.get_order_book(sym) {
                Some(b) => serde_json::json!({
                    "symbol": sym,
                    "best_bid": b.bids.iter().find(|l| l.price > 0.0).map(|l| l.price).unwrap_or(0.0),
                    "best_ask": b.asks.iter().find(|l| l.price > 0.0).map(|l| l.price).unwrap_or(0.0),
                    "last": b.bids.iter().find(|l| l.price > 0.0).map(|l| l.price)
                        .or_else(|| b.asks.iter().find(|l| l.price > 0.0).map(|l| l.price))
                        .unwrap_or(0.0),
                    "depth": serde_json::json!({
                        "bids": b.bids.iter().filter(|l| l.price > 0.0).take(10).map(|l| serde_json::json!([l.price, l.volume])).collect::<Vec<_>>(),
                        "asks": b.asks.iter().filter(|l| l.price > 0.0).take(10).map(|l| serde_json::json!([l.price, l.volume])).collect::<Vec<_>>(),
                    }),
                    "stale_ms": chrono::Utc::now().signed_duration_since(b.last_update_time).num_milliseconds(),
                }),
                None => serde_json::json!({ "symbol": sym, "error": "no book" }),
            }
        })
        .collect();
    Json(serde_json::json!({ "books": out }))
}

async fn book_debug_handler(
    State(state): State<AppState>,
    axum::extract::Path(sym): axum::extract::Path<String>,
) -> Json<serde_json::Value> {
    let engine = state.telemetry_collector.engine();
    let book = engine.get_order_book(&sym.to_uppercase());
    let out = match book {
        Some(b) => serde_json::json!({
            "symbol": sym,
            "bids": b.bids.iter().filter(|l| l.price > 0.0).take(5).map(|l| serde_json::json!([l.price, l.volume])).collect::<Vec<_>>(),
            "asks": b.asks.iter().filter(|l| l.price > 0.0).take(5).map(|l| serde_json::json!([l.price, l.volume])).collect::<Vec<_>>(),
            "stale_ms": chrono::Utc::now().signed_duration_since(b.last_update_time).num_milliseconds(),
        }),
        None => serde_json::json!({ "symbol": sym, "error": "no book" }),
    };
    Json(out)
}

async fn login_handler(
    State(state): State<AppState>,
    Json(payload): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let provided = payload["password"].as_str().unwrap_or("");
    if provided == state.admin_password {
        let exp = (Utc::now() + chrono::Duration::days(30)).timestamp() as usize;
        let token = encode(
            &Header::default(),
            &Claims { sub: "admin".into(), exp },
            &EncodingKey::from_secret(state.jwt_secret.as_bytes()),
        )
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        tracing::info!("✅ Login successful");
        Ok(Json(serde_json::json!({ "token": token, "expires_days": 30 })))
    } else {
        tracing::warn!("❌ Failed login attempt");
        Err((StatusCode::UNAUTHORIZED, "Invalid password".into()))
    }
}

async fn get_settings_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<SettingsSnapshot>, (StatusCode, String)> {
    check_auth(&state, &headers)?;
    Ok(Json(state.settings.read().await.clone()))
}

#[derive(Debug, Deserialize)]
struct SaveSettingsPayload {
    min_profit_threshold: Option<f64>,
    taker_fee: Option<f64>,
    slippage_buffer: Option<f64>,
    target_volume_usd: Option<f64>,
    tick_interval_ms: Option<u64>,
    required_ticks: Option<u8>,
    max_whitelist: Option<usize>,
    min_24h_volume_usd: Option<f64>,
    retention_days: Option<i32>,
    scan_paused: Option<bool>,
}

async fn save_settings_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(mut payload): Json<SaveSettingsPayload>,
) -> Result<Json<String>, (StatusCode, String)> {
    check_auth(&state, &headers)?;

    let mut snap = state
        .db
        .get_settings()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if let Some(v) = payload.min_profit_threshold { snap.min_profit_threshold = v; }
    if let Some(v) = payload.taker_fee { snap.taker_fee = v; }
    if let Some(v) = payload.slippage_buffer { snap.slippage_buffer = v; }
    if let Some(v) = payload.target_volume_usd { snap.target_volume_usd = v; }
    if let Some(v) = payload.tick_interval_ms { snap.tick_interval_ms = v; }
    if let Some(v) = payload.required_ticks { snap.required_ticks = v; }
    if let Some(v) = payload.max_whitelist { snap.max_whitelist = v; }
    if let Some(v) = payload.min_24h_volume_usd { snap.min_24h_volume_usd = v; }
    if let Some(v) = payload.retention_days { snap.retention_days = v; }

    snap.validate()
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;

    // Handle pause toggle live
    if let Some(paused) = payload.scan_paused {
        snap.scan_paused = paused;
        state.scanner.lock().await.paused = paused;
        tracing::info!("Scan {} by settings change", if paused { "PAUSED" } else { "RESUMED" });
    }

    state
        .db
        .save_settings(&snap)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Hot-apply: update the shared lock so the scanner tick / validator read the
    // new values immediately (no restart required).
    {
        let mut locked = state.settings.write().await;
        *locked = snap.clone();
    }

    tracing::info!("⚙️ Settings saved: {:?}", serde_json::to_string(&snap).unwrap_or_default());
    Ok(Json("Settings saved and applied".to_string()))
}

/// Credential verification BEFORE save (E1-E4). Runs real signed MEXC calls.
async fn verify_keys_handler(
    State(state): State<AppState>,
    Json(payload): Json<ApiKeyRequest>,
) -> Json<serde_json::Value> {
    let key = payload.api_key.trim();
    let secret = payload.secret_key.trim();

    let mut results = Vec::new();
    let now = Utc::now();

    // Test 1: signed GET /account — proves the key/secret pair + permissions
    match state
        .rest_client
        .signed_get(key, secret, "/api/v3/account", "recvWindow=5000")
        .await
    {
        Ok(body) => {
            let permissions: Vec<String> = body["permissions"]
                .as_array()
                .map(|a| a.iter().filter_map(|p| p.as_str().map(String::from)).collect())
                .unwrap_or_default();
            let spot_ok = permissions.contains(&"SPOT".to_string());
            results.push(KeyTestResult {
                checked_at: now,
                test_name: "Signed /account access".to_string(),
                passed: true,
                detail: format!(
                    "OK | spot permissions: {} | email: {}",
                    if spot_ok { "SPOT trading enabled" } else { "NO SPOT — bot can only read" },
                    body["email"].as_str().unwrap_or("?")
                ),
            });
            // Record sub-results
            results.push(KeyTestResult {
                checked_at: now,
                test_name: "Spot trading permission".to_string(),
                passed: spot_ok,
                detail: if spot_ok {
                    "Key authorized for spot trading".into()
                } else {
                    "Key lacks SPOT permission — trading execution will fail".into()
                },
            });
        }
        Err(e) => {
            let msg = e.to_string();
            results.push(KeyTestResult {
                checked_at: now,
                test_name: "Signed /account access".to_string(),
                passed: false,
                detail: if msg.contains("401") || msg.contains("signature") {
                    "INVALID KEY/SECRET — signature rejected by MEXC".into()
                } else if msg.contains("418") {
                    "IP NOT WHITELISTED — add this server's IP in MEXC key settings (HF container IPs change on restart)".into()
                } else if msg.contains("2006") || msg.to_lowercase().contains("timestamp") {
                    "TIMESTAMP ERROR — check server clock".into()
                } else {
                    format!("FAILED: {}", msg)
                },
            });
        }
    }

    // Test 2: unsigned GET /exchangeInfo — proves the endpoint path + reachability
    match state.rest_client.get_exchange_info().await {
        Ok(body) => {
            let n = body["symbols"].as_array().map(|a| a.len()).unwrap_or(0);
            results.push(KeyTestResult {
                checked_at: now,
                test_name: "Exchange reachability (/exchangeInfo)".to_string(),
                passed: true,
                detail: format!("OK | {} tradable symbols listed", n),
            });
        }
        Err(e) => {
            results.push(KeyTestResult {
                checked_at: now,
                test_name: "Exchange reachability (/exchangeInfo)".to_string(),
                passed: false,
                detail: format!("FAILED: {}", e),
            });
        }
    }

    // Test 3: signed order TEST (dry-run) — proves execution capability without risk
    match state
        .rest_client
        .signed_get(
            key,
            secret,
            "/api/v3/order/test",
            "symbol=BTCUSDT&side=BUY&type=LIMIT&timeInForce=GTC&quantity=0.0001&price=10000&recvWindow=5000",
        )
        .await
    {
        Ok(_) => {
            results.push(KeyTestResult {
                checked_at: now,
                test_name: "Dry-run order test (BTCUSDT limit BUY)".to_string(),
                passed: true,
                detail: "OK | order placement API reachable (no order placed)".into(),
            });
        }
        Err(e) => {
            let msg = e.to_string();
            results.push(KeyTestResult {
                checked_at: now,
                test_name: "Dry-run order test (BTCUSDT limit BUY)".to_string(),
                passed: msg.contains("418"),
                detail: if msg.contains("418") {
                    "IP restricted — add this IP to key whitelist in MEXC".into()
                } else {
                    format!("Note: {} (account access test is the authoritative check)", msg)
                },
            });
        }
    }

    // Persist
    for r in &results {
        let _ = state.db.record_key_test(r).await;
    }

    let all_passed = results.iter().all(|r| r.passed);
    Json(serde_json::json!({
        "all_passed": all_passed,
        "results": results,
        "save_recommended": all_passed
    }))
}

async fn save_api_keys_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<ApiKeyRequest>,
) -> Result<Json<String>, (StatusCode, String)> {
    check_auth(&state, &headers)?;
    state
        .db
        .save_api_keys(&payload)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    tracing::info!("✅ API keys saved (encrypted at rest)");
    Ok(Json("API keys saved successfully".to_string()))
}

async fn delete_api_keys_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<String>, (StatusCode, String)> {
    check_auth(&state, &headers)?;
    state
        .db
        .delete_api_keys()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json("API keys removed".to_string()))
}

async fn get_api_keys_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<bool>, (StatusCode, String)> {
    check_auth(&state, &headers)?;
    let keys = state.db.get_api_keys().await.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(keys.is_some()))
}

async fn key_tests_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<KeyTestResult>>, (StatusCode, String)> {
    check_auth(&state, &headers)?;
    match state.db.get_latest_key_tests().await {
        Ok(r) => Ok(Json(r)),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}

async fn live_pulse_sse_handler(
    State(state): State<AppState>,
) -> Sse<impl futures::Stream<Item = Result<Event, Infallible>>> {
    let rx = state.opportunity_sender.subscribe();
    let stream = tokio_stream::wrappers::BroadcastStream::new(rx)
        .filter_map(|msg| async move { msg.ok() })
        .map(|opportunity| Ok(Event::default().json_data(opportunity).unwrap()));
    Sse::new(stream)
}

async fn recent_opportunities_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<Opportunity>>, (StatusCode, String)> {
    check_auth(&state, &headers)?;
    match state.trade_logger.get_recent(200).await {
        Ok(ops) => Ok(Json(ops)),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}

async fn all_opportunities_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<Opportunity>>, (StatusCode, String)> {
    check_auth(&state, &headers)?;
    match state.trade_logger.persistence().get_all_opportunities().await {
        Ok(ops) => Ok(Json(ops)),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}

async fn export_csv_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, axum::response::Response), (StatusCode, String)> {
    check_auth(&state, &headers)?;
    let ops = state
        .trade_logger
        .persistence()
        .get_all_opportunities()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let mut csv = String::from("id,path,net_yield_pct,gross_gap_pct,fee_cost_pct,profit_usd,capacity_usd,optimal_size_usd,optimal_yield_pct,gap_age_ms,ticks_survived,fill_score,confidence,maker_plan_yield_pct,slippage_pct,leg1,leg1_entry,leg1_fill,leg2,leg2_entry,leg2_fill,leg3,leg3_entry,leg3_fill,detected_at\n");
    for o in &ops {
        csv.push_str(&format!(
            "{},{},{:.4},{:.4},{:.4},{:.2},{:.2},{:.2},{:.4},{},{},{},{:.2},{:.4},{:.4},{},{:.6},{:.6},{},{:.6},{:.6},{},{:.6},{:.6},{}\n",
            o.id, o.path, o.net_yield_percent, o.gross_gap_percent, o.fee_cost_percent,
            o.estimated_profit_usd, o.capacity_usd, o.optimal_size_usd, o.optimal_net_yield_percent,
            o.gap_age_ms, o.ticks_survived,
            o.fill_score, o.confidence, o.maker_plan_yield_percent, o.slippage_percent,
            o.leg1_symbol, o.leg1_entry_price, o.leg1_fill_price,
            o.leg2_symbol, o.leg2_entry_price, o.leg2_fill_price,
            o.leg3_symbol, o.leg3_entry_price, o.leg3_fill_price,
            o.detected_at.to_rfc3339(),
        ));
    }

    use axum::response::IntoResponse;
    let resp = axum::response::Response::builder()
        .header("Content-Type", "text/csv; charset=utf-8")
        .header("Content-Disposition", "attachment; filename=\"mexc_trades.csv\"")
        .body(csv.into())
        .unwrap();
    Ok((StatusCode::OK, resp))
}

async fn today_stats_handler(State(state): State<AppState>) -> Json<serde_json::Value> {
    match state.trade_logger.get_today_analytics().await {
        Ok((gaps, avg, total_profit)) => Json(serde_json::json!({
            "gaps_found": gaps,
            "avg_yield_pct": avg,
            "total_estimated_profit_usd": total_profit
        })),
        Err(e) => {
            tracing::warn!("today-stats failed: {}", e);
            Json(serde_json::json!({ "gaps_found": 0, "avg_yield_pct": 0.0, "total_estimated_profit_usd": 0.0 }))
        }
    }
}

async fn whitelist_handler(State(state): State<AppState>) -> Json<Vec<String>> {
    let pool = state.ws_pool.lock().await;
    let mut symbols = pool.current_symbols().to_vec();
    drop(pool);
    symbols.sort();
    Json(symbols)
}
