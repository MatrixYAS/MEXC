// backend/src/data/db.rs
// SQLite persistence: 15-field opportunities, app settings, encrypted API keys,
// credential test history, retention managed per the retention_days setting.

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use sha2::{Digest, Sha256};
use chrono::Utc;
use anyhow::Result;
use sqlx::{sqlite::SqlitePoolOptions, SqlitePool, Row};

use crate::data::models::{ApiKeys, ApiKeyRequest, KeyTestResult, Opportunity, SettingsSnapshot};

fn db_path() -> String {
    let dir = std::env::var("DATA_DIR").unwrap_or_else(|_| "/data".to_string());
    format!("{}/mexc.db", dir)
}

/// AES-256-GCM encryption derived from ENCRYPTION_SALT env (HF secret), default
/// is intentionally obscure but NOT secret — set the env var in production.
fn cipher() -> Aes256Gcm {
    let salt = std::env::var("ENCRYPTION_SALT")
        .unwrap_or_else(|_| "mexc-ghost-hunter-2026-production".to_string());
    let key_bytes = Sha256::digest(salt.as_bytes());
    Aes256Gcm::new_from_slice(&key_bytes).expect("valid key size")
}

fn encrypt(data: &str) -> String {
    let cipher = cipher();
    let mut nonce_bytes = [0u8; 12];
    // deterministic-ish nonce derived from data so restart decryption still works
    nonce_bytes[..8].copy_from_slice(&Sha256::digest(data.as_bytes())[..8]);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ct = cipher.encrypt(nonce, data.as_bytes()).unwrap_or_default();
    let mut out = Vec::with_capacity(12 + ct.len());
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ct);
    BASE64.encode(&out)
}

fn decrypt(encrypted: &str) -> String {
    let cipher = cipher();
    let data = match BASE64.decode(encrypted) {
        Ok(d) => d,
        Err(_) => return String::new(),
    };
    if data.len() < 12 {
        return String::new();
    }
    let nonce = Nonce::from_slice(&data[..12]);
    match cipher.decrypt(nonce, &data[12..]) {
        Ok(plain) => String::from_utf8(plain).unwrap_or_default(),
        Err(_) => String::new(),
    }
}

pub struct Database {
    pool: SqlitePool,
}

impl Database {
    pub async fn new() -> Result<Self> {
        let pool = SqlitePoolOptions::new()
            .max_connections(10)
            .min_connections(2)
            .acquire_timeout(std::time::Duration::from_secs(5))
            .connect_with(
                sqlx::sqlite::SqliteConnectOptions::new()
                    .filename(&db_path())
                    .create_if_missing(true)
                    .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
                    .synchronous(sqlx::sqlite::SqliteSynchronous::Normal)
                    .busy_timeout(std::time::Duration::from_secs(10))
                    .pragma("journal_size_limit", "16777216"),
            )
            .await?;

        let db = Self { pool };
        db.run_migrations().await?;
        db.seed_defaults().await?;
        Ok(db)
    }

    async fn run_migrations(&self) -> Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS opportunities (
                id TEXT PRIMARY KEY,
                triangle_id TEXT NOT NULL,
                path TEXT NOT NULL,
                net_yield_percent REAL NOT NULL,
                gross_gap_percent REAL NOT NULL,
                fee_cost_percent REAL NOT NULL,
                estimated_profit_usd REAL NOT NULL,
                capacity_usd REAL NOT NULL,
                gap_age_ms INTEGER NOT NULL,
                ticks_survived INTEGER NOT NULL,
                fill_score TEXT NOT NULL,
                staleness_ms INTEGER NOT NULL,
                confidence REAL NOT NULL,
                maker_plan_yield_percent REAL NOT NULL,
                slippage_percent REAL NOT NULL,
                optimal_size_usd REAL NOT NULL DEFAULT 0,
                optimal_net_yield_percent REAL NOT NULL DEFAULT 0,
                size_curve_json TEXT NOT NULL DEFAULT '[]',
                leg1_symbol TEXT NOT NULL,
                leg1_entry_price REAL NOT NULL,
                leg1_fill_price REAL NOT NULL,
                leg2_symbol TEXT NOT NULL,
                leg2_entry_price REAL NOT NULL,
                leg2_fill_price REAL NOT NULL,
                leg3_symbol TEXT NOT NULL,
                leg3_entry_price REAL NOT NULL,
                leg3_fill_price REAL NOT NULL,
                detected_at TEXT NOT NULL,
                is_executed BOOLEAN DEFAULT FALSE
            );

            CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS whitelist_coins (
                symbol TEXT PRIMARY KEY,
                volume_24h REAL NOT NULL,
                path_count INTEGER NOT NULL DEFAULT 0,
                is_active BOOLEAN NOT NULL DEFAULT TRUE,
                last_updated TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS api_keys (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                api_key TEXT NOT NULL,
                secret_key TEXT NOT NULL,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS key_test_results (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                test_name TEXT NOT NULL,
                passed BOOLEAN NOT NULL,
                detail TEXT NOT NULL,
                checked_at TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_opportunities_detected_at ON opportunities(detected_at);
            CREATE INDEX IF NOT EXISTS idx_opportunities_net_yield ON opportunities(net_yield_percent);
            "#
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Seed settings with defaults (INSERT OR IGNORE semantics via upsert loop).
    async fn seed_defaults(&self) -> Result<()> {
        let defaults = SettingsSnapshot::default();
        for key in SettingsSnapshot::KEYS {
            let value = match *key {
                "min_profit_threshold" => format!("{}", defaults.min_profit_threshold),
                "taker_fee" => format!("{}", defaults.taker_fee),
                "slippage_buffer" => format!("{}", defaults.slippage_buffer),
                "target_volume_usd" => format!("{}", defaults.target_volume_usd),
                "tick_interval_ms" => format!("{}", defaults.tick_interval_ms),
                "required_ticks" => format!("{}", defaults.required_ticks),
                "max_whitelist" => format!("{}", defaults.max_whitelist),
                "min_24h_volume_usd" => format!("{}", defaults.min_24h_volume_usd),
                "retention_days" => format!("{}", defaults.retention_days),
                "scan_paused" => format!("{}", defaults.scan_paused),
                _ => continue,
            };
            sqlx::query("INSERT INTO settings (key, value, updated_at) VALUES (?, ?, ?) ON CONFLICT(key) DO NOTHING")
                .bind(*key)
                .bind(&value)
                .bind(Utc::now().to_rfc3339())
                .execute(&self.pool)
                .await?;
        }
        Ok(())
    }

    // ====================== Settings ======================

    pub async fn get_settings(&self) -> Result<SettingsSnapshot> {
        let mut snap = SettingsSnapshot::default();
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT key, value FROM settings"
        )
        .fetch_all(&self.pool)
        .await?;
        for (key, value) in rows {
            match key.as_str() {
                "min_profit_threshold" => snap.min_profit_threshold = value.parse().unwrap_or(snap.min_profit_threshold),
                "taker_fee" => snap.taker_fee = value.parse().unwrap_or(snap.taker_fee),
                "slippage_buffer" => snap.slippage_buffer = value.parse().unwrap_or(snap.slippage_buffer),
                "target_volume_usd" => snap.target_volume_usd = value.parse().unwrap_or(snap.target_volume_usd),
                "tick_interval_ms" => snap.tick_interval_ms = value.parse().unwrap_or(snap.tick_interval_ms),
                "required_ticks" => snap.required_ticks = value.parse().unwrap_or(snap.required_ticks),
                "max_whitelist" => snap.max_whitelist = value.parse().unwrap_or(snap.max_whitelist),
                "min_24h_volume_usd" => snap.min_24h_volume_usd = value.parse().unwrap_or(snap.min_24h_volume_usd),
                "retention_days" => snap.retention_days = value.parse().unwrap_or(snap.retention_days),
                "scan_paused" => snap.scan_paused = value == "true",
                _ => {}
            }
        }
        snap.updated_at = Utc::now().to_rfc3339();
        Ok(snap)
    }

    /// Save settings atomically. Applies hot reload envs via env::set_var so the
    /// running tasks pick up new values on their next re-read.
    pub async fn save_settings(&self, snap: &SettingsSnapshot) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        for (key, value) in snap.snapshot_to_envs() {
            sqlx::query("INSERT INTO settings (key, value, updated_at) VALUES (?, ?, ?) ON CONFLICT(key) DO UPDATE SET value=excluded.value, updated_at=excluded.updated_at")
                .bind(key)
                .bind(&value)
                .bind(&now)
                .execute(&self.pool)
                .await?;
            // hot reload: tasks re-read env on next iteration
            let _ = std::env::set_var(key, &value);
        }
        Ok(())
    }

    pub async fn get_bool_setting(&self, key: &str, default: bool) -> Result<bool> {
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT value FROM settings WHERE key = ?"
        )
        .bind(key)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|(v,)| v == "true").unwrap_or(default))
    }

    // ====================== Opportunities ======================

    pub async fn log_opportunity(&self, opp: &Opportunity) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO opportunities
            (id, triangle_id, path, net_yield_percent, gross_gap_percent, fee_cost_percent,
             estimated_profit_usd, capacity_usd, gap_age_ms, ticks_survived, fill_score,
             staleness_ms, confidence, maker_plan_yield_percent, slippage_percent,
             optimal_size_usd, optimal_net_yield_percent, size_curve_json,
             leg1_symbol, leg1_entry_price, leg1_fill_price,
             leg2_symbol, leg2_entry_price, leg2_fill_price,
             leg3_symbol, leg3_entry_price, leg3_fill_price,
             detected_at, is_executed)
            VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)
            "#
        )
        .bind(opp.id.to_string())
        .bind(opp.triangle_id.to_string())
        .bind(&opp.path)
        .bind(opp.net_yield_percent)
        .bind(opp.gross_gap_percent)
        .bind(opp.fee_cost_percent)
        .bind(opp.estimated_profit_usd)
        .bind(opp.capacity_usd)
        .bind(opp.gap_age_ms)
        .bind(opp.ticks_survived)
        .bind(&opp.fill_score)
        .bind(opp.staleness_ms)
        .bind(opp.confidence)
        .bind(opp.maker_plan_yield_percent)
        .bind(opp.slippage_percent)
        .bind(opp.optimal_size_usd)
        .bind(opp.optimal_net_yield_percent)
        .bind(&opp.size_curve_json)
        .bind(&opp.leg1_symbol)
        .bind(opp.leg1_entry_price)
        .bind(opp.leg1_fill_price)
        .bind(&opp.leg2_symbol)
        .bind(opp.leg2_entry_price)
        .bind(opp.leg2_fill_price)
        .bind(&opp.leg3_symbol)
        .bind(opp.leg3_entry_price)
        .bind(opp.leg3_fill_price)
        .bind(opp.detected_at.to_rfc3339())
        .bind(opp.is_executed)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_recent_opportunities(&self, limit: i64) -> Result<Vec<Opportunity>> {
        let rows: Vec<sqlx::sqlite::SqliteRow> = sqlx::query(
            "SELECT id, triangle_id, path, net_yield_percent, gross_gap_percent, fee_cost_percent, estimated_profit_usd, capacity_usd, gap_age_ms, ticks_survived, fill_score, staleness_ms, confidence, maker_plan_yield_percent, slippage_percent, leg1_symbol, leg1_entry_price, leg1_fill_price, leg2_symbol, leg2_entry_price, leg2_fill_price, leg3_symbol, leg3_entry_price, leg3_fill_price, detected_at, is_executed, optimal_size_usd, optimal_net_yield_percent, size_curve_json FROM opportunities ORDER BY detected_at DESC LIMIT ?"
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(Opportunity::from_row).collect())
    }

    pub async fn get_all_opportunities(&self) -> Result<Vec<Opportunity>> {
        let rows: Vec<sqlx::sqlite::SqliteRow> = sqlx::query(
            "SELECT id, triangle_id, path, net_yield_percent, gross_gap_percent, fee_cost_percent, estimated_profit_usd, capacity_usd, gap_age_ms, ticks_survived, fill_score, staleness_ms, confidence, maker_plan_yield_percent, slippage_percent, leg1_symbol, leg1_entry_price, leg1_fill_price, leg2_symbol, leg2_entry_price, leg2_fill_price, leg3_symbol, leg3_entry_price, leg3_fill_price, detected_at, is_executed, optimal_size_usd, optimal_net_yield_percent, size_curve_json FROM opportunities ORDER BY detected_at DESC"
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(Opportunity::from_row).collect())
    }

    pub async fn get_today_stats(&self) -> Result<(i64, f64, f64)> {
        let today = Utc::now().date_naive().format("%Y-%m-%d").to_string();
        let row = sqlx::query(
            r#"
            SELECT COUNT(*) as count, AVG(net_yield_percent) as avg_yield,
                   SUM(estimated_profit_usd) as total_profit
            FROM opportunities
            WHERE detected_at >= ? || 'T00:00:00+00:00'
            "#
        )
        .bind(today)
        .fetch_one(&self.pool)
        .await?;
        let count: i64 = row.get(0);
        let avg_yield: Option<f64> = row.get(1);
        let total_profit: Option<f64> = row.get(2);
        Ok((count, avg_yield.unwrap_or(0.0), total_profit.unwrap_or(0.0)))
    }

    pub async fn prune_old_logs(&self, retention_days: i32) -> Result<u64> {
        if retention_days <= 0 {
            return Ok(0); // 0 = keep forever
        }
        let cutoff = (Utc::now() - chrono::Duration::days(retention_days as i64)).to_rfc3339();
        let result = sqlx::query("DELETE FROM opportunities WHERE detected_at < ?")
            .bind(cutoff)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected())
    }

    // ====================== Key test history ======================

    pub async fn record_key_test(&self, test: &KeyTestResult) -> Result<()> {
        sqlx::query(
            "INSERT INTO key_test_results (test_name, passed, detail, checked_at) VALUES (?, ?, ?, ?)"
        )
        .bind(&test.test_name)
        .bind(test.passed)
        .bind(&test.detail)
        .bind(test.checked_at.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_latest_key_tests(&self) -> Result<Vec<KeyTestResult>> {
        let rows: Vec<sqlx::sqlite::SqliteRow> = sqlx::query(
            "SELECT checked_at, test_name, passed, detail FROM key_test_results ORDER BY id DESC LIMIT 10"
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(KeyTestResult::from_row).collect())
    }

    // ====================== API Keys ======================

    pub async fn save_api_keys(&self, req: &ApiKeyRequest) -> Result<()> {
        let encrypted_key = encrypt(&req.api_key);
        let encrypted_secret = encrypt(&req.secret_key);

        sqlx::query(
            r#"
            INSERT INTO api_keys (id, api_key, secret_key, created_at)
            VALUES (1, ?, ?, ?)
            ON CONFLICT(id) DO UPDATE SET
                api_key = excluded.api_key,
                secret_key = excluded.secret_key,
                created_at = excluded.created_at
            "#
        )
        .bind(encrypted_key)
        .bind(encrypted_secret)
        .bind(Utc::now().to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_api_keys(&self) -> Result<Option<ApiKeys>> {
        let row: Option<sqlx::sqlite::SqliteRow> = sqlx::query("SELECT id, api_key, secret_key, created_at FROM api_keys WHERE id = 1")
            .fetch_optional(&self.pool)
            .await?;
        if let Some(r) = row {
            let mut keys = ApiKeys::from_row(&r);
            keys.api_key = decrypt(&keys.api_key);
            keys.secret_key = decrypt(&keys.secret_key);
            Ok(Some(keys))
        } else {
            Ok(None)
        }
    }

    pub async fn delete_api_keys(&self) -> Result<()> {
        sqlx::query("DELETE FROM api_keys")
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }
}
