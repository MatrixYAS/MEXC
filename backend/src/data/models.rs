// backend/src/data/models.rs
// Full data model: lock-free books, enriched Opportunity dossier (15+ fields),
// application Settings (all configurable in-app), and credential test results.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use sqlx::sqlite::SqliteRow;
use uuid::Uuid;

fn str_to_uuid(v: String) -> Uuid {
    Uuid::parse_str(&v).unwrap_or_else(|_| Uuid::nil())
}

fn str_to_dt(v: String) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(&v)
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}

impl Opportunity {
    pub fn from_row(r: &SqliteRow) -> Self {
        Self {
            id: str_to_uuid(r.get(0)),
            triangle_id: str_to_uuid(r.get(1)),
            path: r.get(2),
            net_yield_percent: r.get(3),
            gross_gap_percent: r.get(4),
            fee_cost_percent: r.get(5),
            estimated_profit_usd: r.get(6),
            capacity_usd: r.get(7),
            gap_age_ms: r.get(8),
            ticks_survived: r.get(9),
            fill_score: r.get(10),
            staleness_ms: r.get(11),
            confidence: r.get(12),
            maker_plan_yield_percent: r.get(13),
            slippage_percent: r.get(14),
            leg1_symbol: r.get(15),
            leg1_entry_price: r.get(16),
            leg1_fill_price: r.get(17),
            leg2_symbol: r.get(18),
            leg2_entry_price: r.get(19),
            leg2_fill_price: r.get(20),
            leg3_symbol: r.get(21),
            leg3_entry_price: r.get(22),
            leg3_fill_price: r.get(23),
            detected_at: str_to_dt(r.get(24)),
            is_executed: r.get(25),
        }
    }
}

impl KeyTestResult {
    pub fn from_row(r: &SqliteRow) -> Self {
        Self {
            checked_at: str_to_dt(r.get(0)),
            test_name: r.get(1),
            passed: r.get(2),
            detail: r.get(3),
        }
    }
}

impl ApiKeys {
    pub fn from_row(r: &SqliteRow) -> Self {
        Self {
            id: r.get(0),
            api_key: r.get(1),
            secret_key: r.get(2),
            created_at: str_to_dt(r.get(3)),
        }
    }
}

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
        let age = Utc::now()
            .signed_duration_since(self.last_update_time)
            .num_milliseconds();
        age > max_age_ms
    }

    pub fn update_time(&mut self) {
        self.last_update_time = Utc::now();
    }
}

// =============================================
// Opportunity Log — the 15-field trade dossier
// =============================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Opportunity {
    pub id: Uuid,
    pub triangle_id: Uuid,

    pub path: String,
    pub net_yield_percent: f64,
    pub gross_gap_percent: f64,
    pub fee_cost_percent: f64,
    pub estimated_profit_usd: f64,
    pub capacity_usd: f64,
    pub gap_age_ms: i64,
    pub ticks_survived: i32,
    pub fill_score: String,
    pub staleness_ms: i64,
    pub confidence: f64,
    pub maker_plan_yield_percent: f64,
    pub slippage_percent: f64,

    pub leg1_symbol: String,
    pub leg1_entry_price: f64,
    pub leg1_fill_price: f64,
    pub leg2_symbol: String,
    pub leg2_entry_price: f64,
    pub leg2_fill_price: f64,
    pub leg3_symbol: String,
    pub leg3_entry_price: f64,
    pub leg3_fill_price: f64,

    pub detected_at: DateTime<Utc>,
    pub is_executed: bool,
}

// =============================================
// Settings — everything configurable in-app
// =============================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSetting {
    pub key: String,
    pub value: String,
}

