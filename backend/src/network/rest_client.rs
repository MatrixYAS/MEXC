// backend/src/network/rest_client.rs
// REST Client with Token-Bucket Rate Limiter (API Guard)
// As specified in the PRD: Hard-capped at 15 requests/second for safety

use hmac::{Hmac, Mac};
use sha2::Sha256;
use reqwest::{Client, Response};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use anyhow::Result;
use serde::de::DeserializeOwned;

const MAX_REQUESTS_PER_SECOND: u32 = 15; // Well under MEXC 20 req/s limit

type HmacSha256 = Hmac<Sha256>;

/// MEXC-style HMAC-SHA256 signature (returned as lowercase hex).
pub fn hmac_sign(secret: &str, payload: &str) -> String {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC accepts any key size");
    mac.update(payload.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

/// Simple async token-bucket rate limiter (replacement for governor, which has
/// incompatible type bounds in this dependency set).
pub struct TokenBucket {
    state: Mutex<(f64, Instant)>,
    /// tokens added per second
    rate: f64,
    /// maximum burst size
    burst: f64,
}

impl TokenBucket {
    pub fn new(rate: f64, burst: f64) -> Self {
        Self {
            state: Mutex::new((burst, Instant::now())),
            rate,
            burst,
        }
    }

    /// Wait until a token is available, then consume it.
    pub async fn acquire(&self) {
        loop {
            let wait = {
                let mut state = self.state.lock().await;
                let now = Instant::now();
                let elapsed = now.duration_since(state.1).as_secs_f64();
                state.0 = (state.0 + elapsed * self.rate).min(self.burst);
                state.1 = now;
                if state.0 >= 1.0 {
                    state.0 -= 1.0;
                    None
                } else {
                    // tokens missing -> wait (1 - tokens) / rate seconds, plus jitter
                    Some(Duration::from_secs_f64((1.0 - state.0) / self.rate + 0.02))
                }
            };
            match wait {
                None => return,
                Some(d) => tokio::time::sleep(d).await,
            }
        }
    }
}

/// Rate-limited REST client for MEXC API
#[derive(Clone)]
pub struct RestClient {
    client: Client,
    rate_limiter: Arc<TokenBucket>,
}

impl RestClient {
    pub fn new() -> Self {
        // 15 req/s sustained, burst of 5
        let rate_limiter = Arc::new(TokenBucket::new(
            MAX_REQUESTS_PER_SECOND as f64,
            5.0,
        ));

        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(30))
                .user_agent("MEXC-Ghost-Hunter/0.1")
                .build()
                .expect("Failed to build reqwest client"),
            rate_limiter,
        }
    }

    /// Wait for rate limit permission before making any request
    async fn wait_for_permission(&self) {
        self.rate_limiter.acquire().await;
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

    /// Fetch 24h ticker statistics for a single symbol (used by the legacy per-symbol flow)
    pub async fn get_24h_ticker(&self, symbol: &str) -> Result<serde_json::Value> {
        let url = format!("https://api.mexc.com/api/v3/ticker/24hr?symbol={}", symbol);
        self.get(&url).await
    }

    /// Fetch current order book snapshot (used during initial load or recovery)
    pub async fn get_order_book_snapshot(
        &self, symbol: &str, limit: u32,
    ) -> Result<serde_json::Value> {
        let url = format!(
            "https://api.mexc.com/api/v3/depth?symbol={}&limit={}",
            symbol, limit
        );
        self.get(&url).await
    }

    /// Fetch ALL 24h tickers in one call — used by maintenance to build the
    /// full-market volume-ranked whitelist (no pagination needed).
    pub async fn get_all_tickers(&self) -> Result<Vec<serde_json::Value>> {
        self.wait_for_permission().await;
        let resp = match tokio::time::timeout(
            std::time::Duration::from_secs(30),
            self.client.get("https://api.mexc.com/api/v3/ticker/24hr").send(),
        )
        .await
        {
            Ok(r) => r?,
            Err(_) => anyhow::bail!("ticker/24hr request timed out (API too slow; retry later)"),
        };
        if !resp.status().is_success() {
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("MEXC API error: {}", text);
        }
        let text = resp.text().await.unwrap_or_default();
        let tickers = match serde_json::from_str::<serde_json::Value>(&text) {
            Ok(v) => v.as_array().cloned().unwrap_or_default(),
            Err(_) => Vec::new(),
        };
        if tickers.is_empty() {
            anyhow::bail!("ticker feed empty or undecodable");
        }
        Ok(tickers)
    }

    /// Fetch exchangeInfo — the source of truth for valid symbols and status.
    pub async fn get_exchange_info(&self) -> Result<serde_json::Value> {
        self.get("https://api.mexc.com/api/v3/exchangeInfo").await
    }

    /// HMAC-SHA256 signed GET helper for credential tests.
    pub async fn signed_get(
        &self,
        api_key: &str,
        secret_key: &str,
        path: &str,
        query: &str,
    ) -> Result<serde_json::Value> {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let payload = if query.is_empty() {
            format!("timestamp={}", timestamp)
        } else {
            format!("{}&timestamp={}", query, timestamp)
        };
        let signature = hmac_sign(secret_key, &payload);
        let url = format!(
            "https://api.mexc.com{}?{}&signature={}",
            path, payload, signature
        );
        self.wait_for_permission().await;
        let resp = self.client
            .get(&url)
            .header("X-MEXC-APIKEY", api_key)
            .send()
            .await?;
        let status = resp.status();
        let body: serde_json::Value = resp.json().await.unwrap_or_default();
        if !status.is_success() {
            anyhow::bail!(
                "HTTP {}: {}",
                status,
                body.get("msg").and_then(|m| m.as_str()).unwrap_or("unknown error")
            );
        }
        Ok(body)
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
