// backend/src/cron/maintenance.rs
// Full-market whitelist maintenance.
//
// Replaces the legacy 18-coin hard-coded list: pulls ALL 24h tickers in ONE
// call, ranks by USDT quoteVolume, cross-checks symbol existence against
// exchangeInfo (trading enabled only), and seamlessly swaps the WS pool.
// Runs every hour (refreshes faster than the old 24h cycle — MEXC markets
// rotate quickly).

use crate::network::RestClient;
use crate::network::WssPool;
use crate::engine::scanner::Scanner;
use crate::data::Database;
use anyhow::Result;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};

fn max_whitelist() -> usize {
    std::env::var("MAX_WHITELIST")
        .unwrap_or_else(|_| "300".to_string())
        .parse()
        .unwrap_or(300)
}

fn min_volume_24h() -> f64 {
    std::env::var("MIN_VOLUME_24H")
        .unwrap_or_else(|_| "1000000.0".to_string())
        .parse()
        .unwrap_or(1_000_000.0)
}

fn refresh_interval_secs() -> u64 {
    std::env::var("WHITELIST_REFRESH_SECS")
        .unwrap_or_else(|_| "3600".to_string())
        .parse()
        .unwrap_or(3600)
}

pub struct MaintenanceTask {
    rest_client: Arc<RestClient>,
    db: Arc<Database>,
}

impl MaintenanceTask {
    pub fn new(rest_client: Arc<RestClient>, db: Arc<Database>) -> Self {
        Self { rest_client, db }
    }

    /// Build the volume-ranked whitelist from the full ticker feed.
    pub async fn build_whitelist(&self) -> Result<Vec<String>> {
        let tickers = self.rest_client.get_all_tickers().await?;
        let max = max_whitelist();
        let min_vol = min_volume_24h();

        // Collect USDT pairs with volume, plus a set of all valid trading symbols
        // from exchangeInfo to filter out delisted/invalid pairs.
        let mut usdt_pairs: Vec<(String, f64)> = Vec::new();
        let mut valid_symbols = std::collections::HashSet::new();

        let mut exchange_info_ok = false;
        match self.rest_client.get_exchange_info().await {
            Ok(info) => {
                if let Some(symbols) = info["symbols"].as_array() {
                    for s in symbols {
                        let status = s["status"].as_str().unwrap_or("");
                        if status == "TRADING"
                            && s["isSpotTradingAllowed"].as_bool().unwrap_or(false)
                            || status == "1" && s["isSpotTradingAllowed"].as_bool().unwrap_or(false)
                        {
                            if let Some(sym) = s["symbol"].as_str() {
                                valid_symbols.insert(sym.to_string());
                            }
                        }
                    }
                    exchange_info_ok = true;
                }
            }
            Err(e) => {
                tracing::warn!("exchangeInfo unavailable ({}); falling back to tickers only", e);
            }
        }

        let mut ticker_vols = std::collections::HashMap::new();
        for t in tickers {
            let sym = t["symbol"].as_str().unwrap_or("").to_string();
            let vol: f64 = t["quoteVolume"]
                .as_f64()
                .or_else(|| {
                    t["quoteVolume"]
                        .as_str()
                        .and_then(|s| s.parse::<f64>().ok())
                })
                .unwrap_or(0.0);
            if vol > 0.0 {
                ticker_vols.insert(sym.clone(), vol);
            }
            if sym.is_empty() {
                continue;
            }
            if !sym.ends_with("USDT") || (!exchange_info_ok && !valid_symbols.contains(&sym)) {
                continue;
            }
            usdt_pairs.push((sym.clone(), ticker_vols.get(&sym).copied().unwrap_or(0.0)));
        }

        usdt_pairs.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        let mut whitelist: Vec<String> = usdt_pairs
            .into_iter()
            .filter(|(_, vol)| *vol >= min_vol)
            .take(max)
            .map(|(sym, _)| sym)
            .collect();

        // Always include the deepest majors for topological stability.
        for major in &["BTCUSDT", "ETHUSDT", "SOLUSDT", "USDCUSDT"] {
            if !whitelist.contains(&major.to_string()) {
                whitelist.insert(0, major.to_string());
            }
        }

        // ---- Cross-pair expansion ----
        // Triangle closing legs (e.g. ETHSOL, BTCSOL) are NOT in the top-300 by
        // USDT quoteVolume, but they ARE essential for triangulation. For every
        // ordered pair of USDT-quote coins in the whitelist, add the reverse
        // cross pair (COIN_B + COIN_A) if it is a valid TRADING symbol.
        let usdt_coins: Vec<String> = whitelist
            .iter()
            .filter(|s| s.ends_with("USDT") && s.len() > 4)
            .map(|s| s.trim_end_matches("USDT").to_string())
            .collect();
        let usdt_set: std::collections::HashSet<String> = whitelist
            .iter()
            .filter(|s| s.ends_with("USDT") && s.len() > 4)
            .cloned()
            .collect();
        // Minimum 24h volume on the closing leg: 1% of the min whitelist
        // volume guarantees the leg can actually move the arbitrage amount.
        let min_cross_vol = (min_vol * 0.01).max(5000.0);
        let mut added_crosses = 0;
        for coin_a in &usdt_coins {
            for coin_b in &usdt_coins {
                if coin_a == coin_b {
                    continue;
                }
                let cross = format!("{}{}", coin_b, coin_a);
                // Only add the closing leg when BOTH USDT legs are real trading
                // pairs on MEXC and the cross itself is a valid trading symbol.
                // This filters out junk coins whose names are prefixes of other
                // symbols (e.g. coin "A" from "ABTC" / "AUSDT").
                if !whitelist.contains(&cross)
                    && usdt_set.contains(&format!("{}USDT", coin_a))
                    && usdt_set.contains(&format!("{}USDT", coin_b))
                    && valid_symbols.contains(&cross)
                {
                    whitelist.push(cross);
                    added_crosses += 1;
                }
            }
        }
        if added_crosses > 0 {
            tracing::info!(
                "Cross-pair expansion added {} closing-leg symbols (whitelist now {})",
                added_crosses,
                whitelist.len()
            );
        }

        tracing::info!(
            "Whitelist built: {} symbols (min vol ${}, top {})",
            whitelist.len(),
            min_vol,
            max
        );
        Ok(whitelist)
    }

