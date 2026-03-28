// backend/src/engine/mod.rs
// Updated with DashMap for high-performance order book updates (no full clone on every tick)

use crate::data::models::{OrderBookLevels, Triangle};
use crate::engine::calculator::validate_triangle;
use crate::engine::validator::TriangleValidator;
use dashmap::DashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

// Central Math Engine with DashMap for lock-free, high-performance access
pub struct MathEngine {
    pub order_books: Arc<DashMap<String, OrderBookLevels>>,  // Changed from HashMap + arc-swap
    validator: RwLock<TriangleValidator>,
}

impl MathEngine {
    pub fn new() -> Self {
        Self {
            order_books: Arc::new(DashMap::new()),
            validator: RwLock::new(TriangleValidator::new()),
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

    /// Main hot-path function: Try to find and validate a persistent triangle
    pub async fn process_triangle(
        &self,
        leg1: &str,
        leg2: &str,
        leg3: &str,
    ) -> Option<Triangle> {
        let book1 = self.order_books.get(leg1)?.clone();
        let book2 = self.order_books.get(leg2)?.clone();
        let book3 = self.order_books.get(leg3)?.clone();

        let triangle_id = Uuid::new_v5(
            &Uuid::NAMESPACE_URL,
            format!("{}-{}-{}", leg1, leg2, leg3).as_bytes()
        );

        let mut validator = self.validator.write().await;
        validator.validate_persistent(
            triangle_id,
            &book1,
            &book2,
            &book3,
        )
    }

    pub async fn cleanup(&self) {
        let mut validator = self.validator.write().await;
        validator.cleanup_old_entries(tokio::time::Duration::from_secs(60));
    }

    pub async fn get_stats(&self) -> (usize, usize) {
        let validator = self.validator.read().await;
        validator.get_stats()
    }
}

// Re-exports
pub use crate::engine::calculator::{calculate_weighted_fill_price, calculate_net_yield, validate_triangle};
pub use crate::engine::validator::TriangleValidator;
