// backend/src/persistence/trade_logger.rs
// High-level Trade Logger with 5-second batching
// Ensures the math hot-path stays fast while still persisting every verified opportunity

use crate::data::models::Opportunity;
use crate::persistence::sqlite_pool::SqlitePersistence;
use anyhow::Result;
use std::sync::Arc;
use tokio::time::{interval, Duration};
use tracing;

#[derive(Clone)]
pub struct TradeLogger {
    persistence: Arc<SqlitePersistence>,
    is_enabled: bool,
}

impl TradeLogger {
    pub fn new(persistence: Arc<SqlitePersistence>) -> Self {
        Self {
            persistence,
            is_enabled: true,
        }
    }

    /// Queue a verified opportunity for batch writing (non-blocking for hot path)
    pub async fn log_verified_gap(&self, opportunity: Opportunity) {
        if !self.is_enabled {
            return;
        }

        self.persistence.queue_opportunity(opportunity).await;
    }

    /// Force flush current batch (useful on shutdown or manual trigger)
    pub async fn flush(&self) -> Result<usize> {
        self.persistence.flush_batch().await
    }

    /// Get recent opportunities for the UI "Verified Executions" page
    pub async fn get_recent(&self, limit: i64) -> Result<Vec<Opportunity>> {
        self.persistence.get_recent_opportunities(limit).await
    }

    /// Get today's analytics (Gaps Found Today, Average Gap Duration, Total Potential Yield)
    pub async fn get_today_analytics(&self) -> Result<(i64, f64, f64)> {
        self.persistence.get_today_stats().await
    }

    /// Auto-pruning job (called daily)
    pub async fn prune_old_logs(&self) -> Result<u64> {
        let deleted = self.persistence.prune_old_logs().await?;
        if deleted > 0 {
            tracing::info!("Pruned {} old opportunities from database", deleted);
        }
        Ok(deleted)
    }

    /// Enable / disable logging (useful for Paper Mode)
    pub fn set_enabled(&mut self, enabled: bool) {
        self.is_enabled = enabled;
    }

    /// Background batch flusher task (should be spawned in main.rs)
    pub async fn start_batch_flusher(self: Arc<Self>) {
        let mut ticker = interval(Duration::from_secs(5));

        loop {
            ticker.tick().await;

            match self.flush().await {
                Ok(count) if count > 0 => {
                    tracing::debug!("Flushed {} opportunities to SQLite", count);
                }
                Err(e) => {
                    tracing::error!("Failed to flush opportunity batch: {}", e);
                }
                _ => {}
            }
        }
    }
}
