// backend/src/network/wss_worker.rs
// Individual WebSocket Worker (handles up to 30 symbols per connection)
// MEXC Spot WebSocket: aggregated depth stream, protobuf-encoded
// (`spot@public.aggre.depth.v3.api.pb@100ms@{symbol}`).
//
// Incremental semantics: each push carries only the price levels that changed.
// A quantity of "0" means the level was removed. We apply these deltas on top
// of a full depth snapshot fetched from REST at startup.

use crate::data::models::{OrderBookLevels, PriceLevel};
use crate::engine::MathEngine;
use crate::network::pb::decode_depth_frame;
use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use url::Url;
use chrono::Utc;

const MEXC_WS_URL: &str = "wss://wbs-api.mexc.com/ws";
const MAX_SYMBOLS_PER_CONN: usize = 30;
const MAX_LEVELS: usize = 20;
/// Periodic full-book refresh interval. The aggregated pb stream does not
/// reliably push removal frames for consumed levels, so stale prices can
/// linger at the top of the book and create phantom arbitrage. A full REST
/// depth snapshot on a cadence keeps the books anchored to reality.
const RESNAP_INTERVAL: Duration = Duration::from_secs(60);

/// One WebSocket worker managing up to 30 symbols
pub struct WssWorker {
    symbols: Vec<String>,
    math_engine: Arc<MathEngine>,
    worker_id: usize,
    /// Shared rate-limited REST client — one global rate bucket for the whole
    /// bot, so all workers together stay under MEXC's IP rate limit.
    rest: Arc<crate::network::RestClient>,
}

/// Full-book state used to apply incremental depth deltas.
struct BookState {
    bids: HashMap<String, f64>,
    asks: HashMap<String, f64>,
}

impl WssWorker {
    pub fn new(
        symbols: Vec<String>,
        math_engine: Arc<MathEngine>,
        worker_id: usize,
        rest: Arc<crate::network::RestClient>,
    ) -> Self {
        Self {
            symbols,
            math_engine,
            worker_id,
            rest,
        }
    }

