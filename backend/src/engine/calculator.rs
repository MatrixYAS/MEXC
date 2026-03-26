use crate::data::models::{OrderBookLevels, PriceLevel};
use chrono::Utc;

/// Core Math Engine - Real-World Weighted Average Fill + Triple-Tax Net Profit
///
/// Follows the PRD exactly:
/// - Simulates eating $1000 USD (or bottleneck) through the order book
/// - Uses stack-allocated [PriceLevel; 20]
/// - Triple 0.1% taker fee → (0.999)^3
/// - Only gaps >= 0.15% net are considered

const TARGET_VOLUME_USD: f64 = 1000.0;
const TAKER_FEE: f64 = 0.001; // 0.1% as per PRD
const MIN_NET_YIELD: f64 = 0.0015; // 0.15%

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

    let is_low_liquidity = filled < target_volume * 0.95; // Allow small tolerance

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
///
/// Net% = ( (Input * P_fill1 * P_fill2 * P_fill3 / Input) * (0.999)^3 ) - 1
pub fn calculate_net_yield(
    p1: f64, // fill price leg 1 (buy)
    p2: f64, // fill price leg 2
    p3: f64, // fill price leg 3 (sell back)
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
    book1: &OrderBookLevels, // e.g. USDT->BTC (ask side for buy)
    book2: &OrderBookLevels, // e.g. BTC->PEPE
    book3: &OrderBookLevels, // e.g. PEPE->USDT (bid side for sell)
) -> Option<(f64, f64)> {  // (net_yield, effective_capacity)
    
    // Check staleness first (2000ms as per PRD)
    let max_stale_ms = 2000;
    if book1.is_stale(max_stale_ms) || 
       book2.is_stale(max_stale_ms) || 
       book3.is_stale(max_stale_ms) {
        return None;
    }

    // Simulate $1000 fill on all three legs
    let fill1 = calculate_weighted_fill_price(&book1.asks, TARGET_VOLUME_USD); // Buy leg → use asks
    let fill2 = calculate_weighted_fill_price(&book2.asks, TARGET_VOLUME_USD);
    let fill3 = calculate_weighted_fill_price(&book3.bids, TARGET_VOLUME_USD); // Sell leg → use bids

    if fill1.is_low_liquidity || fill2.is_low_liquidity || fill3.is_low_liquidity {
        return None;
    }

    let net_yield = calculate_net_yield(fill1.fill_price, fill2.fill_price, fill3.fill_price);

    // Minimalist Filter: only send gaps >= 0.15%
    if net_yield < MIN_NET_YIELD {
        return None;
    }

    // Rough effective capacity estimation (can be refined later)
    let effective_capacity = TARGET_VOLUME_USD * (1.0 + net_yield * 2.0); // simple heuristic

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
        assert!(net > 0.001); // Should be positive after fees
    }
}
