// backend/src/engine/calculator.rs
// Real-World Math Engine
//
// Fixes from the audit (master plan B2-B5):
// - Correct MEXC fees: taker 0.05% per leg (configurable), maker legs 0%.
// - Per-leg fill normalization: the same USD target is converted through each
//   leg's price so the simulation eats the correct quote-currency amount per leg.
// - True capacity = min(fillable_volume_usd across all 3 legs), not a heuristic.
// - Full FillReport carries every field needed for the 15-field trade dossier.

use crate::data::models::{OrderBookLevels, PriceLevel};
use serde::{Deserialize, Serialize};

/// Live settings handle (set once by main at boot; `None` falls back to env).
static LIVE_SETTINGS: std::sync::OnceLock<
    std::sync::Arc<tokio::sync::RwLock<crate::data::models::SettingsSnapshot>>,
> = std::sync::OnceLock::new();

pub fn attach_live_settings(
    s: std::sync::Arc<tokio::sync::RwLock<crate::data::models::SettingsSnapshot>>,
) {
    let _ = LIVE_SETTINGS.set(s);
}

fn with_settings<T>(default: impl Fn() -> T, f: impl Fn(&crate::data::models::SettingsSnapshot) -> T) -> T {
    LIVE_SETTINGS
        .get()
        .map(|s| f(&futures::executor::block_on(s.read())))
        .unwrap_or_else(default)
}

/// Taker fee per leg (0.05% = MEXC spot reality as of 2026). Configurable via env.
fn taker_fee() -> f64 {
    with_settings(
        || {
            std::env::var("TAKER_FEE")
                .unwrap_or_else(|_| "0.0005".to_string())
                .parse()
                .unwrap_or(0.0005)
        },
        |s| s.taker_fee,
    )
}

/// Slippage buffer added to the profit floor (e.g. 0.05% safety margin).
fn slippage_buffer() -> f64 {
    with_settings(
        || {
            std::env::var("SLIPPAGE_BUFFER")
                .unwrap_or_else(|_| "0.0005".to_string())
                .parse()
                .unwrap_or(0.0005)
        },
        |s| s.slippage_buffer,
    )
}

fn target_volume_usd() -> f64 {
    with_settings(
        || {
            std::env::var("TARGET_VOLUME_USD")
                .unwrap_or_else(|_| "1000.0".to_string())
                .parse::<f64>()
                .unwrap_or(1000.0)
        },
        |s| s.target_volume_usd,
    )
}

fn min_net_yield() -> f64 {
    std::env::var("MIN_PROFIT_THRESHOLD")
        .unwrap_or_else(|_| "0.0025".to_string())
        .parse()
        .unwrap_or(0.0025)
}

#[derive(Debug, Clone, Copy)]
pub struct FillResult {
    pub fill_price: f64,
    pub filled_volume: f64,
    pub weighted_volume_usd: f64,
    pub is_low_liquidity: bool,
    pub slippage_pct: f64, // (fill_price - best_price) / best_price
}

