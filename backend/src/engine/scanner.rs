// backend/src/engine/scanner.rs
// USDT-Centric Triangle Enumeration Engine
//
// Architecture (master plan D1-D4):
// - Streams, never caches value: profit is re-derived from the live books every tick.
// - Topology (which loops exist) is cached from the subscribed symbol set and
//   refreshed when maintenance swaps symbols; only the VALUE recomputes each tick.
// - Only closed loops USDT -> COIN_A -> COIN_B -> USDT are ever generated.
//
// Thread model: Scanner is NOT sync-shared. main.rs owns `Mutex<Scanner>` and the
// single scan task is the only mutator. No unsafe needed.

use crate::data::models::{Opportunity, OrderBookLevels};
use crate::engine::calculator::validate_triangle_full;
use crate::engine::validator::TriangleValidator;
use crate::engine::MathEngine;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::time::{Duration, Instant};
use tracing;
use uuid::Uuid;

/// A cached topological candidate: USDT -> A -> B -> USDT
#[derive(Debug, Clone)]
pub struct TopologyLoop {
    pub id: Uuid,
    pub coin_a: String, // e.g. SOL (leg1 pair: SOLUSDT)
    pub coin_b: String, // e.g. ETH (leg3 pair: ETHUSDT, leg2 cross: ETHSOL)
    pub pair1: String,  // COINAUSDT — buy COIN_A with USDT
    pub pair2: String,  // COINBCOINA — sell COIN_A for COIN_B
    pub pair3: String,  // COINBUSDT — sell COIN_B for USDT
    pub generated_at: Instant,
}

pub struct Scanner {
    pub math_engine: Arc<MathEngine>,
    pub topology: Vec<TopologyLoop>,
    topology_updated_at: Instant,
    pub validator: TriangleValidator,
    pub tick_interval_ms: u64,
    pub paused: bool,
    pub settings: Option<std::sync::Arc<tokio::sync::RwLock<crate::data::models::SettingsSnapshot>>>,
    /// per-loop learning stats: (passed_checks, verified_emissions)
    pub loop_stats: HashMap<Uuid, (u64, u64)>,
    /// cooldown: last emission time per loop to avoid flooding identical
    /// opportunities every tick.
    last_emit: HashMap<Uuid, Instant>,
    /// Best gross gap observed across all loops in the current sample window.
    best_gap: Option<(f64, String, Instant)>,
    last_gap_log: Instant,
}