    /// Main run loop for this worker — reconnects forever with backoff.
    pub async fn run(&self) -> Result<()> {
        loop {
            match self.connect_and_stream().await {
                Ok(()) => {
                    tracing::info!("Worker {} connection closed cleanly", self.worker_id);
                }
                Err(e) => {
                    tracing::warn!(
                        "Worker {} connection error ({}); reconnecting in 3s: {}",
                        self.worker_id,
                        self.symbols.join(","),
                        e
                    );
                    tokio::time::sleep(Duration::from_secs(3)).await;
                    continue;
                }
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }

    async fn connect_and_stream(&self) -> Result<()> {
        let (mut ws_stream, _) = connect_async(MEXC_WS_URL.to_string()).await?;
        tracing::info!(
            "Worker {} connected with {} symbols",
            self.worker_id,
            self.symbols.len()
        );

        // Subscribe to aggregated depth streams for all symbols
        let subscription = serde_json::json!({
            "method": "SUBSCRIPTION",
            "params": self.symbols.iter().map(|s| {
                format!("spot@public.aggre.depth.v3.api.pb@100ms@{}", s)
            }).collect::<Vec<_>>()
        });
        ws_stream.send(Message::Text(subscription.to_string())).await?;

        // Boot seeding uses the single shared RestClient (one global rate
        // bucket for the whole bot). All workers draw from the same bucket,
        // so boot bursts never exceed the MEXC limit.
        // CRITICAL: seeding is a LONG blocking job (many REST calls); the WS
        // message loop MUST run immediately or MEXC's 100ms deltas are lost
        // and books go stale. Seed in the background, push incremental deltas
        // onto the books while seeding catches up, then replace them.
        let symbols_clone = self.symbols.clone();
        let rest_clone = Arc::clone(&self.rest);
        let worker_id_clone = self.worker_id;
        let (seed_tx, mut seed_rx) = tokio::sync::mpsc::channel::<(String, BookState)>(self.symbols.len() + 16);
        tokio::spawn(async move {
            for sym in &symbols_clone {
                let mut bids = HashMap::new();
                let mut asks = HashMap::new();
                let mut ok = true;
                match rest_clone.get_order_book_snapshot(sym, 100).await {
                    Ok(value) => {
                        if let Some(b) = value["bids"].as_array() {
                            for level in b {
                                if let (Some(p), Some(q)) = (level[0].as_str(), level[1].as_str()) {
                                    if let (Ok(_price), Ok(vol)) = (p.parse::<f64>(), q.parse::<f64>()) {
                                        if vol > 0.0 { bids.insert(p.to_string(), vol); }
                                    }
                                }
                            }
                        }
                        if let Some(a) = value["asks"].as_array() {
                            for level in a {
                                if let (Some(p), Some(q)) = (level[0].as_str(), level[1].as_str()) {
                                    if let (Ok(_price), Ok(vol)) = (p.parse::<f64>(), q.parse::<f64>()) {
                                        if vol > 0.0 { asks.insert(p.to_string(), vol); }
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Worker {} snapshot fetch failed for {} ({}); book starts from WS deltas",
                            worker_id_clone, sym, e
                        );
                        ok = false;
                    }
                }
                let _ = seed_tx.send((sym.to_string(), BookState { bids, asks })).await;
                if !ok {
                    let _ = seed_tx.send((sym.to_string(), BookState { bids: HashMap::new(), asks: HashMap::new() })).await;
                }
            }
            drop(seed_tx);
        });

        // Process incoming WS messages IMMEDIATELY (deltas land on top of an
        // empty or snapshot-seeded book — deltas accumulate while seeding runs)
        let mut seeded = 0usize;
        let mut books: HashMap<String, BookState> = HashMap::new();
        let ws_fut = async {
            while let Some(msg) = ws_stream.next().await {
            match msg {
                Ok(Message::Binary(bin)) => {
                    if let Err(e) = self.handle_binary(&bin, &mut books).await {
                        tracing::warn!("Worker {} frame error: {}", self.worker_id, e);
                    }
                }
                Ok(Message::Text(text)) => {
                    // JSON control frames (subscription confirmations) — ignore.
                    let _ = text;
                }
                Ok(Message::Ping(p)) => {
                    let _ = ws_stream.send(Message::Pong(p)).await;
                }
                Ok(Message::Pong(_)) | Ok(Message::Frame(_)) => {}
                        Ok(Message::Close(_)) => {
                    tracing::info!("Worker {} connection closed", self.worker_id);
                    break;
                }
                Err(e) => {
                    tracing::error!("Worker {} WebSocket error: {}", self.worker_id, e);
                    break;
                }
            }
        }
        };
        // Run the WS loop and the seed channel concurrently: both must be
        // drained so no deltas are lost and no snapshots are dropped.
        // Seed channel uses its OWN map (ws_fut borrows `books` mutably, so
        // the two futures cannot share it) — deltas and snapshots merge after.
        let mut seeds: HashMap<String, BookState> = HashMap::new();
        tokio::select! {
            _ = ws_fut => {}
            _ = async {
                while let Some((sym, snapshot)) = seed_rx.recv().await {
                    seeded += 1;
                    seeds.insert(sym, snapshot);
                }
            } => {}
        }
        // Merge: snapshot wins over WS deltas only while the delta map is
        // still empty for that symbol (deltas arriving after the snapshot are
        // always the freshest truth — never overwritten).
        for (sym, snapshot) in seeds {
            let entry = books.entry(sym.clone()).or_insert_with(|| BookState { bids: HashMap::new(), asks: HashMap::new() });
            if entry.bids.is_empty() && entry.asks.is_empty() {
                entry.bids = snapshot.bids;
                entry.asks = snapshot.asks;
            }
            self.push_book(&sym, entry);
        }

        Ok(())
    }

    /// Apply one protobuf depth frame as an incremental delta.
    async fn handle_binary(
        &self,
        bin: &[u8],
        books: &mut HashMap<String, BookState>,
    ) -> Result<()> {
        let depth = decode_depth_frame(bin).map_err(|e| anyhow::anyhow!(e))?;

        if !depth.channel.contains("aggre.depth") {
            return Ok(());
        }

        let symbol = depth.symbol.to_uppercase();

        // Normalize the symbol: MEXC pushes the subscription key which may
        // already be uppercase; keep the first match among our subscribed
        // symbols to stay consistent with the snapshot seeding.
        let canon = books
            .keys()
            .find(|k| k.eq_ignore_ascii_case(&symbol))
            .cloned()
            .unwrap_or(symbol);

        let book = books
            .entry(canon.clone())
            .or_insert_with(|| BookState { bids: HashMap::new(), asks: HashMap::new() });

        for (price, qty) in &depth.bids {
            apply_level(&mut book.bids, price, qty);
        }
        for (price, qty) in &depth.asks {
            apply_level(&mut book.asks, price, qty);
        }

        self.push_book(&canon, book);
        Ok(())
    }

    fn push_book(&self, symbol: &str, book: &BookState) {
        let mut bids: [PriceLevel; MAX_LEVELS] = [PriceLevel::default(); MAX_LEVELS];
        let mut asks: [PriceLevel; MAX_LEVELS] = [PriceLevel::default(); MAX_LEVELS];

        let mut bid_vec: Vec<(f64, f64)> = book
            .bids
            .iter()
            .filter_map(|(p, q)| Some((p.parse::<f64>().ok()?, *q)))
            .collect();
        bid_vec.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        for (i, (price, vol)) in bid_vec.into_iter().take(MAX_LEVELS).enumerate() {
            bids[i] = PriceLevel { price, volume: vol };
        }

        let mut ask_vec: Vec<(f64, f64)> = book
            .asks
            .iter()
            .filter_map(|(p, q)| Some((p.parse::<f64>().ok()?, *q)))
            .collect();
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
                let bytes = symbol.as_bytes();
                let len = bytes.len().min(16);
                arr[..len].copy_from_slice(&bytes[..len]);
                arr
            },
        };
        levels.update_time();

        // Update shared lock-free state
        self.math_engine.update_order_book(symbol.to_string(), levels);
    }
}

/// Apply an incremental level delta: qty == "0" removes the level.
fn apply_level(map: &mut HashMap<String, f64>, price: &str, qty: &str) {
    if let Ok(q) = qty.parse::<f64>() {
        if q == 0.0 {
            map.remove(price);
        } else {
            map.insert(price.to_string(), q);
        }
    }
}
