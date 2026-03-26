// backend/src/persistence/mod.rs
// Clean exports for the persistence layer

pub mod sqlite_pool;
pub mod trade_logger;

pub use sqlite_pool::SqlitePersistence;
pub use trade_logger::TradeLogger;
