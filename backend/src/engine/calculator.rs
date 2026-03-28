// backend/src/engine/calculator.rs
// Updated: TARGET_VOLUME_USD is now read from environment variable (per guide 1.3)

use crate::data::models::{OrderBookLevels, PriceLevel};

/// Core Math Engine - Real-World Weighted Average Fill + Triple-Tax Net Profit
///
/// Follows the PRD exactly:
/// - Simulates eating $1,000 USD (or bottleneck) through the order book
/// - Uses stack-allocated [PriceLevel; 20]
/// - Triple 0.1% taker fee → (0.999)^3
/// - Only gaps >= 0.15% net are considered

const TAKER_FEE: f64 = 0.001; // 0.1% as per PRD
const MIN_NET_YIELD: f64 = 0.0015; // 0.15% - this is also externalized in validator.rs

/// Reads TARGET_VOLUME_USD from environment variable with fallback (guide requirement)
fn get_target_volume() -> f64 {
    std::env::var("TARGET_VOLUME_USD")
        .unwrap_or_else(|_| "1000.0".to_string())
        .parse::<f64>()
        .expect("TARGET_VOLUME_USD must be a valid float (e.g. 1000.0)")
}

#[derive(Debug, Clone, Copy)]
pub struct FillResult {
    pub fill_price: f64,
    pub filled_volume: f64,
    pub is_low_liquidity: bool,
}

/// Calculates the weighted average fill price by "eating" into the order book
pub fn calculate_weighted_fill_price(levels: &[PriceLevel; 20], target_volume: f64) -> FillResult {
    let mut remaining = target_volume;
    let mut total_cost = 0.0;
    let mut filled = 0.0;

    for level in levels.iter() {
        if level.price <= 0.0 || level.volume <= 0.0 {
            break;
        }

        let take_volume = remaining.min(level.volume);
        if take_volume <= 0.0 {
            break;
        }

        total_cost += take_volume * level.price;
        filled += take_volume;
        remaining -= take_volume;

        if remaining <= 0.0 {
            break;
        }
    }

    let is_low_liquidity = filled < target_volume * 0.95;

    let fill_price = if filled > 0.0 {
        total_cost / filled
    } else {
        0.0
    };

    FillResult {
        fill_price,
        filled_volume: filled,
        is_low_liquidity,
    }
}

/// Triple-Tax Net Profit Formula (exactly as in PRD)
pub fn calculate_net_yield(
    p1: f64,
    p2: f64,
    p3: f64,
) -> f64 {
    if p1 <= 0.0 || p2 <= 0.0 || p3 <= 0.0 {
        return -1.0;
    }

    let gross = p1 * p2 * p3;
    let after_fees = gross * (1.0 - TAKER_FEE).powi(3);
    let net = after_fees - 1.0;

    net
}

/// Main function: Validates a triangle with real-world slippage simulation
pub fn validate_triangle(
    book1: &OrderBookLevels,
    book2: &OrderBookLevels,
    book3: &OrderBookLevels,
) -> Option<(f64, f64)> {
    let target_volume = get_target_volume();   // ← Now dynamic from ENV

    // Check staleness first (2000ms as per PRD)
    let max_stale_ms = 2000;
    if book1.is_stale(max_stale_ms) || 
       book2.is_stale(max_stale_ms) || 
       book3.is_stale(max_stale_ms) {
        return None;
    }

    let fill1 = calculate_weighted_fill_price(&book1.asks, target_volume);
    let fill2 = calculate_weighted_fill_price(&book2.asks, target_volume);
    let fill3 = calculate_weighted_fill_price(&book3.bids, target_volume);

    if fill1.is_low_liquidity || fill2.is_low_liquidity || fill3.is_low_liquidity {
        return None;
    }

    let net_yield = calculate_net_yield(fill1.fill_price, fill2.fill_price, fill3.fill_price);

    if net_yield < MIN_NET_YIELD {
        return None;
    }

    let effective_capacity = target_volume * (1.0 + net_yield * 2.0);

    Some((net_yield, effective_capacity))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_weighted_fill_basic() {
        let mut levels = [PriceLevel::default(); 20];
        levels[0] = PriceLevel { price: 100.0, volume: 500.0 };
        levels[1] = PriceLevel { price: 101.0, volume: 600.0 };

        let result = calculate_weighted_fill_price(&levels, 1000.0);
        assert!(!result.is_low_liquidity);
        assert!(result.fill_price > 100.0);
    }

    #[test]
    fn test_net_yield_formula() {
        let net = calculate_net_yield(1.0, 1.0, 1.0015);
        assert!(net > 0.001);
    }
}
