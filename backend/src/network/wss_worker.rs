// backend/src/network/wss_worker.rs
// Individual WebSocket Worker (handles up to 30 symbols per connection)
// MEXC Spot WebSocket using aggregated depth stream

use crate::data::models::{OrderBookLevels, PriceLevel};
use crate::engine::MathEngine;
use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use std::sync::Arc;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use url::Url;
use chrono::Utc;

const MEXC_WS_URL: &str = "wss://wbs-api.mexc.com/ws";
const MAX_SYMBOLS_PER_CONN: usize = 30;

/// One WebSocket worker managing up to 30 symbols
pub struct WssWorker {
    symbols: Vec<String>,
    math_engine: Arc<MathEngine>,
    worker_id: usize,
}

impl WssWorker {
    pub fn new(symbols: Vec<String>, math_engine: Arc<MathEngine>, worker_id: usize) -> Self {
        Self {
            symbols,
            math_engine,
            worker_id,
        }
    }

    /// Main run loop for this worker
    pub async fn run(&self) -> Result<()> {
        let url = Url::parse(MEXC_WS_URL)?;
        let (mut ws_stream, _) = connect_async(url).await?;

        println!("WssWorker {} connected with {} symbols", self.worker_id, self.symbols.len());

        // Subscribe to aggregated depth streams for all symbols
        let subscription = serde_json::json!({
            "method": "SUBSCRIPTION",
            "params": self.symbols.iter().map(|s| {
                format!("spot@public.aggre.depth.v3.api.pb@100ms@{}", s)
            }).collect::<Vec<_>>()
        });

        ws_stream.send(Message::Text(subscription.to_string())).await?;

        // Process incoming messages
        while let Some(msg) = ws_stream.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    if let Err(e) = self.handle_message(&text).await {
                        tracing::warn!("Worker {} message error: {}", self.worker_id, e);
                    }
                }
                Ok(Message::Close(_)) => {
                    tracing::info!("Worker {} connection closed", self.worker_id);
                    break;
                }
                Err(e) => {
                    tracing::error!("Worker {} WebSocket error: {}", self.worker_id, e);
                    break;
                }
                _ => {} // Ignore binary/pong etc.
            }
        }

        Ok(())
    }

    /// Parse MEXC aggregated depth update and convert to our stack-allocated OrderBookLevels
    async fn handle_message(&self, text: &str) -> Result<()> {
        let data: Value = serde_json::from_str(text)?;

        let channel = data["channel"].as_str().unwrap_or("");
        if !channel.contains("aggre.depth") {
            return Ok(());
        }

        let symbol = data["symbol"].as_str()
            .or_else(|| channel.split('@').last())
            .unwrap_or("")
            .to_uppercase();

        let depth_data = data["publicincreasedepths"].as_object()
            .or_else(|| data["data"].as_object())
            .unwrap_or(&serde_json::Map::new());

        let mut bids: [PriceLevel; 20] = [PriceLevel::default(); 20];
        let mut asks: [PriceLevel; 20] = [PriceLevel::default(); 20];

        // Parse bids
        if let Some(bids_list) = depth_data["bidsList"].as_array().or_else(|| depth_data["bids"].as_array()) {
            for (i, level) in bids_list.iter().take(20).enumerate() {
                if let (Some(price), Some(vol)) = (level[0].as_str(), level[1].as_str()) {
                    bids[i] = PriceLevel {
                        price: price.parse::<f64>().unwrap_or(0.0),
                        volume: vol.parse::<f64>().unwrap_or(0.0),
                    };
                }
            }
        }

        // Parse asks
        if let Some(asks_list) = depth_data["asksList"].as_array().or_else(|| depth_data["asks"].as_array()) {
            for (i, level) in asks_list.iter().take(20).enumerate() {
                if let (Some(price), Some(vol)) = (level[0].as_str(), level[1].as_str()) {
                    asks[i] = PriceLevel {
                        price: price.parse::<f64>().unwrap_or(0.0),
                        volume: vol.parse::<f64>().unwrap_or(0.0),
                    };
                }
            }
        }

        let mut levels = OrderBookLevels {
            bids,
            asks,
            last_update_time: Utc::now(),
            symbol: {
                let mut arr = [0u8; 16];
                let bytes = symbol.as_bytes();
                let len = bytes.len().min(16);
                arr[..len].copy_from_slice(&bytes[..len]);
                arr
            },
        };
        levels.update_time();

        // Update shared lock-free state
        self.math_engine.update_order_book(symbol, levels);

        Ok(())
    }
}