/// Directional weighted fill.
///
/// * `consume_quote = true`  (BUY: we eat ASKS): we spend `target` units of the
///   quote currency and receive base. Book volume is in base units, so at each
///   level we can spend up to `price * volume` of quote and receive `take/price`
///   base.
///
/// * `consume_quote = false` (SELL: we hit BIDS): we spend `target` units of the
///   base currency and receive quote. Book volume is in base units, so at each
///   level we take up to `volume` base and receive `take * price` quote.
pub fn calculate_weighted_fill(levels: &[PriceLevel; 20], target: f64, consume_quote: bool) -> FillResult {
    let mut remaining = target;
    let mut filled_receive = 0.0_f64; // what we receive (base when buying, quote when selling)
    let mut spent = 0.0_f64;          // what we consume (quote when buying, base when selling)
    let best = levels
        .iter()
        .find(|l| l.price > 0.0 && l.volume > 0.0)
        .map(|l| l.price)
        .unwrap_or(0.0);

    for level in levels.iter() {
        if level.price <= 0.0 || level.volume <= 0.0 {
            break;
        }
        let (take_base, receive, spend) = if consume_quote {
            // BUY: spend quote, receive base = take/price
            let max_quote = level.price * level.volume;
            let take_quote = remaining.min(max_quote);
            if take_quote <= 0.0 {
                break;
            }
            (take_quote / level.price, take_quote / level.price, take_quote)
        } else {
            // SELL: spend base, receive quote = take*price
            let take_base = remaining.min(level.volume);
            if take_base <= 0.0 {
                break;
            }
            (take_base, take_base * level.price, take_base)
        };
        spent += spend;
        filled_receive += receive;
        remaining -= spend;
        if remaining <= 0.0 {
            break;
        }
    }

    let is_low_liquidity = filled_receive <= 0.0 || spent < target * 0.95;
    // Weighted fill price: effective price of what we consumed/received.
    // BUY:  price = spent / received (quote per base)
    // SELL: price = received / spent (quote per base)
    let fill_price = if spent > 0.0 && filled_receive > 0.0 {
        if consume_quote {
            spent / filled_receive
        } else {
            filled_receive / spent
        }
    } else {
        0.0
    };
    let slippage_pct = if best > 0.0 && fill_price > 0.0 {
        (fill_price - best).abs() / best
    } else {
        0.0
    };

    FillResult {
        fill_price,
        filled_volume: filled_receive,
        weighted_volume_usd: spent,
        is_low_liquidity,
        slippage_pct,
    }
}

/// Complete per-leg fill report for a USDT -> A -> B -> USDT loop.
///
/// Leg 1 (COINAUSDT): BUY A with USDT  -> eat `target` USD through ASKS
/// Leg 2 (COINBCOINA): SELL A for B   -> eat A amount through BIDS (we receive B)
/// Leg 3 (COINBUSDT):  SELL B for USDT -> eat B amount through BIDS (we receive USDT)
///
/// Returns None if any leg is stale, illiquid, or unprofitable after fees+buffer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FillReport {
    pub net_yield: f64,
    pub gross_yield: f64,
    pub fee_cost: f64,
    pub slippage: f64,
    pub estimated_profit_usd: f64,
    pub capacity_usd: f64,
    pub maker_yield: f64,
    pub leg1_entry: f64,
    pub leg1_fill: f64,
    pub leg2_entry: f64,
    pub leg2_fill: f64,
    pub leg3_entry: f64,
    pub leg3_fill: f64,
    pub liquidity_score: f64,
}

/// Leg 2 specialist: BUY B paying A on the ASKS of COINBCOINA (base = COIN_B,
/// quote = COIN_A, price = A per B, volume in B). Each unit of B costs `price`
/// units of A. Returns (B received, FillResult).
fn buy_b_with_a(asks: &[PriceLevel; 20], amount_a: f64) -> (f64, FillResult) {
    let mut remaining_a = amount_a;
    let mut b_received = 0.0_f64;
    let mut a_spent = 0.0_f64;
    let best = asks
        .iter()
        .find(|l| l.price > 0.0 && l.volume > 0.0)
        .map(|l| l.price)
        .unwrap_or(0.0);

    for level in asks.iter() {
        if level.price <= 0.0 || level.volume <= 0.0 {
            break;
        }
        // Each unit of B costs `price` units of A from us.
        let max_b = level.volume;
        let affordable_b = remaining_a / level.price;
        let take_b = max_b.min(affordable_b);
        if take_b <= 0.0 {
            break;
        }
        b_received += take_b;
        a_spent += take_b * level.price;
        remaining_a -= take_b * level.price;
        if remaining_a <= 0.0 {
            break;
        }
    }

    let is_low_liquidity = b_received <= 0.0 || a_spent < amount_a * 0.95;
    let fill_price = if a_spent > 0.0 && b_received > 0.0 {
        a_spent / b_received
    } else {
        0.0
    };
    let slippage_pct = if best > 0.0 && fill_price > 0.0 {
        (fill_price - best).abs() / best
    } else {
        0.0
    };

    (
        b_received,
        FillResult {
            fill_price,
            filled_volume: b_received,
            weighted_volume_usd: a_spent,
            is_low_liquidity,
            slippage_pct,
        },
    )
}

/// Size point on the yield curve: net yield achievable at a given trade size.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct SizePoint {
    pub size_usd: f64,
    pub net_yield: f64,
}