/// Canonical settings + defaults. Kept in sync with frontend Settings page.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingsSnapshot {
    pub min_profit_threshold: f64,   // e.g. 0.0025  (0.25% net after fees+buffer)
    pub taker_fee: f64,              // e.g. 0.0005  (0.05%)
    pub slippage_buffer: f64,        // e.g. 0.0005
    pub target_volume_usd: f64,      // discovery size, e.g. 1000
    pub tick_interval_ms: u64,       // e.g. 50
    pub required_ticks: u8,          // e.g. 3
    pub max_whitelist: usize,        // e.g. 300
    pub min_24h_volume_usd: f64,     // e.g. 1_000_000
    pub retention_days: i32,         // e.g. 7, 0 = forever
    pub scan_paused: bool,
    pub updated_at: String,
}

impl Default for SettingsSnapshot {
    fn default() -> Self {
        Self {
            min_profit_threshold: 0.0025,
            taker_fee: 0.0005,
            slippage_buffer: 0.0005,
            target_volume_usd: 1000.0,
            tick_interval_ms: 50,
            required_ticks: 3,
            max_whitelist: 300,
            min_24h_volume_usd: 1_000_000.0,
            retention_days: 7,
            scan_paused: false,
            updated_at: String::new(),
        }
    }
}

impl SettingsSnapshot {
    pub const KEYS: &'static [&'static str] = &[
        "min_profit_threshold",
        "taker_fee",
        "slippage_buffer",
        "target_volume_usd",
        "tick_interval_ms",
        "required_ticks",
        "max_whitelist",
        "min_24h_volume_usd",
        "retention_days",
        "scan_paused",
    ];

    /// Validate values before saving (called by the API handler).
    pub fn validate(&self) -> Result<(), String> {
        if !(0.0001..=0.1).contains(&self.min_profit_threshold) {
            return Err("min_profit_threshold must be 0.0001-0.1".into());
        }
        if !(0.0..=0.002).contains(&self.taker_fee) {
            return Err("taker_fee must be 0-0.002 (0-0.2%)".into());
        }
        if !(0.0..=0.01).contains(&self.slippage_buffer) {
            return Err("slippage_buffer must be 0-0.01".into());
        }
        if !(10.0..=100_000.0).contains(&self.target_volume_usd) {
            return Err("target_volume_usd must be 10-100000".into());
        }
        if !(10..=500).contains(&self.tick_interval_ms) {
            return Err("tick_interval_ms must be 10-500".into());
        }
        if !(2..=10).contains(&self.required_ticks) {
            return Err("required_ticks must be 2-10".into());
        }
        if !(10..=1000).contains(&self.max_whitelist) {
            return Err("max_whitelist must be 10-1000".into());
        }
        if !(10_000.0..=100_000_000.0).contains(&self.min_24h_volume_usd) {
            return Err("min_24h_volume_usd must be 10000-100000000".into());
        }
        if self.retention_days < 0 {
            return Err("retention_days must be >= 0 (0 = forever)".into());
        }
        Ok(())
    }

    pub fn snapshot_to_envs(&self) -> Vec<(&'static str, String)> {
        vec![
            ("MIN_PROFIT_THRESHOLD", format!("{}", self.min_profit_threshold)),
            ("TAKER_FEE", format!("{}", self.taker_fee)),
            ("SLIPPAGE_BUFFER", format!("{}", self.slippage_buffer)),
            ("TARGET_VOLUME_USD", format!("{}", self.target_volume_usd)),
            ("TICK_INTERVAL_MS", format!("{}", self.tick_interval_ms)),
            ("REQUIRED_TICKS", format!("{}", self.required_ticks)),
            ("MAX_WHITELIST", format!("{}", self.max_whitelist)),
            ("MIN_VOLUME_24H", format!("{}", self.min_24h_volume_usd)),
            ("RETENTION_DAYS", format!("{}", self.retention_days)),
        ]
    }
}

// =============================================
// Credential verification test results
// =============================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyTestResult {
    pub checked_at: DateTime<Utc>,
    pub test_name: String,
    pub passed: bool,
    pub detail: String,
}

// =============================================
// API Keys (encrypted at rest)
// =============================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeys {
    pub id: i64,
    pub api_key: String,
    pub secret_key: String,
    pub created_at: DateTime<Utc>,
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
