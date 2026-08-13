// backend/src/network/reseed.rs
// Single shared REST re-seed scheduler for all order books.
//
// Walks every subscribed symbol serially using the shared RestClient (and its
// shared rate-limit bucket), refreshing each book from the REST depth snapshot
// every RESNAP_INTERVAL. One task instead of per-worker tasks keeps the global
// request rate well inside MEXC's limits.

use crate::data::models::{OrderBookLevels, PriceLevel};
use crate::engine::MathEngine;
use crate::network::rest_client::RestClient;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use chrono::Utc;

const RESNAP_INTERVAL: Duration = Duration::from_secs(60);
const MAX_LEVELS: usize = 20;

pub struct ReseedTask {
    math_engine: Arc<MathEngine>,
    rest_client: Arc<RestClient>,
    symbols: Arc<RwLock<Vec<String>>>,
}

impl ReseedTask {
    pub fn new(math_engine: Arc<MathEngine>, rest_client: Arc<RestClient>) -> Self {
        Self {
            math_engine,
            rest_client,
            symbols: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Update the symbol set (called by maintenance when the whitelist changes).
    pub async fn update_symbols(&self, syms: Vec<String>) {
        let mut w = self.symbols.write().await;
        *w = syms;
    }

    /// Main loop: serial, paced re-seed of every symbol.
    pub async fn run(&self) {
        let mut ticker = tokio::time::interval(RESNAP_INTERVAL);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            ticker.tick().await;
            let syms = self.symbols.read().await.clone();
            let n = syms.len();
            tracing::debug!("Reseed cycle starting for {} symbols", n);
            for sym in &syms {
                match self.rest_client.get_order_book_snapshot(sym, 50).await {
                    Ok(value) => {
                        let mut bids = [PriceLevel::default(); MAX_LEVELS];
                        let mut asks = [PriceLevel::default(); MAX_LEVELS];
                        let mut bid_vec: Vec<(f64, f64)> = Vec::new();
                        if let Some(b) = value["bids"].as_array() {
                            for level in b {
                                if let (Some(p), Some(q)) = (level[0].as_str(), level[1].as_str()) {
                                    if let (Ok(price), Ok(vol)) = (p.parse::<f64>(), q.parse::<f64>()) {
                                        if vol > 0.0 {
                                            bid_vec.push((price, vol));
                                        }
                                    }
                                }
                            }
                        }
                        let mut ask_vec: Vec<(f64, f64)> = Vec::new();
                        if let Some(a) = value["asks"].as_array() {
                            for level in a {
                                if let (Some(p), Some(q)) = (level[0].as_str(), level[1].as_str()) {
                                    if let (Ok(price), Ok(vol)) = (p.parse::<f64>(), q.parse::<f64>()) {
                                        if vol > 0.0 {
                                            ask_vec.push((price, vol));
                                        }
                                    }
                                }
                            }
                        }
                        bid_vec.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
                        for (i, (price, vol)) in bid_vec.into_iter().take(MAX_LEVELS).enumerate() {
                            bids[i] = PriceLevel { price, volume: vol };
                        }
                        ask_vec.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
                        for (i, (price, vol)) in ask_vec.into_iter().take(MAX_LEVELS).enumerate() {
                            asks[i] = PriceLevel { price, volume: vol };
                        }
                        let mut levels = OrderBookLevels {
                            bids,
                            asks,
                            last_update_time: Utc::now(),
                            symbol: {
                                let mut arr = [0u8; 16];
                                let bytes = sym.as_bytes();
                                let len = bytes.len().min(16);
                                arr[..len].copy_from_slice(&bytes[..len]);
                                arr
                            },
                        };
                        levels.update_time();
                        self.math_engine.update_order_book(sym.to_uppercase(), levels);
                    }
                    Err(e) => {
                        tracing::warn!("Reseed failed for {} ({}); keeping current book", sym, e);
                        // On 429s, back off for the remainder of the cycle.
                        if e.to_string().contains("429") || e.to_string().contains("Too Many Requests") {
                            tokio::time::sleep(Duration::from_secs(30)).await;
                        }
                    }
                }
            }
        }
    }
}
