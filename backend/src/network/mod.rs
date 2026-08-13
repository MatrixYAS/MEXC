// backend/src/network/mod.rs
// Public exports for the networking layer

pub mod rest_client;
pub mod wss_worker;
pub mod pool;
pub mod pb;
pub mod reseed;

pub use rest_client::RestClient;
pub use wss_worker::WssWorker;
pub use pool::WssPool;
pub use reseed::ReseedTask;

/// Combined Network Manager for easy access in main.rs
pub struct NetworkManager {
    pub rest_client: RestClient,
    pub ws_pool: std::sync::Arc<tokio::sync::Mutex<WssPool>>,
    pub reseed_task: std::sync::Arc<reseed::ReseedTask>,
}

impl NetworkManager {
    pub fn new(math_engine: std::sync::Arc<crate::engine::MathEngine>) -> Self {
        let rest_client = RestClient::new();
        let reseed_task = std::sync::Arc::new(reseed::ReseedTask::new(
            std::sync::Arc::clone(&math_engine),
            std::sync::Arc::new(rest_client.clone()),
        ));
        Self {
            rest_client,
            ws_pool: std::sync::Arc::new(tokio::sync::Mutex::new(WssPool::new(math_engine))),
            reseed_task,
        }
    }
}
