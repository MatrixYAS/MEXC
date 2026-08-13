// backend/src/engine/validator.rs
// Triangle persistence filter (anti-ghost) + enriched opportunity builder.
//
// Tracks only *freshness facts* per loop (first_seen, tick count) — never a stored
// profit. The profit value is always recomputed from the live books by the scanner.
// When a loop passes the required consecutive ticks, an Opportunity dossier is
// built with the full 15-field payload, recomputed one final time from the freshest
// books before emission.

use crate::data::models::{Opportunity, OrderBookLevels, PriceLevel};
use crate::engine::calculator::{validate_triangle_full, FillReport};
use std::collections::HashMap;
use tokio::time::{Duration, Instant};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct PersistenceState {
    pub first_seen: Instant,
    pub last_seen: Instant,
    pub consecutive_ticks: u8,
    pub last_net_yield: f64,
    pub best_capacity: f64,
    pub best_fill_report: Option<FillReport>,
}

impl Default for PersistenceState {
    fn default() -> Self {
        let now = Instant::now();
        Self {
            first_seen: now,
            last_seen: now,
            consecutive_ticks: 0,
            last_net_yield: 0.0,
            best_capacity: 0.0,
            best_fill_report: None,
        }
    }
}

pub struct TriangleValidator {
    persistence_map: HashMap<Uuid, PersistenceState>,
    min_profit_threshold: f64,
    required_ticks: u8,
    /// loop identity -> (coin_a, coin_b, pair1, pair2, pair3) for dossier building
    loop_identity: HashMap<Uuid, (String, String, String, String, String)>,
}

impl TriangleValidator {
    pub fn new(required_ticks: u8) -> Self {
        let min_profit_threshold = std::env::var("MIN_PROFIT_THRESHOLD")
            .unwrap_or_else(|_| "0.0025".to_string())
            .parse::<f64>()
            .expect("MIN_PROFIT_THRESHOLD must be a valid float");

        Self {
            persistence_map: HashMap::new(),
            min_profit_threshold,
            required_ticks,
            loop_identity: HashMap::new(),
        }
    }

    pub fn register_loop(&mut self, id: Uuid, coin_a: String, coin_b: String, p1: String, p2: String, p3: String) {
        self.loop_identity.insert(id, (coin_a, coin_b, p1, p2, p3));
    }