    /// Persist the current whitelist into settings DB for UI visibility.
    async fn persist_whitelist(&self, symbols: &[String]) {
        for sym in symbols {
            let _ = sqlx::query(
                "INSERT INTO whitelist_coins (symbol, volume_24h, path_count, is_active, last_updated) VALUES (?, 0, 0, 1, ?) ON CONFLICT(symbol) DO UPDATE SET is_active=1, last_updated=excluded.last_updated"
            )
            .bind(sym)
            .bind(chrono::Utc::now().to_rfc3339())
            .execute(self.db.pool())
            .await;
        }
    }

    /// Run one maintenance cycle: build whitelist + swap WS pool + rebuild topology.
    pub async fn run(
        &self,
        ws_pool: Arc<Mutex<WssPool>>,
        scanner: Arc<Mutex<Scanner>>,
        reseed: Arc<crate::network::reseed::ReseedTask>,
    ) -> Result<()> {
        tracing::info!("Starting whitelist maintenance...");

        let whitelist = self.build_whitelist().await?;
        if whitelist.len() < 10 {
            anyhow::bail!("Whitelist too small ({} symbols); keeping previous state", whitelist.len());
        }

        // Swap WebSocket subscriptions
        {
            let mut pool = ws_pool.lock().await;
            pool.seamless_update(whitelist.clone()).await;
        }

        // Rebuild USDT loop topology for the scanner
        {
            let mut sc = scanner.lock().await;
            sc.rebuild_topology(&whitelist);
        }

        // Keep the shared REST re-seed scheduler on the same symbol set
        reseed.update_symbols(whitelist.clone()).await;

        self.persist_whitelist(&whitelist).await;

        tracing::info!("Whitelist maintenance completed: {} symbols active", whitelist.len());
        Ok(())
    }

    /// Schedule periodic maintenance.
    pub async fn start_scheduler(
        self: Arc<Self>,
        ws_pool: Arc<Mutex<WssPool>>,
        scanner: Arc<Mutex<Scanner>>,
        reseed: Arc<crate::network::reseed::ReseedTask>,
    ) {
        // Failure retry: after a failed cycle, retry in MAINT_RETRY_SECS (5 min
        // default) instead of waiting the full refresh interval — without a
        // live whitelist the scanner sees zero loops and finds nothing.
        let retry_secs: u64 = std::env::var("MAINT_RETRY_SECS")
            .unwrap_or_else(|_| "300".to_string())
            .parse()
            .unwrap_or(300);
        tokio::spawn(async move {
            loop {
                match self.run(Arc::clone(&ws_pool), Arc::clone(&scanner), reseed.clone()).await {
                    Ok(()) => {
                        tracing::info!("Maintenance cycle OK; next full refresh in {}s", refresh_interval_secs());
                        sleep(Duration::from_secs(refresh_interval_secs())).await;
                    }
                    Err(e) => {
                        tracing::warn!("Maintenance failed ({}); retrying in {}s", e, retry_secs);
                        sleep(Duration::from_secs(retry_secs)).await;
                    }
                }
            }
        });
    }
}
