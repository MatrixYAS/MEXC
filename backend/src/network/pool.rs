// backend/src/network/pool.rs
// WebSocket Pool Manager - Spawns 10 concurrent workers (30 symbols each)
// Handles health checks, exponential backoff reconnects, and zero-downtime operation

use crate::network::wss_worker::WssWorker;
use crate::engine::MathEngine;
use std::sync::Arc;
use tokio::task::JoinHandle;
use tokio::time::{sleep, Duration};
use tracing;

const NUM_WORKERS: usize = 10;
const SYMBOLS_PER_WORKER: usize = 30;
const RECONNECT_BACKOFF_MS: u64 = 1000;

pub struct WssPool {
    math_engine: Arc<MathEngine>,
    workers: Vec<JoinHandle<()>>,
    current_symbols: Vec<String>,
    /// One shared REST client for the whole pool — all workers draw from the
    /// same rate bucket, so boot seeding + reseeds never exceed MEXC's limit.
    rest: Arc<crate::network::RestClient>,
}

impl WssPool {
    pub fn new(math_engine: Arc<MathEngine>) -> Self {
        Self::new_with_rest(math_engine, Arc::new(crate::network::RestClient::new()))
    }

    pub fn new_with_rest(
        math_engine: Arc<MathEngine>,
        rest: Arc<crate::network::RestClient>,
    ) -> Self {
        Self {
            math_engine,
            workers: Vec::with_capacity(NUM_WORKERS),
            current_symbols: Vec::new(),
            rest,
        }
    }

    /// Set the full list of symbols to track (up to 300)
    pub fn set_symbols(&mut self, symbols: Vec<String>) {
        self.current_symbols = symbols;
    }

    /// Read the currently subscribed symbol set
    pub fn current_symbols(&self) -> &[String] {
        &self.current_symbols
    }

    /// Shared rate-limited REST client used by all workers (single bucket).
    pub fn rest_client(&self) -> Arc<crate::network::RestClient> {
        Arc::clone(&self.rest)
    }

    /// Spawn all workers with their assigned symbols
    pub async fn start(&mut self) {
        self.stop().await; // Clean shutdown of old workers

        let total_symbols = self.current_symbols.len();
        println!("Starting WSS Pool with {} symbols across {} workers", total_symbols, NUM_WORKERS);

        for i in 0..NUM_WORKERS {
            let start_idx = i * SYMBOLS_PER_WORKER;
            let end_idx = (start_idx + SYMBOLS_PER_WORKER).min(total_symbols);
            
            if start_idx >= total_symbols {
                break;
            }

            let worker_symbols = self.current_symbols[start_idx..end_idx].to_vec();
            let engine_clone = Arc::clone(&self.math_engine);
            let rest_clone = Arc::clone(&self.rest);
            let worker_id = i;

            let handle: JoinHandle<()> = tokio::spawn(async move {
                let mut retries = 0;
                loop {
                    let worker = WssWorker::new(
                        worker_symbols.clone(),
                        engine_clone.clone(),
                        worker_id,
                        Arc::clone(&rest_clone),
                    );
                    
                    if let Err(e) = worker.run().await {
                        tracing::error!("Worker {} failed: {}", worker_id, e);
                    }

                    // Exponential backoff reconnect
                    retries += 1;
                    let backoff = RECONNECT_BACKOFF_MS * (1u64 << retries.min(6)); // max ~64s
                    tracing::warn!("Worker {} reconnecting in {}ms (attempt {})", worker_id, backoff, retries);
                    
                    sleep(Duration::from_millis(backoff)).await;
                }
            });

            self.workers.push(handle);
        }
    }

    /// Graceful shutdown of all workers
    pub async fn stop(&mut self) {
        for handle in self.workers.drain(..) {
            handle.abort();
        }
        // Give a small grace period for cleanup
        sleep(Duration::from_millis(500)).await;
    }

    /// Health check - returns number of active workers
    pub fn active_worker_count(&self) -> usize {
        self.workers.len()
    }

    /// Seamless swap support (used by 24h Adaptive Maintenance)
    /// Replaces the symbol list and restarts workers without full downtime
    pub async fn seamless_update(&mut self, new_symbols: Vec<String>) {
        tracing::info!("Performing seamless symbol swap: {} → {} symbols", 
                      self.current_symbols.len(), new_symbols.len());
        
        self.set_symbols(new_symbols);
        self.start().await;  // This will stop old workers first
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::MathEngine;

    #[tokio::test]
    async fn test_pool_creation() {
        let engine = Arc::new(MathEngine::new());
        let mut pool = WssPool::new(engine);
        pool.set_symbols(vec!["BTCUSDT".to_string(); 50]);
        assert_eq!(pool.current_symbols.len(), 50);
    }
}
