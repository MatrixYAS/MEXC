// backend/src/data/db.rs
// Final update: Improved API key encryption, added get_db helper for main.rs, and full migrations

use crate::data::models::{Opportunity, Triangle, WhitelistCoin, ApiKeys, ApiKeyRequest};
use anyhow::Result;
use sqlx::{sqlite::SqlitePoolOptions, SqlitePool, Row};
use std::sync::Arc;
use chrono::Utc;

// Improved encryption using simple XOR + base64 (better than plain base64)
// In production, replace with `ring` or `aes-gcm` crate for real security
fn encrypt(data: &str) -> String {
    let salt = std::env::var("ENCRYPTION_SALT").unwrap_or_else(|_| "mexc-ghost-hunter-salt-2026".to_string());
    let mut encrypted = Vec::with_capacity(data.len());
    
    for (i, byte) in data.bytes().enumerate() {
        encrypted.push(byte ^ salt.as_bytes()[i % salt.len()]);
    }
    
    base64::encode(encrypted)
}

fn decrypt(encrypted: &str) -> String {
    let decoded = base64::decode(encrypted).unwrap_or_default();
    let salt = std::env::var("ENCRYPTION_SALT").unwrap_or_else(|_| "mexc-ghost-hunter-salt-2026".to_string());
    
    let mut decrypted = Vec::with_capacity(decoded.len());
    for (i, byte) in decoded.iter().enumerate() {
        decrypted.push(*byte ^ salt.as_bytes()[i % salt.len()]);
    }
    
    String::from_utf8(decrypted).unwrap_or_default()
}

const DB_PATH: &str = "mexc.db";

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
                    .filename(DB_PATH)
                    .create_if_missing(true)
                    .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
                    .synchronous(sqlx::sqlite::SqliteSynchronous::Normal)
                    .busy_timeout(std::time::Duration::from_secs(10))
            )
            .await?;

        let db = Self { pool };
        db.run_migrations().await?;
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
                capacity_usd REAL NOT NULL,
                gap_age_ms INTEGER NOT NULL,
                fill_score TEXT NOT NULL,
                detected_at TEXT NOT NULL,
                is_executed BOOLEAN DEFAULT FALSE
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

            CREATE INDEX IF NOT EXISTS idx_opportunities_detected_at ON opportunities(detected_at);
            CREATE INDEX IF NOT EXISTS idx_opportunities_net_yield ON opportunities(net_yield_percent);
            "#
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    // ====================== Opportunity & Stats methods ======================

    pub async fn log_opportunity(&self, opportunity: Opportunity) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO opportunities 
            (id, triangle_id, path, net_yield_percent, capacity_usd, gap_age_ms, fill_score, detected_at, is_executed)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#
        )
        .bind(opportunity.id.to_string())
        .bind(opportunity.triangle_id.to_string())
        .bind(&opportunity.path)
        .bind(opportunity.net_yield_percent)
        .bind(opportunity.capacity_usd)
        .bind(opportunity.gap_age_ms)
        .bind(&opportunity.fill_score)
        .bind(opportunity.detected_at.to_rfc3339())
        .bind(opportunity.is_executed)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn get_recent_opportunities(&self, limit: i64) -> Result<Vec<Opportunity>> {
        sqlx::query_as::<_, Opportunity>(
            "SELECT * FROM opportunities ORDER BY detected_at DESC LIMIT ?"
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| anyhow::anyhow!(e))
    }

    pub async fn get_today_stats(&self) -> Result<(i64, f64, f64)> {
        let today = Utc::now().date_naive().format("%Y-%m-%d").to_string();

        let row = sqlx::query(
            r#"
            SELECT 
                COUNT(*) as count,
                AVG(net_yield_percent) as avg_yield,
                SUM(net_yield_percent) as total_yield
            FROM opportunities 
            WHERE detected_at >= ? || ' 00:00:00'
            "#
        )
        .bind(today)
        .fetch_one(&self.pool)
        .await?;

        let count: i64 = row.get(0);
        let avg_yield: Option<f64> = row.get(1);
        let total_yield: Option<f64> = row.get(2);

        Ok((count, avg_yield.unwrap_or(0.0), total_yield.unwrap_or(0.0)))
    }

    pub async fn prune_old_logs(&self) -> Result<u64> {
        let cutoff = (Utc::now() - chrono::Duration::days(7)).to_rfc3339();
        let result = sqlx::query("DELETE FROM opportunities WHERE detected_at < ?")
            .bind(cutoff)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected())
    }

    // ====================== API Keys (Improved encryption) ======================

    pub async fn save_api_keys(&self, req: ApiKeyRequest) -> Result<()> {
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
        let row = sqlx::query_as::<_, ApiKeys>(
            "SELECT * FROM api_keys WHERE id = 1"
        )
        .fetch_optional(&self.pool)
        .await?;

        if let Some(mut keys) = row {
            keys.api_key = decrypt(&keys.api_key);
            keys.secret_key = decrypt(&keys.secret_key);
            Ok(Some(keys))
        } else {
            Ok(None)
        }
    }

    // Helper for main.rs to access pool if needed
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }
}