/// Find the OPTIMAL trade size for the loop: the largest size whose net yield
/// stays at or above the minimum threshold, plus a compact yield curve (7 size
/// points from $100 up to the max fillable size). This is the size the loop
/// SHOULD trade — bigger sizes eat deeper into the book and shrink the gap.
/// Returns None only if the books are unusable; the curve may show negative
/// yields at large sizes (that's the honest slippage reality).
pub fn find_optimal_size(
    book1: &OrderBookLevels,
    book2: &OrderBookLevels,
    book3: &OrderBookLevels,
    threshold: f64,
) -> Option<(f64, f64, Vec<SizePoint>)> {
    let fee = taker_fee();
    let buffer = slippage_buffer();

    // rough max fillable USD on leg 1 (first pass, best levels only)
    let leg1_base_cap = book1
        .asks
        .iter()
        .filter(|l| l.price > 0.0 && l.volume > 0.0)
        .take(10)
        .map(|l| l.price * l.volume)
        .sum::<f64>();
    let max_usd = leg1_base_cap.min(100_000.0).max(100.0);

    let eval = |usd: f64| -> Option<(f64, f64)> {
        let f1 = calculate_weighted_fill(&book1.asks, usd, true);
        if f1.is_low_liquidity || f1.fill_price <= 0.0 {
            return None;
        }
        let (amount_b, f2) = buy_b_with_a(&book2.asks, f1.filled_volume);
        if amount_b <= 0.0 || f2.is_low_liquidity {
            return None;
        }
        let f3 = calculate_weighted_fill(&book3.bids, amount_b, false);
        if f3.is_low_liquidity || f3.fill_price <= 0.0 {
            return None;
        }
        let gross_yield = f3.filled_volume / usd - 1.0;
        let net = gross_yield - fee * 3.0 - buffer;
        Some((net, usd))
    };

    // Yield curve at geometric size points up to max_usd.
    let mut curve = Vec::new();
    for exp in [2.0, 2.302, 2.605, 2.908, 3.21, 3.51, 3.815] {
        let size = 10_f64.powf(exp).min(max_usd);
        if let Some((net, _)) = eval(size) {
            curve.push(SizePoint { size_usd: size, net_yield: net });
        }
    }

    // Optimal = largest curve size with net >= threshold (and at least $100).
    let mut optimal: Option<(f64, f64)> = None;
    for p in curve.iter() {
        if p.size_usd >= 100.0 && p.net_yield >= threshold {
            optimal = Some((p.size_usd, p.net_yield));
        }
    }
    optimal.map(|(size, yield_at)| (size, yield_at, curve))
}

