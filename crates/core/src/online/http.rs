//! The polite HTTP client shared by every provider: a descriptive User-Agent, one request gate per provider
//! (a minimum spacing between requests) and a retry with back-off on 429 / 503.

use reqwest::{RequestBuilder, Response};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// What providers are told about us. MusicBrainz requires app name, version and a contact.
pub fn user_agent() -> String {
    format!(
        "NOCTURNE/{} (+https://nocturne.ifsvivek.in)",
        env!("CARGO_PKG_VERSION")
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Err {
    NotFound,
    /// 401 / 403: a wrong or revoked key.
    Auth,
    /// 406: a download quota is used up.
    Quota,
    /// Network error, server error, or still rate limited after the retries.
    Failed,
    Cancelled,
}

/// Minimum spacing between requests to one provider.
pub struct Gate {
    every: Duration,
    next: Mutex<Instant>,
}

impl Gate {
    pub fn new(every: Duration) -> Self {
        Self {
            every,
            next: Mutex::new(Instant::now()),
        }
    }

    /// Reserve the next slot; how long to wait from `now` before using it.
    pub fn reserve(&self, now: Instant) -> Duration {
        let mut n = self.next.lock().unwrap_or_else(|e| e.into_inner());
        let at = (*n).max(now);
        *n = at + self.every;
        at - now
    }

    pub async fn wait(&self) {
        let d = self.reserve(Instant::now());
        if !d.is_zero() {
            tokio::time::sleep(d).await;
        }
    }
}

/// How long to wait before retry number `attempt` (0-based): the server's `Retry-After` when it gave one,
/// else 2, 4, 8 s; never more than 30 s.
pub fn backoff(attempt: u32, retry_after: Option<u64>) -> Duration {
    Duration::from_secs(retry_after.unwrap_or(2u64 << attempt.min(4)).min(30))
}

pub struct Http {
    client: reqwest::Client,
    pub cancel: Arc<AtomicBool>,
}

impl Http {
    pub fn new() -> Self {
        Self::with_cancel(Arc::default())
    }

    /// `cancel` is checked before every request and while backing off.
    pub fn with_cancel(cancel: Arc<AtomicBool>) -> Self {
        Self {
            client: reqwest::Client::builder()
                .user_agent(user_agent())
                .timeout(Duration::from_secs(20))
                .build()
                .expect("http client"),
            cancel,
        }
    }

    pub fn get(&self, url: &str) -> RequestBuilder {
        self.client.get(url)
    }

    pub fn post(&self, url: &str) -> RequestBuilder {
        self.client.post(url)
    }

    pub async fn send(&self, gate: &Gate, rb: RequestBuilder) -> Result<Response, Err> {
        for attempt in 0..4u32 {
            if self.cancel.load(Ordering::Relaxed) {
                return Err(Err::Cancelled);
            }
            gate.wait().await;
            let Some(req) = rb.try_clone() else {
                return Err(Err::Failed);
            };
            let wait = match req.send().await {
                Ok(r) => match r.status().as_u16() {
                    200..=299 => return Ok(r),
                    404 => return Err(Err::NotFound),
                    401 | 403 => return Err(Err::Auth),
                    406 => return Err(Err::Quota),
                    429 | 503 => backoff(
                        attempt,
                        r.headers()
                            .get("retry-after")
                            .and_then(|v| v.to_str().ok())
                            .and_then(|v| v.trim().parse().ok()),
                    ),
                    _ => return Err(Err::Failed),
                },
                Err(_) => backoff(attempt, None),
            };
            if attempt < 3 {
                tokio::time::sleep(wait).await;
            }
        }
        Err(Err::Failed)
    }

    pub async fn json(&self, gate: &Gate, rb: RequestBuilder) -> Result<serde_json::Value, Err> {
        self.send(gate, rb)
            .await?
            .json()
            .await
            .map_err(|_| Err::Failed)
    }

    /// An image or file, at most `max` bytes.
    pub async fn bytes(&self, gate: &Gate, rb: RequestBuilder, max: usize) -> Result<Vec<u8>, Err> {
        let b = self
            .send(gate, rb)
            .await?
            .bytes()
            .await
            .map_err(|_| Err::Failed)?;
        if b.is_empty() || b.len() > max {
            return Err(Err::Failed);
        }
        Ok(b.to_vec())
    }
}

impl Default for Http {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gate_spaces_requests() {
        let g = Gate::new(Duration::from_secs(1));
        let t = Instant::now();
        assert_eq!(g.reserve(t), Duration::ZERO);
        assert_eq!(g.reserve(t), Duration::from_secs(1));
        assert_eq!(g.reserve(t), Duration::from_secs(2));
        // After a quiet spell the next one goes straight out.
        assert_eq!(g.reserve(t + Duration::from_secs(10)), Duration::ZERO);
    }

    #[test]
    fn backoff_grows_and_honours_retry_after() {
        assert_eq!(backoff(0, None), Duration::from_secs(2));
        assert_eq!(backoff(1, None), Duration::from_secs(4));
        assert_eq!(backoff(2, None), Duration::from_secs(8));
        assert_eq!(backoff(0, Some(5)), Duration::from_secs(5));
        assert_eq!(backoff(0, Some(9999)), Duration::from_secs(30));
    }

    #[test]
    fn user_agent_names_app_version_and_project() {
        let ua = user_agent();
        assert!(ua.starts_with("NOCTURNE/") && ua.contains("(+https://nocturne.ifsvivek.in)"));
    }
}
