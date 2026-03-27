// backend/src/engine/validator.rs
// Updated: min_profit_threshold now read from environment variable (per guide 1.3)

use crate::data::models::{OrderBookLevels, Triangle};
use crate::engine::calculator::{validate_triangle, calculate_weighted_fill_price};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::time::{Duration, Instant};
use uuid::Uuid;

/// Persistence Filter (The "Anti-Ghost" Algo)
/// - 3-Tick Rule: Gap must stay profitable (>0.15%) for 3 consecutive updates
/// - Fill Score (A-F) based on order book density
/// - Gap Age tracking

#[derive(Debug, Clone)]
pub struct PersistenceState {
    pub last_seen: Instant,
    pub consecutive_ticks: u8,
    pub last_net_yield: f64,
    pub best_capacity: f64,
    pub fill_score: String,
}

impl Default for PersistenceState {
    fn default() -> Self {
        Self {
            last_seen: Instant::now(),
            consecutive_ticks: 0,
            last_net_yield: 0.0,
            best_capacity: 0.0,
            fill_score: "C".to_string(),
        }
    }
}

/// Main Validator for the Persistence Filter
pub struct TriangleValidator {
    persistence_map: HashMap<Uuid, PersistenceState>,
    min_profit_threshold: f64,   // Now dynamic from env var
}

impl TriangleValidator {
    pub fn new() -> Self {
        // Externalized configuration (guide 1.3)
        let min_profit_threshold = std::env::var("MIN_PROFIT_THRESHOLD")
            .unwrap_or_else(|_| "0.0015".to_string())   // default = 0.15%
            .parse::<f64>()
            .expect("MIN_PROFIT_THRESHOLD must be a valid float");

        Self {
            persistence_map: HashMap::new(),
            min_profit_threshold,
        }
    }

    /// Calculate Fill Score (A-F) based on order book density
    fn calculate_fill_score(book1: &OrderBookLevels, book2: &OrderBookLevels, book3: &OrderBookLevels) -> String {
        let avg_vol = |levels: &[crate::data::models::PriceLevel; 20]| -> f64 {
            levels.iter().take(5).map(|l| l.volume).sum::<f64>() / 5.0
        };

        let density = (avg_vol(&book1.asks) + avg_vol(&book2.asks) + avg_vol(&book3.bids)) / 3.0;

        match density {
            d if d > 5000.0 => "A",
            d if d > 2000.0 => "B",
            d if d > 800.0  => "C",
            d if d > 200.0  => "D",
            _ => "F",
        }.to_string()
    }

    /// Main validation entry point - called on every WebSocket tick for a triangle
    pub fn validate_persistent(
        &mut self,
        triangle_id: Uuid,
        book1: &OrderBookLevels,
        book2: &OrderBookLevels,
        book3: &OrderBookLevels,
    ) -> Option<Triangle> {
        let validation_result = validate_triangle(book1, book2, book3);

        let current_time = Instant::now();

        let state = self.persistence_map
            .entry(triangle_id)
            .or_insert_with(PersistenceState::default);

        match validation_result {
            Some((net_yield, capacity)) => {
                if net_yield >= self.min_profit_threshold {
                    state.consecutive_ticks = state.consecutive_ticks.saturating_add(1);
                    state.last_net_yield = net_yield.max(state.last_net_yield);
                    state.best_capacity = capacity.max(state.best_capacity);
                    state.last_seen = current_time;
                } else {
                    state.consecutive_ticks = 0;
                }
            }
            None => {
                state.consecutive_ticks = 0;
            }
        }

        if state.consecutive_ticks >= 3 {
            let fill_score = Self::calculate_fill_score(book1, book2, book3);

            let mut triangle = Triangle::new(
                "leg1".to_string(), // Real symbols are set by caller
                "leg2".to_string(),
                "leg3".to_string(),
                state.last_net_yield,
                state.best_capacity,
            );

            triangle.fill_score = fill_score;
            triangle.gap_age_ms = current_time
                .duration_since(state.last_seen)
                .as_millis() as i64;

            triangle.is_verified = true;

            Some(triangle)
        } else {
            None
        }
    }

    pub fn cleanup_old_entries(&mut self, max_age: Duration) {
        let now = Instant::now();
        self.persistence_map.retain(|_, state| {
            now.duration_since(state.last_seen) < max_age
        });
    }

    pub fn get_stats(&self) -> (usize, usize) {
        let total = self.persistence_map.len();
        let active = self.persistence_map.values()
            .filter(|s| s.consecutive_ticks >= 3)
            .count();
        (total, active)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::models::PriceLevel;

    #[test]
    fn test_3_tick_rule() {
        let mut validator = TriangleValidator::new();
        let dummy_book = OrderBookLevels::default();

        for _ in 0..2 {
            let result = validator.validate_persistent(Uuid::new_v4(), &dummy_book, &dummy_book, &dummy_book);
            assert!(result.is_none());
        }
    }
}