pub fn validate_triangle_full(
    book1: &OrderBookLevels,
    book2: &OrderBookLevels,
    book3: &OrderBookLevels,
) -> Option<FillReport> {
    let target = target_volume_usd();
    let fee = taker_fee();
    let buffer = slippage_buffer();

    // --- Leg 1: BUY A with USDT (asks of COINAUSDT) ---
    // Consume USDT (quote), receive A. consume_quote=true.
    let f1 = calculate_weighted_fill(&book1.asks, target, true);
    if f1.is_low_liquidity || f1.fill_price <= 0.0 {
        return None;
    }
    let amount_a = f1.filled_volume; // A received

    // --- Leg 2: BUY B paying A (asks of COINBCOINA) ---
    // MEXC convention: COINBCOINA base=COIN_B, quote=COIN_A; price = A per B.
    // An ask is an offer to SELL COIN_B for COIN_A. We hold A and want B, so we
    // consume the asks: each unit of B costs `price` units of A, i.e.
    // B_received = A_spent / price. Book volume is in COIN_B, so we can take up
    // to `volume` B per level.
    let (amount_b, f2) = buy_b_with_a(&book2.asks, amount_a);
    if amount_b <= 0.0 || f2.is_low_liquidity {
        return None;
    }

    // --- Leg 3: SELL B for USDT (bids of COINBUSDT) ---
    // Bids: counterparties buy B paying USDT; price = USDT per B, volume = B.
    // We sell our B: consume base B, receive USDT = take * price.
    // consume_quote=false.
    let f3 = calculate_weighted_fill(&book3.bids, amount_b, false);
    if f3.is_low_liquidity || f3.fill_price <= 0.0 {
        return None;
    }
    let usd_out = f3.filled_volume; // USDT received

    // --- Yield math ---
    let gross = usd_out / target;
    let gross_yield = gross - 1.0;

    // Taker fees on all three legs (MEXC charges taker on every executed trade).
    let fee_cost = fee * 3.0;
    let slippage = (f1.slippage_pct + f2.slippage_pct + f3.slippage_pct) / 3.0;

    let net_yield = gross_yield - fee_cost - buffer;

    // --- True capacity: min fillable depth across legs, in USD ---
    let cap1 = f1.weighted_volume_usd; // USD spent on leg 1
    let cap2 = f2.weighted_volume_usd * f1.fill_price; // B-base USD equiv via A-price
    let cap3 = f3.weighted_volume_usd; // USD received on leg 3
    let capacity = cap1.min(cap2).min(cap3);

    // --- Maker-plan alternative: same path with limit orders, 0% taker ---
    let maker_yield = gross_yield - buffer;

    if net_yield < min_net_yield_live() {
        return None;
    }

    Some(FillReport {
        net_yield,
        gross_yield,
        fee_cost,
        slippage,
        estimated_profit_usd: capacity * net_yield,
        capacity_usd: capacity,
        maker_yield,
        leg1_entry: book1.asks.iter().find(|l| l.price > 0.0).map(|l| l.price).unwrap_or(0.0),
        leg1_fill: f1.fill_price,
        leg2_entry: book2.asks.iter().find(|l| l.price > 0.0).map(|l| l.price).unwrap_or(0.0),
        leg2_fill: f2.fill_price,
        leg3_entry: book3.bids.iter().find(|l| l.price > 0.0).map(|l| l.price).unwrap_or(0.0),
        leg3_fill: f3.fill_price,
        liquidity_score: (f1.weighted_volume_usd + f2.weighted_volume_usd + f3.weighted_volume_usd) / 3.0,
    })
}

/// Legacy helpers retained for backward compatibility of tests.
pub fn calculate_weighted_fill_price(levels: &[PriceLevel; 20], target_volume: f64) -> FillResult {
    calculate_weighted_fill(levels, target_volume, false)
}

/// Live threshold (SettingsSnapshot if attached, else env default 0.25%).
fn min_net_yield_live() -> f64 {
    with_settings(|| min_net_yield(), |s| s.min_profit_threshold)
}

pub fn calculate_net_yield(p1: f64, p2: f64, p3: f64) -> f64 {
    if p1 <= 0.0 || p2 <= 0.0 || p3 <= 0.0 {
        return -1.0;
    }
    let fee = taker_fee();
    let gross = p1 * p2 * p3;
    let after_fees = gross * (1.0 - fee).powi(3);
    after_fees - 1.0
}

pub fn validate_triangle(
    book1: &OrderBookLevels,
    book2: &OrderBookLevels,
    book3: &OrderBookLevels,
) -> Option<(f64, f64)> {
    validate_triangle_full(book1, book2, book3)
        .map(|r| (r.net_yield, r.capacity_usd))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rich_book(best: f64) -> OrderBookLevels {
        let mut book = OrderBookLevels::default();
        for i in 0..20 {
            book.asks[i] = PriceLevel { price: best * (1.0 + i as f64 * 0.0002), volume: 10000.0 };
            book.bids[i] = PriceLevel { price: best * (1.0 - i as f64 * 0.0002), volume: 10000.0 };
        }
        book
    }

    #[test]
    fn test_weighted_fill_basic() {
        let mut levels = [PriceLevel::default(); 20];
        levels[0] = PriceLevel { price: 100.0, volume: 500.0 };
        levels[1] = PriceLevel { price: 101.0, volume: 600.0 };

        let result = calculate_weighted_fill(&levels, 1000.0, false);
        assert!(!result.is_low_liquidity);
        assert!(result.fill_price > 100.0);
    }

    #[test]
    fn test_no_profit_in_flat_market() {
        let book = rich_book(100.0);
        // All three books identical => no mispricing => should not profit
        let r = validate_triangle_full(&book, &book, &book);
        assert!(r.is_none() || r.unwrap().net_yield < 0.0025);
    }
}
