// backend/src/cron/cleaner.rs
// Database Cleaner Task
// Handles auto-pruning of old logs (older than 7 days) as specified in the PRD

use crate::persistence::TradeLogger;
use std::sync::Arc;
use tokio::time::{interval, Duration};
use tracing;

pub struct CleanerTask {
    trade_logger: Arc<TradeLogger>,
}

impl CleanerTask {
    pub fn new(trade_logger: Arc<TradeLogger>) -> Self {
        Self { trade_logger }
    }

    /// Run the pruning job (removes opportunities older than 7 days)
    pub async fn run(&self) -> anyhow::Result<u64> {
        tracing::info!("Starting database cleanup (pruning old logs)...");
        
        let deleted_count = self.trade_logger.prune_old_logs().await?;
        
        if deleted_count > 0 {
            tracing::info!("✅ Cleaned up {} old opportunities from SQLite", deleted_count);
        } else {
            tracing::debug!("No old records to prune at this time.");
        }
        
        Ok(deleted_count)
    }

    /// Start the daily cleaner scheduler
    pub async fn start_scheduler(self: Arc<Self>) {
        let mut ticker = interval(Duration::from_secs(24 * 60 * 60)); // Every 24 hours

        // Run once immediately on startup
        if let Err(e) = self.run().await {
            tracing::error!("Initial cleanup failed: {}", e);
        }

        loop {
            ticker.tick().await;

            if let Err(e) = self.run().await {
                tracing::error!("Scheduled cleanup failed: {}", e);
            }
        }
    }
}
