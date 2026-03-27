// backend/src/persistence/sqlite_pool.rs
// Updated: Added get_db() helper for main.rs + improved batch writing

use crate::data::{Database, Opportunity};
use anyhow::Result;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct SqlitePersistence {
    db: Arc<Database>,
    batch_buffer: Arc<Mutex<Vec<Opportunity>>>,
}

impl SqlitePersistence {
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            db: Arc::clone(&db),
            batch_buffer: Arc::new(Mutex::new(Vec::with_capacity(100))),
        }
    }

    /// Add opportunity to batch (very cheap - called from hot path)
    pub async fn queue_opportunity(&self, opportunity: Opportunity) {
        let mut buffer = self.batch_buffer.lock().await;
        buffer.push(opportunity);

        // Auto-flush if buffer gets too large
        if buffer.len() > 80 {
            let _ = self.flush_batch().await;
        }
    }

    /// Flush batch to SQLite
    pub async fn flush_batch(&self) -> Result<usize> {
        let mut buffer = self.batch_buffer.lock().await;
        if buffer.is_empty() {
            return Ok(0);
        }

        let to_write: Vec<Opportunity> = buffer.drain(..).collect();
        let mut written = 0;

        for opp in to_write {
            if let Err(e) = self.db.log_opportunity(opp).await {
                tracing::error!("Failed to log opportunity: {}", e);
            } else {
                written += 1;
            }
        }

        Ok(written)
    }

    /// Direct log (fallback)
    pub async fn log_opportunity(&self, opportunity: Opportunity) -> Result<()> {
        self.db.log_opportunity(opportunity).await
    }

    pub async fn get_recent_opportunities(&self, limit: i64) -> Result<Vec<Opportunity>> {
        self.db.get_recent_opportunities(limit).await
    }

    pub async fn get_today_stats(&self) -> Result<(i64, f64, f64)> {
        self.db.get_today_stats().await
    }

    pub async fn prune_old_logs(&self) -> Result<u64> {
        self.db.prune_old_logs().await
    }

    // NEW HELPER: Expose underlying Database (required by main.rs)
    pub fn get_db(&self) -> Arc<Database> {
        Arc::clone(&self.db)
    }
}
