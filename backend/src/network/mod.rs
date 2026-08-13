// backend/src/network/mod.rs
// Public exports for the networking layer

pub mod rest_client;
pub mod wss_worker;
pub mod pool;
pub mod pb;

pub use rest_client::RestClient;
pub use wss_worker::WssWorker;
pub use pool::WssPool;

/// Combined Network Manager for easy access in main.rs
pub struct NetworkManager {
    pub rest_client: RestClient,
    pub ws_pool: std::sync::Arc<tokio::sync::Mutex<WssPool>>,
}

impl NetworkManager {
    pub fn new(math_engine: std::sync::Arc<crate::engine::MathEngine>) -> Self {
        Self {
            rest_client: RestClient::new(),
            ws_pool: std::sync::Arc::new(tokio::sync::Mutex::new(WssPool::new(math_engine))),
        }
    }
}
