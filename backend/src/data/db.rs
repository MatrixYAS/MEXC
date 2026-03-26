// backend/src/data/db.rs
// SQLite Database Layer with WAL Mode + Connection Pooling
// Supports headless persistence of verified opportunities

use crate::data::models::{Opportunity, Triangle, WhitelistCoin, Telemetry};
use anyhow::Result;
use sqlx::{sqlite::SqlitePoolOptions, SqlitePool, Row};
use std::sync::Arc;
use chrono::Utc;

const DB_PATH: &str = "mexc.db";

pub struct Database {
    pool: SqlitePool,
}

impl Database {
    pub async fn new() -> Result<Self> {
        // Configure SQLite with WAL mode for concurrent read/write performance
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
        // Create tables if they don't exist
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

            CREATE INDEX IF NOT EXISTS idx_opportunities_detected_at ON opportunities(detected_at);
            CREATE INDEX IF NOT EXISTS idx_opportunities_net_yield ON opportunities(net_yield_percent);
            "#
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Batch insert verified opportunities (5-second batches as per PRD)
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

    /// Get recent verified opportunities for the "Verified Executions" page
    pub async fn get_recent_opportunities(&self, limit: i64) -> Result<Vec<Opportunity>> {
        let rows = sqlx::query_as::<_, Opportunity>(
            r#"
            SELECT * FROM opportunities 
            ORDER BY detected_at DESC 
            LIMIT ?
            "#
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows)
    }

    /// Get today's gap statistics for analytics
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

        Ok((
            count,
            avg_yield.unwrap_or(0.0),
            total_yield.unwrap_or(0.0),
        ))
    }

    /// Whitelist management for 24h Adaptive Maintenance
    pub async fn save_or_update_whitelist(&self, coins: &[WhitelistCoin]) -> Result<()> {
        for coin in coins {
            sqlx::query(
                r#"
                INSERT INTO whitelist_coins (symbol, volume_24h, path_count, is_active, last_updated)
                VALUES (?, ?, ?, ?, ?)
                ON CONFLICT(symbol) DO UPDATE SET
                    volume_24h = excluded.volume_24h,
                    path_count = excluded.path_count,
                    is_active = excluded.is_active,
                    last_updated = excluded.last_updated
                "#
            )
            .bind(&coin.symbol)
            .bind(coin.volume_24h)
            .bind(coin.path_count)
            .bind(coin.is_active)
            .bind(coin.last_updated.to_rfc3339())
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    pub async fn get_active_whitelist(&self) -> Result<Vec<WhitelistCoin>> {
        let coins = sqlx::query_as::<_, WhitelistCoin>(
            "SELECT * FROM whitelist_coins WHERE is_active = TRUE ORDER BY volume_24h DESC"
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(coins)
    }

    /// Auto-pruning: Delete logs older than 7 days (as per PRD)
    pub async fn prune_old_logs(&self) -> Result<u64> {
        let cutoff = (Utc::now() - chrono::Duration::days(7)).to_rfc3339();

        let result = sqlx::query(
            "DELETE FROM opportunities WHERE detected_at < ?"
        )
        .bind(cutoff)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected())
    }

    /// Get raw pool for advanced queries if needed
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }
}
