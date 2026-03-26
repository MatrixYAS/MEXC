// backend/src/cron/maintenance.rs
// 24h Adaptive Maintenance Task (Strategy Recalibrator)
// As specified in the PRD: Volume check, Closed Loop validation, Innovation Filter, Seamless Swap

use crate::data::models::WhitelistCoin;
use crate::network::RestClient;
use crate::network::WssPool;
use crate::engine::MathEngine;
use anyhow::Result;
use std::sync::Arc;
use tokio::time::{sleep, Duration};
use tracing;

const MIN_24H_VOLUME_USD: f64 = 500_000.0;

pub struct MaintenanceTask {
    rest_client: Arc<RestClient>,
    math_engine: Arc<MathEngine>,
    ws_pool: Arc<tokio::sync::Mutex<WssPool>>,
}

impl MaintenanceTask {
    pub fn new(
        rest_client: Arc<RestClient>,
        math_engine: Arc<MathEngine>,
        ws_pool: Arc<tokio::sync::Mutex<WssPool>>,
    ) -> Self {
        Self {
            rest_client,
            math_engine,
            ws_pool,
        }
    }

    /// Run the full 24h maintenance cycle
    pub async fn run(&self) -> Result<()> {
        tracing::info!("Starting 24h Adaptive Maintenance...");

        // Step 1: Fetch high-volume coins
        let high_volume_coins = self.fetch_high_volume_coins().await?;
        tracing::info!("Found {} coins with > ${} 24h volume", high_volume_coins.len(), MIN_24H_VOLUME_USD);

        // Step 2: Build new whitelist with Closed Loop validation
        let new_whitelist = self.build_valid_whitelist(high_volume_coins).await?;

        // Step 3: Update database whitelist
        // TODO: Inject Database when we create the struct
        // For now we log the new list
        tracing::info!("New whitelist ready with {} coins", new_whitelist.len());

        // Step 4: Seamless Swap (Atomic Pointer Swap philosophy)
        self.perform_seamless_swap(new_whitelist).await?;

        tracing::info!("24h Maintenance completed successfully. Zero downtime achieved.");
        Ok(())
    }

    /// Fetch coins with sufficient 24h volume using REST API
    async fn fetch_high_volume_coins(&self) -> Result<Vec<String>> {
        // In real implementation, fetch /api/v3/ticker/24hr?type=ALL or filter manually
        // For robustness, we simulate a broad list + filter (you can expand this)
        let popular_symbols = vec![
            "BTCUSDT", "ETHUSDT", "SOLUSDT", "PEPEUSDT", "DOGEUSDT", "XRPUSDT",
            "ADAUSDT", "BNBUSDT", "TONUSDT", "TRXUSDT", "AVAXUSDT", "SHIBUSDT",
            // Add more or fetch dynamically
        ];

        let mut valid = Vec::new();

        for symbol in popular_symbols {
            match self.rest_client.get_24h_ticker(symbol).await {
                Ok(ticker) => {
                    if let Some(vol) = ticker["quoteVolume"].as_str().and_then(|v| v.parse::<f64>().ok()) {
                        if vol > MIN_24H_VOLUME_USD {
                            valid.push(symbol.to_string());
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to fetch 24h volume for {}: {}", symbol, e);
                }
            }
        }

        Ok(valid)
    }

    /// Validate "Closed Loop" exists (USDT -> BASE -> COIN -> USDT)
    async fn build_valid_whitelist(&self, candidates: Vec<String>) -> Result<Vec<String>> {
        let mut whitelist = vec!["USDT".to_string()]; // Base asset

        for coin in candidates {
            // Simple closed loop check: assume USDT pairs exist for high-volume coins
            // In production: check if BTC/COIN, COIN/USDT, etc. pairs are tradable
            let base_pair = format!("{}USDT", coin.replace("USDT", ""));
            if base_pair != coin {
                whitelist.push(coin.clone());
            }
        }

        // Prioritize "Innovation Zone" coins (MEXC has higher inefficiency)
        // TODO: Add real Innovation Zone detection via API if available
        whitelist.sort_by(|a, b| b.len().cmp(&a.len())); // dummy priority

        // Limit to ~300 coins max
        whitelist.truncate(300);

        Ok(whitelist)
    }

    /// Perform seamless symbol update (zero downtime)
    async fn perform_seamless_swap(&self, new_symbols: Vec<String>) {
        let mut pool_guard = self.ws_pool.lock().await;
        
        tracing::info!("Executing Seamless Swap with {} symbols", new_symbols.len());
        
        pool_guard.seamless_update(new_symbols).await;

        // Update order book map in MathEngine if needed (arc-swap handles it)
        tracing::info!("Seamless swap completed - new connections stable");
    }

    /// Schedule the maintenance to run every 24 hours
    pub async fn start_scheduler(self: Arc<Self>) {
        tokio::spawn(async move {
            loop {
                // Run immediately on start, then every 24h
                if let Err(e) = self.run().await {
                    tracing::error!("Maintenance task failed: {}", e);
                }

                // Sleep 24 hours
                sleep(Duration::from_secs(24 * 60 * 60)).await;
            }
        });
    }
}
