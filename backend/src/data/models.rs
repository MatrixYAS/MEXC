// backend/src/data/models.rs
// Updated with ApiKeys and ApiKeyRequest as required by the guide

use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;
use chrono::{DateTime, Utc};

// =============================================
// Order Book Models (Performance Critical)
// =============================================

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PriceLevel {
    pub price: f64,
    pub volume: f64,
}

impl Default for PriceLevel {
    fn default() -> Self {
        PriceLevel { price: 0.0, volume: 0.0 }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct OrderBookLevels {
    pub bids: [PriceLevel; 20],
    pub asks: [PriceLevel; 20],
    pub last_update_time: DateTime<Utc>,
    pub symbol: [u8; 16],
}

impl Default for OrderBookLevels {
    fn default() -> Self {
        OrderBookLevels {
            bids: [PriceLevel::default(); 20],
            asks: [PriceLevel::default(); 20],
            last_update_time: Utc::now(),
            symbol: [0; 16],
        }
    }
}

impl OrderBookLevels {
    pub fn is_stale(&self, max_age_ms: i64) -> bool {
        let age = Utc::now().signed_duration_since(self.last_update_time).num_milliseconds();
        age > max_age_ms
    }

    pub fn update_time(&mut self) {
        self.last_update_time = Utc::now();
    }
}

// =============================================
// Triangle / Arbitrage Path Models
// =============================================

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Triangle {
    pub id: Uuid,
    pub leg1: String,
    pub leg2: String,
    pub leg3: String,
    pub net_yield: f64,
    pub effective_capacity: f64,
    pub gap_age_ms: i64,
    pub fill_score: String,
    pub created_at: DateTime<Utc>,
    pub is_verified: bool,
}

impl Triangle {
    pub fn new(leg1: String, leg2: String, leg3: String, net_yield: f64, effective_capacity: f64) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            leg1,
            leg2,
            leg3,
            net_yield,
            effective_capacity,
            gap_age_ms: 0,
            fill_score: "C".to_string(),
            created_at: now,
            is_verified: false,
        }
    }

    pub fn path_string(&self) -> String {
        format!("{} → {} → {} → {}", 
            self.leg1.split('_').next().unwrap_or(""),
            self.leg2.split('_').next().unwrap_or(""),
            self.leg3.split('_').next().unwrap_or(""),
            self.leg1.split('_').next().unwrap_or("")
        )
    }
}

// =============================================
// Opportunity Log (Stored in SQLite)
// =============================================

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Opportunity {
    pub id: Uuid,
    pub triangle_id: Uuid,
    pub path: String,
    pub net_yield_percent: f64,
    pub capacity_usd: f64,
    pub gap_age_ms: i64,
    pub fill_score: String,
    pub detected_at: DateTime<Utc>,
    pub is_executed: bool,
}

impl Opportunity {
    pub fn from_triangle(triangle: &Triangle) -> Self {
        Self {
            id: Uuid::new_v4(),
            triangle_id: triangle.id,
            path: triangle.path_string(),
            net_yield_percent: triangle.net_yield * 100.0,
            capacity_usd: triangle.effective_capacity,
            gap_age_ms: triangle.gap_age_ms,
            fill_score: triangle.fill_score.clone(),
            detected_at: Utc::now(),
            is_executed: false,
        }
    }
}

// =============================================
// Whitelist Coin Model
// =============================================

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct WhitelistCoin {
    pub symbol: String,
    pub volume_24h: f64,
    pub path_count: i32,
    pub is_active: bool,
    pub last_updated: DateTime<Utc>,
}

impl WhitelistCoin {
    pub fn new(symbol: String, volume_24h: f64) -> Self {
        Self {
            symbol,
            volume_24h,
            path_count: 0,
            is_active: true,
            last_updated: Utc::now(),
        }
    }
}

// =============================================
// NEW: API Keys (Secure storage - per guide)
// =============================================

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ApiKeys {
    pub id: i64,
    pub api_key: String,
    pub secret_key: String,
    pub created_at: DateTime<Utc>,
}

impl ApiKeys {
    pub fn new(api_key: String, secret_key: String) -> Self {
        Self {
            id: 1, // single row for simplicity
            api_key,
            secret_key,
            created_at: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyRequest {
    pub api_key: String,
    pub secret_key: String,
}

// =============================================
// Telemetry & System Health
// =============================================

#[derive(Debug, Clone, Serialize)]
pub struct Telemetry {
    pub cpu_usage: f32,
    pub ram_usage_mb: u64,
    pub ws_latency_ms: f64,
    pub math_loop_time_ms: f64,
    pub active_triangles: usize,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub uptime_ms: i64,
    pub telemetry: Telemetry,
}

// Re-export everything for clean usage
pub use crate::data::models::PriceLevel; // already defined above
