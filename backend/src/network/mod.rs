// backend/src/network/mod.rs
// Public exports for the networking layer

pub mod rest_client;
pub mod wss_worker;
pub mod pool;

pub use rest_client::RestClient;
pub use wss_worker::WssWorker;
pub use pool::WssPool;

/// Combined Network Manager for easy access in main.rs
pub struct NetworkManager {
    pub rest_client: RestClient,
    pub ws_pool: WssPool,
}

impl NetworkManager {
    pub fn new(math_engine: std::sync::Arc<crate::engine::MathEngine>) -> Self {
        Self {
            rest_client: RestClient::new(),
            ws_pool: WssPool::new(math_engine),
        }
    }
}