    fn liquidity_grade(avg_top5_usd: f64) -> &'static str {
        match avg_top5_usd {
            d if d > 5000.0 => "A",
            d if d > 2000.0 => "B",
            d if d > 800.0 => "C",
            d if d > 200.0 => "D",
            _ => "F",
        }
    }

    /// Main persistence check. Returns a fully-built Opportunity dossier when the
    /// loop passes `required_ticks` consecutive successful ticks.
    pub fn validate_persistent(
        &mut self,
        triangle_id: Uuid,
        book1: &OrderBookLevels,
        book2: &OrderBookLevels,
        book3: &OrderBookLevels,
        coin_a: String,
        coin_b: String,
        pair1: String,
        pair2: String,
        pair3: String,
    ) -> Option<Opportunity> {
        self.register_loop(triangle_id, coin_a.clone(), coin_b.clone(), pair1.clone(), pair2.clone(), pair3.clone());

        // Full real-world validation: normalized per-leg math, fees, slippage, capacity
        let report = validate_triangle_full(book1, book2, book3);

        let state = self.persistence_map
            .entry(triangle_id)
            .or_insert_with(PersistenceState::default);

        match &report {
            Some(r) => {
                if r.net_yield >= self.min_profit_threshold {
                    state.consecutive_ticks = state.consecutive_ticks.saturating_add(1);
                    // keep the best (highest net yield) report seen in this run
                    if r.net_yield > state.last_net_yield {
                        state.last_net_yield = r.net_yield;
                        state.best_capacity = r.capacity_usd;
                        state.best_fill_report = Some(r.clone());
                    }
                    state.last_seen = Instant::now();
                } else {
                    state.consecutive_ticks = 0;
                    state.best_fill_report = None;
                }
            }
            None => {
                state.consecutive_ticks = 0;
                state.best_fill_report = None;
            }
        }

        if state.consecutive_ticks >= self.required_ticks {
            // Emit ONLY if the current tick also passes (recomputed from freshest books)
            let report = report?;

            let fill_score = {
                let avg_vol = |levels: &[PriceLevel; 20]| -> f64 {
                    levels.iter().take(5).map(|l| l.price * l.volume).sum::<f64>() / 5.0
                };
                Self::liquidity_grade((avg_vol(&book1.asks) + avg_vol(&book2.bids) + avg_vol(&book3.bids)) / 3.0).to_string()
            };

            let path = format!(
                "USDT → {} → {} → USDT via {} (BUY) | {} (SELL A for B) | {} (SELL B)",
                coin_a, coin_b, pair1, pair2, pair3
            );

            let gap_age_ms = Instant::now()
                .duration_since(state.first_seen)
                .as_millis() as i64;

            let fr = state.best_fill_report.as_ref().unwrap_or(&report);

            Some(Opportunity {
                id: Uuid::new_v4(),
                triangle_id,
                path,
                net_yield_percent: fr.net_yield * 100.0,
                gross_gap_percent: fr.gross_yield * 100.0,
                fee_cost_percent: fr.fee_cost * 100.0,
                estimated_profit_usd: fr.estimated_profit_usd,
                capacity_usd: fr.capacity_usd,
                gap_age_ms,
                ticks_survived: state.consecutive_ticks as i32,
                fill_score,
                staleness_ms: 0,            // set by scanner at emission
                confidence: 0.5,            // set by scanner from loop stats
                maker_plan_yield_percent: fr.maker_yield * 100.0,
                slippage_percent: fr.slippage * 100.0,
                leg1_symbol: pair1,
                leg2_symbol: pair2,
                leg3_symbol: pair3,
                leg1_entry_price: fr.leg1_entry,
                leg1_fill_price: fr.leg1_fill,
                leg2_entry_price: fr.leg2_entry,
                leg2_fill_price: fr.leg2_fill,
                leg3_entry_price: fr.leg3_entry,
                leg3_fill_price: fr.leg3_fill,
                detected_at: chrono::Utc::now(),
                is_executed: false,
            })
        } else {
            None
        }
    }

    pub fn cleanup_old_entries(&mut self, max_age: Duration) {
        let now = Instant::now();
        self.persistence_map.retain(|_, state| {
            now.duration_since(state.last_seen) < max_age
        });
        self.loop_identity.retain(|id, _| self.persistence_map.contains_key(id));
    }

    pub fn get_stats(&self) -> (usize, usize) {
        let total = self.persistence_map.len();
        let active = self.persistence_map
            .values()
            .filter(|s| s.consecutive_ticks >= self.required_ticks)
            .count();
        (total, active)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::models::PriceLevel;

    fn rich_book() -> OrderBookLevels {
        let mut book = OrderBookLevels::default();
        for i in 0..20 {
            let p = 100.0 + i as f64 * 0.05;
            book.asks[i] = PriceLevel { price: p, volume: 10000.0 };
            book.bids[i] = PriceLevel { price: 100.0 - i as f64 * 0.05, volume: 10000.0 };
        }
        book
    }

    #[test]
    fn test_requires_ticks() {
        let mut validator = TriangleValidator::new(3);
        let book = rich_book();
        for _ in 0..2 {
            let r = validator.validate_persistent(
                Uuid::new_v4(),
                &book, &book, &book,
                "SOL".into(), "ETH".into(),
                "SOLUSDT".into(), "ETHSOL".into(), "ETHUSDT".into(),
            );
            assert!(r.is_none());
        }
    }
}
