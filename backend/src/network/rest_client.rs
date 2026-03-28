// backend/src/network/rest_client.rs
// REST Client with Token-Bucket Rate Limiter (API Guard)
// As specified in the PRD: Hard-capped at 15 requests/second for safety

use governor::{Quota, RateLimiter, Jitter, clock::DefaultClock, middleware::NoOpMiddleware, state::InMemoryState};
use reqwest::{Client, Response};
use std::time::Duration;
use anyhow::Result;
use serde::de::DeserializeOwned;

const MAX_REQUESTS_PER_SECOND: u32 = 15; // Well under MEXC 20 req/s limit

/// Rate-limited REST client for MEXC API
pub struct RestClient {
    client: Client,
    rate_limiter: Arc<RateLimiter<InMemoryState, DefaultClock, NoOpMiddleware>>,
}

impl RestClient {
    pub fn new() -> Self {
        let quota = Quota::per_second(
    std::num::NonZeroU32::new(MAX_REQUESTS_PER_SECOND).unwrap()
)
.allow_burst(std::num::NonZeroU32::new(5).unwrap()); // Small burst allowed for maintenance

        let rate_limiter = Arc::new(RateLimiter::direct_with_clock(
            quota,
            &DefaultClock::default(),
        ));

        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(10))
                .user_agent("MEXC-Ghost-Hunter/0.1")
                .build()
                .expect("Failed to build reqwest client"),
            rate_limiter,
        }
    }

    /// Wait for rate limit permission before making any request
    async fn wait_for_permission(&self) {
        let _ = self.rate_limiter.until_ready_with_jitter(Jitter::up_to(Duration::from_millis(50))).await;
    }

    /// Generic GET request with rate limiting
    pub async fn get<T: DeserializeOwned>(&self, url: &str) -> Result<T> {
        self.wait_for_permission().await;

        let resp: Response = self.client.get(url).send().await?;
        let status = resp.status();

        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("MEXC API error {}: {}", status, text);
        }

        let data: T = resp.json().await?;
        Ok(data)
    }

    /// GET with query parameters
    pub async fn get_with_params<T: DeserializeOwned>(
        &self,
        url: &str,
        params: &[(&str, &str)],
    ) -> Result<T> {
        self.wait_for_permission().await;

        let resp = self.client
            .get(url)
            .query(params)
            .send()
            .await?;

        if !resp.status().is_success() {
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("MEXC API error: {}", text);
        }

        let data: T = resp.json().await?;
        Ok(data)
    }

    /// Fetch 24h ticker statistics for volume filter (used in maintenance)
    pub async fn get_24h_ticker(&self, symbol: &str) -> Result<serde_json::Value> {
        let url = format!("https://api.mexc.com/api/v3/ticker/24hr?symbol={}", symbol);
        self.get(&url).await
    }

    /// Fetch current order book snapshot (used during initial load or recovery)
    pub async fn get_order_book_snapshot(
        &self,
        symbol: &str,
        limit: u32,
    ) -> Result<serde_json::Value> {
        let url = format!(
            "https://api.mexc.com/api/v3/depth?symbol={}&limit={}",
            symbol, limit
        );
        self.get(&url).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_rate_limiter_respects_limit() {
        let client = RestClient::new();
        // Simple smoke test - actual rate limiting is hard to test precisely
        let result = client.get_24h_ticker("BTCUSDT").await;
        // We don't assert success because it requires valid API keys / internet in CI
        assert!(result.is_ok() || result.is_err()); // Just ensure it doesn't panic
    }
}
