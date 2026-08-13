// backend/src/engine/mod.rs
// Lock-free order book store (DashMap) + Scanner coordination.
//
// The Scanner (triangle enumeration + persistence filter) is owned by the single
// scan task as Mutex<Scanner> in main.rs — it is NOT accessed through MathEngine.
// MathEngine remains the lock-free book store shared with the WebSocket workers.

use crate::data::models::OrderBookLevels;
use dashmap::DashMap;
use std::sync::Arc;

pub mod calculator;
pub mod scanner;
pub mod validator;

/// Central Math Engine with DashMap for lock-free, high-performance access
pub struct MathEngine {
    pub order_books: Arc<DashMap<String, OrderBookLevels>>,
}

impl MathEngine {
    pub fn new() -> Self {
        Self {
            order_books: Arc::new(DashMap::new()),
        }
    }

    /// Update a single symbol's order book - zero-copy insert
    pub fn update_order_book(&self, symbol: String, levels: OrderBookLevels) {
        self.order_books.insert(symbol, levels);
    }

    /// Get current order book for a symbol
    pub fn get_order_book(&self, symbol: &str) -> Option<OrderBookLevels> {
        self.order_books.get(symbol).map(|r| *r)
    }

    /// Number of live books (for telemetry)
    pub fn book_count(&self) -> usize {
        self.order_books.len()
    }

    /// Remove books for symbols no longer subscribed
    pub fn drop_symbols(&self, keep: &[String]) {
        self.order_books.retain(|k, _| keep.contains(k));
    }
}

// Re-exports
pub use crate::engine::calculator::{calculate_weighted_fill_price, calculate_net_yield, validate_triangle, validate_triangle_full, FillReport};
pub use crate::engine::scanner::Scanner;
pub use crate::engine::validator::TriangleValidator;
