// backend/src/persistence/trade_logger.rs
// Updated: Added get_db() helper + improved batch writing + today analytics

use crate::data::models::Opportunity;
use crate::persistence::sqlite_pool::SqlitePersistence;
use crate::data::Database; // for get_db helper
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

    /// Queue a verified opportunity for batch writing
    pub async fn log_verified_gap(&self, opportunity: Opportunity) {
        if !self.is_enabled {
            return;
        }
        self.persistence.queue_opportunity(opportunity).await;
    }

    /// Force flush current batch
    pub async fn flush(&self) -> Result<usize> {
        self.persistence.flush_batch().await
    }

    pub async fn get_recent(&self, limit: i64) -> Result<Vec<Opportunity>> {
        self.persistence.get_recent_opportunities(limit).await
    }

    /// NEW: Get today's analytics (used by /api/today-stats)
    pub async fn get_today_analytics(&self) -> Result<(i64, f64, f64)> {
        self.persistence.get_today_stats().await
    }

    pub async fn prune_old_logs(&self) -> Result<u64> {
        self.persistence.prune_old_logs().await
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.is_enabled = enabled;
    }

    /// Background batch flusher
    pub async fn start_batch_flusher(self: Arc<Self>) {
        let mut ticker = interval(Duration::from_secs(5));

        loop {
            ticker.tick().await;

            match self.flush().await {
                Ok(count) if count > 0 => {
                    tracing::debug!("Flushed {} opportunities to SQLite", count);
                }
                Err(e) => {
                    tracing::error!("Failed to flush batch: {}", e);
                }
                _ => {}
            }
        }
    }

    // NEW HELPER: Allow main.rs to access the underlying Database if needed
    pub fn get_db(&self) -> Arc<Database> {
        self.persistence.get_db()
    }
}
