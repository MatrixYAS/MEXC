// backend/src/engine/mod.rs
// Orchestrates the Math Engine as described in the PRD

pub mod calculator;
pub mod validator;

use crate::data::models::{OrderBookLevels, Triangle};
use crate::engine::calculator::validate_triangle;
use crate::engine::validator::TriangleValidator;
use std::sync::Arc;
use arc_swap::ArcSwap;
use tokio::sync::RwLock;
use uuid::Uuid;
use std::collections::HashMap;

/// Central Math Engine Orchestrator
pub struct MathEngine {
    /// Lock-free shared order books (300 coins)
    pub order_books: Arc<ArcSwap<HashMap<String, OrderBookLevels>>>,
    
    /// Persistence filter (Anti-Ghost 3-Tick Rule)
    validator: RwLock<TriangleValidator>,
}

impl MathEngine {
    pub fn new() -> Self {
        Self {
            order_books: Arc::new(ArcSwap::from_pointee(HashMap::new())),
            validator: RwLock::new(TriangleValidator::new()),
        }
    }

    /// Update a single symbol's order book (called by WebSocket workers)
    /// Uses arc-swap for zero-wait, lock-free updates
    pub fn update_order_book(&self, symbol: String, levels: OrderBookLevels) {
        let mut books = self.order_books.load().as_ref().clone();
        books.insert(symbol, levels);
        self.order_books.store(Arc::new(books));
    }

    /// Get current order book for a symbol (fast read - nanosecond access)
    pub fn get_order_book(&self, symbol: &str) -> Option<OrderBookLevels> {
        self.order_books.load().get(symbol).cloned()
    }

    /// Main hot-path function: Try to find and validate a persistent triangle
    /// This is called frequently from the main processing loop
    pub async fn process_triangle(
        &self,
        leg1: &str,   // e.g. "BTCUSDT"
        leg2: &str,   // e.g. "PEPEBTC"
        leg3: &str,   // e.g. "PEPEUSDT"
    ) -> Option<Triangle> {
        let books = self.order_books.load();

        let book1 = books.get(leg1)?;
        let book2 = books.get(leg2)?;
        let book3 = books.get(leg3)?;

        // Generate a deterministic ID for this triangle path
        let triangle_id = Uuid::new_v5(
            &Uuid::NAMESPACE_URL,
            format!("{}-{}-{}", leg1, leg2, leg3).as_bytes()
        );

        // Delegate to persistence validator (3-Tick Rule + Fill Score)
        let mut validator = self.validator.write().await;
        validator.validate_persistent(
            triangle_id,
            book1,
            book2,
            book3,
        )
    }

    /// Clean up old persistence entries periodically
    pub async fn cleanup(&self) {
        let mut validator = self.validator.write().await;
        validator.cleanup_old_entries(tokio::time::Duration::from_secs(60));
    }

    /// Get engine statistics for telemetry
    pub async fn get_stats(&self) -> (usize, usize) {
        let validator = self.validator.read().await;
        validator.get_stats()
    }
}

// Re-exports for clean usage
pub use calculator::{calculate_weighted_fill_price, calculate_net_yield, validate_triangle};
pub use validator::TriangleValidator;