/// Minimum silence between two emissions of the same loop.
fn emit_cooldown() -> Duration {
    std::env::var("EMIT_COOLDOWN_MS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .map(Duration::from_millis)
        .unwrap_or_else(|| Duration::from_secs(5))
}

impl Scanner {
    pub fn new(math_engine: Arc<MathEngine>) -> Self {
        let tick_interval_ms = std::env::var("TICK_INTERVAL_MS")
            .unwrap_or_else(|_| "50".to_string())
            .parse()
            .unwrap_or(50);
        let max_ticks = std::env::var("REQUIRED_TICKS")
            .unwrap_or_else(|_| "3".to_string())
            .parse()
            .unwrap_or(3);

        Self {
            math_engine,
            topology: Vec::new(),
            topology_updated_at: Instant::now(),
            validator: TriangleValidator::new(3),
            tick_interval_ms: 50,
            paused: false,
            settings: None,
            loop_stats: HashMap::new(),
            last_emit: HashMap::new(),
            best_gap: None,
            last_gap_log: Instant::now(),
        }
    }

    /// Rebuild the USDT loop topology from the symbol list currently subscribed.
    /// USDT pairs give coin A/B; a cross pair COINBCOINA closes the loop.
    pub fn rebuild_topology(&mut self, symbols: &[String]) {
        let mut usdt_pairs: HashMap<String, String> = HashMap::new(); // coin -> symbol
        let mut cross_pairs: HashMap<String, String> = HashMap::new();

        for sym in symbols {
            if sym.ends_with("USDT") && sym.len() > 4 {
                let coin = sym.trim_end_matches("USDT").to_string();
                usdt_pairs.entry(coin).or_insert(sym.clone());
            } else {
                cross_pairs.insert(sym.to_string(), sym.clone());
            }
        }

        let mut loops = Vec::new();
        for coin_a in usdt_pairs.keys() {
            for coin_b in usdt_pairs.keys() {
                if coin_a == coin_b {
                    continue;
                }
                let cross = format!("{}{}", coin_b, coin_a); // ETHSOL
                if let Some(sym) = cross_pairs.get(&cross) {
                    // deterministic stable id from loop identity
                    let mut h = [0u8; 16];
                    let key = format!("{}->{}", coin_a, coin_b);
                    h[..key.len().min(16)]
                        .copy_from_slice(&key.as_bytes()[..key.len().min(16)]);
                    loops.push(TopologyLoop {
                        id: Uuid::from_bytes(h),
                        coin_a: coin_a.clone(),
                        coin_b: coin_b.clone(),
                        pair1: usdt_pairs[coin_a].clone(),
                        pair2: sym.clone(),
                        pair3: usdt_pairs[coin_b].clone(),
                        generated_at: Instant::now(),
                    });
                }
            }
        }

        tracing::info!(
            "Topology rebuilt: {} USDT loops from {} symbols",
            loops.len(),
            symbols.len()
        );
        self.topology = loops;
        self.topology_updated_at = Instant::now();
    }

    pub fn topology_age_ms(&self) -> u64 {
        self.topology_updated_at.elapsed().as_millis() as u64
    }

    pub fn loop_count(&self) -> usize {
        self.topology.len()
    }

    /// Single scan tick: re-derive value for every cached loop from live books.
    pub fn tick(&mut self) -> Vec<Opportunity> {
        let mut verified = Vec::new();

        // Sample the best live gross gap across all loops (5s window).
        let mut sampled_gap: Option<(f64, String)> = None;
        for topo in self.topology.iter() {
            let b1 = match self.math_engine.get_order_book(&topo.pair1) {
                Some(b) => b,
                None => continue,
            };
            let b2 = match self.math_engine.get_order_book(&topo.pair2) {
                Some(b) => b,
                None => continue,
            };
            let b3 = match self.math_engine.get_order_book(&topo.pair3) {
                Some(b) => b,
                None => continue,
            };

            if b1.is_stale(2000) || b2.is_stale(2000) || b3.is_stale(2000) {
                continue;
            }
            if top_of_book_precheck(&b1, &b2, &b3).is_none() {
                // Still track the best live gap for visibility even if it
                // doesn't clear the 0.5% precheck floor.
                if let Some(gross) = gross_gap(&b1, &b2, &b3) {
                    match sampled_gap {
                        None => sampled_gap = Some((gross, topo.pair2.clone())),
                        Some((cur, _)) if gross > cur => {
                            sampled_gap = Some((gross, topo.pair2.clone()));
                        }
                        _ => {}
                    }
                }
                continue;
            }

            let outcome = self.validator.validate_persistent(
                topo.id,
                &b1,
                &b2,
                &b3,
                topo.coin_a.clone(),
                topo.coin_b.clone(),
                topo.pair1.clone(),
                topo.pair2.clone(),
                topo.pair3.clone(),
            );

            if let Some(mut opp) = outcome {
                let entry = self.loop_stats.entry(topo.id).or_insert((0, 0));
                entry.0 += 1;
                entry.1 += 1;

                // Cooldown: same loop re-emits at most once per EMIT_COOLDOWN_MS.
                if let Some(last) = self.last_emit.get(&topo.id) {
                    if last.elapsed() < emit_cooldown() {
                        continue;
                    }
                }

                // Final freshness gate at emission: youngest book < 1s old
                let youngest = youngest_book_age_ms(&b1, &b2, &b3);
                if youngest > 1000 {
                    tracing::debug!(
                        "Loop {} passed persistence but books aged {}ms at emission",
                        topo.pair2,
                        youngest
                    );
                    continue;
                }
                opp.staleness_ms = youngest;

                // Confidence (WIN %): honest, per-emission score —
                // 60% order-book liquidity grade + 40% persistence (ticks the gap
                // survived). High WIN % = deep books + gap held across ticks.
                let grade_score = match opp.fill_score.as_str() {
                    "A" => 1.0,
                    "B" => 0.85,
                    "C" => 0.7,
                    "D" => 0.55,
                    _ => 0.3, // F / unknown
                };
                let ticks_score = (opp.ticks_survived as f64 / 10.0).min(1.0);
                opp.confidence = (grade_score * 0.6 + ticks_score * 0.4) * 100.0;

                self.last_emit.insert(topo.id, Instant::now());
                tracing::info!(
                    "VERIFIED {} net_yield={:.4}% capacity=${:.2} age={}ms path={} books=[L1asks={:?} L2bids={:?} L3bids={:?}]",
                    opp.id,
                    opp.net_yield_percent,
                    opp.capacity_usd,
                    opp.gap_age_ms,
                    opp.path,
                    b1.asks.iter().take(3).map(|l| (l.price, l.volume)).collect::<Vec<_>>(),
                    b2.asks.iter().take(3).map(|l| (l.price, l.volume)).collect::<Vec<_>>(),
                    b3.bids.iter().take(3).map(|l| (l.price, l.volume)).collect::<Vec<_>>()
                );
                verified.push(opp);
            } else {
                let entry = self.loop_stats.entry(topo.id).or_insert((0, 0));
                entry.0 += 1;
            }
        }

        self.validator.cleanup_old_entries(Duration::from_secs(30));

        // Publish the sampled best gap every 5 seconds (visibility, not noise).
        if let Some((g, path)) = sampled_gap {
            match &self.best_gap {
                None => self.best_gap = Some((g, path, Instant::now())),
                Some((cur, _, start)) if g > *cur || start.elapsed() >= Duration::from_secs(5) => {
                    self.best_gap = Some((g, path, Instant::now()));
                }
                Some(_) => {}
            }
        }
        if self.last_gap_log.elapsed() >= Duration::from_secs(5) {
            if let Some((g, path, _)) = self.best_gap.take() {
                tracing::info!("best live gross gap = {:.4}% via {} ({} loops scanned)", (g - 1.0) * 100.0, path, self.topology.len());
            }
            self.last_gap_log = Instant::now();
        }

        verified
    }

}

/// Instant top-of-book gross-product pre-filter. If the naive product of best
/// prices cannot exceed ~0.5% above 1.0, skip the heavy weighted-fill math.
/// Gross round-trip multiplier: USDT -ask1-> A -ask2-> B -bid3-> USDT.
fn gross_gap(b1: &OrderBookLevels, b2: &OrderBookLevels, b3: &OrderBookLevels) -> Option<f64> {
    let best_ask1 = b1.asks.iter().find(|l| l.price > 0.0)?;
    let best_ask2 = b2.asks.iter().find(|l| l.price > 0.0)?;
    let best_bid3 = b3.bids.iter().find(|l| l.price > 0.0)?;
    if best_ask2.price <= 0.0 {
        return None;
    }
    Some(best_bid3.price / (best_ask1.price * best_ask2.price))
}

fn top_of_book_precheck(b1: &OrderBookLevels, b2: &OrderBookLevels, b3: &OrderBookLevels) -> Option<()> {
    let gross = gross_gap(b1, b2, b3)?;
    if gross <= 1.005 {
        None
    } else {
        Some(())
    }
}

fn youngest_book_age_ms(b1: &OrderBookLevels, b2: &OrderBookLevels, b3: &OrderBookLevels) -> i64 {
    let ages = [
        chrono::Utc::now()
            .signed_duration_since(b1.last_update_time)
            .num_milliseconds(),
        chrono::Utc::now()
            .signed_duration_since(b2.last_update_time)
            .num_milliseconds(),
        chrono::Utc::now()
            .signed_duration_since(b3.last_update_time)
            .num_milliseconds(),
    ];
    *ages.iter().max().unwrap_or(&9999)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_topology_from_symbols() {
        let symbols = vec![
            "SOLUSDT".into(),
            "ETHUSDT".into(),
            "ETHSOL".into(),
            "BTCUSDT".into(),
            "BTCUSDC".into(),
        ];
        let engine = Arc::new(MathEngine::new());
        let mut scanner = Scanner::new(engine);
        scanner.rebuild_topology(&symbols);
        assert_eq!(scanner.topology.len(), 1);
        let t = &scanner.topology[0];
        assert_eq!(t.pair1, "SOLUSDT");
        assert_eq!(t.pair2, "ETHSOL");
        assert_eq!(t.pair3, "ETHUSDT");
    }

    #[test]
    fn test_no_loop_without_cross_pair() {
        let symbols = vec!["SOLUSDT".into(), "ETHUSDT".into(), "BTCUSDT".into()];
        let engine = Arc::new(MathEngine::new());
        let mut scanner = Scanner::new(engine);
        scanner.rebuild_topology(&symbols);
        assert_eq!(scanner.topology.len(), 0);
    }
}
