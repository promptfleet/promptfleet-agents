//! Activation-aware retry logic for cold-start tolerance.
//!
//! When KEDA scales an agent to 0 replicas, the first request may encounter:
//! - 503 Service Unavailable (interceptor buffering, no backend yet)
//! - Connection refused (pod not started)
//! - 504 Gateway Timeout (cold start exceeded upstream timeout)
//!
//! This module provides configurable retry with exponential backoff and jitter
//! to handle these transient failures transparently.
//!
//! ## Usage
//!
//! Use [`Client::call_with_activation`] for activation-aware A2A calls, or
//! [`retry_with_activation`] for generic retry over any async callable.

use std::time::Duration;

/// Configuration for activation-aware retries when calling potentially cold agents.
///
/// These values drive the SDK-side retry budget. The KEDA HTTP Add-on
/// `HTTPScaledObject` does NOT have a CRD-level queueDepth -- this is the
/// platform's enforcement layer.
#[derive(Debug, Clone)]
pub struct ActivationConfig {
    /// Max time to wait for a cold agent to respond. Default: 5s.
    pub max_cold_start_timeout: Duration,
    /// Initial retry delay. Default: 100ms.
    pub initial_backoff: Duration,
    /// Max retry delay cap. Default: 2s.
    pub max_backoff: Duration,
    /// Max retries before giving up. Default: 3.
    pub max_retries: u32,
    /// Add jitter to backoff. Default: true.
    pub jitter: bool,
}

impl Default for ActivationConfig {
    fn default() -> Self {
        Self {
            max_cold_start_timeout: Duration::from_mins(1),
            initial_backoff: Duration::from_millis(100),
            max_backoff: Duration::from_secs(2),
            max_retries: 3,
            jitter: true,
        }
    }
}

impl ActivationConfig {
    /// Compute the backoff duration for a given attempt (0-indexed).
    ///
    /// Uses exponential backoff: `initial * 2^attempt`, capped at `max_backoff`.
    /// When jitter is enabled, the result is multiplied by a factor in [0.5, 1.0].
    pub fn backoff_for_attempt(&self, attempt: u32) -> Duration {
        let base = self
            .initial_backoff
            .saturating_mul(2u32.saturating_pow(attempt));
        let capped = std::cmp::min(base, self.max_backoff);

        if self.jitter {
            // Simple deterministic jitter: reduce by up to 50%
            // In real deployment this would use a PRNG, but for WASM compatibility
            // we use a simple approach based on attempt number.
            let jitter_factor = 0.5 + (0.5 * ((attempt as f64 * 0.7).sin().abs()));
            Duration::from_secs_f64(capped.as_secs_f64() * jitter_factor)
        } else {
            capped
        }
    }

    /// Check if an HTTP status code indicates a retriable cold-start scenario.
    pub fn is_retriable_status(status: u16) -> bool {
        matches!(status, 503 | 502 | 504)
    }

    /// Check if an error message indicates a retriable connection failure.
    pub fn is_retriable_error(error_msg: &str) -> bool {
        let lower = error_msg.to_lowercase();
        lower.contains("connection refused")
            || lower.contains("econnrefused")
            || lower.contains("connect error")
            || lower.contains("502")
            || lower.contains("503")
            || lower.contains("504")
    }
}

/// Generate an idempotency key for safe retries on state-mutating requests.
///
/// Format: `{request_id}:{attempt}` -- the server should deduplicate by this key.
pub fn idempotency_key(request_id: &str, attempt: u32) -> String {
    format!("{}:{}", request_id, attempt)
}

// ============================================================================
// Platform-adaptive sleep
// ============================================================================

/// Async delay for activation retries. Uses `tokio::time::sleep` on native
/// and `std::thread::sleep` (WASI monotonic clock) on WASM.
#[cfg(not(target_arch = "wasm32"))]
pub async fn activation_delay(duration: Duration) {
    tokio::time::sleep(duration).await;
}

/// WASM: Spin SDK runs each request in an isolated instance (single-threaded).
/// `std::thread::sleep` is supported via WASI `wasi:clocks/monotonic-clock`.
#[cfg(target_arch = "wasm32")]
pub async fn activation_delay(duration: Duration) {
    if !duration.is_zero() {
        std::thread::sleep(duration);
    }
}

// ============================================================================
// Generic retry loop
// ============================================================================

/// Execute an async operation with activation-aware retries.
///
/// `call_fn` receives the 0-indexed attempt number and returns `Result<T, E>`.
/// Errors whose `Display` output matches [`ActivationConfig::is_retriable_error`]
/// are retried with exponential backoff; all other errors are returned immediately.
///
/// Returns the first `Ok`, or the last `Err` when retries are exhausted.
pub async fn retry_with_activation<T, E, F, Fut>(
    config: &ActivationConfig,
    mut call_fn: F,
) -> Result<T, E>
where
    E: std::fmt::Display,
    F: FnMut(u32) -> Fut,
    Fut: std::future::Future<Output = Result<T, E>>,
{
    let mut last_err: Option<E> = None;
    let start = web_time::Instant::now();

    for attempt in 0..=config.max_retries {
        if attempt > 0 && start.elapsed() > config.max_cold_start_timeout {
            break;
        }

        match call_fn(attempt).await {
            Ok(val) => return Ok(val),
            Err(e) => {
                let retriable = ActivationConfig::is_retriable_error(&e.to_string());
                if retriable && attempt < config.max_retries {
                    activation_delay(config.backoff_for_attempt(attempt)).await;
                    last_err = Some(e);
                    continue;
                }
                return Err(e);
            }
        }
    }

    Err(last_err.expect("at least one attempt must have been made"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_values() {
        let config = ActivationConfig::default();
        assert_eq!(config.max_cold_start_timeout, Duration::from_mins(1));
        assert_eq!(config.initial_backoff, Duration::from_millis(100));
        assert_eq!(config.max_backoff, Duration::from_secs(2));
        assert_eq!(config.max_retries, 3);
        assert!(config.jitter);
    }

    #[test]
    fn backoff_exponential_no_jitter() {
        let config = ActivationConfig {
            initial_backoff: Duration::from_millis(100),
            max_backoff: Duration::from_secs(10),
            jitter: false,
            ..Default::default()
        };
        assert_eq!(config.backoff_for_attempt(0), Duration::from_millis(100));
        assert_eq!(config.backoff_for_attempt(1), Duration::from_millis(200));
        assert_eq!(config.backoff_for_attempt(2), Duration::from_millis(400));
        assert_eq!(config.backoff_for_attempt(3), Duration::from_millis(800));
    }

    #[test]
    fn backoff_capped_at_max() {
        let config = ActivationConfig {
            initial_backoff: Duration::from_millis(500),
            max_backoff: Duration::from_secs(1),
            jitter: false,
            ..Default::default()
        };
        assert_eq!(config.backoff_for_attempt(0), Duration::from_millis(500));
        assert_eq!(config.backoff_for_attempt(1), Duration::from_secs(1));
        assert_eq!(config.backoff_for_attempt(2), Duration::from_secs(1));
        assert_eq!(config.backoff_for_attempt(5), Duration::from_secs(1));
    }

    #[test]
    fn backoff_with_jitter_is_reduced() {
        let config = ActivationConfig {
            initial_backoff: Duration::from_millis(100),
            max_backoff: Duration::from_secs(10),
            jitter: true,
            ..Default::default()
        };
        let base = Duration::from_millis(100);
        let jittered = config.backoff_for_attempt(0);
        // Jitter should produce a value between 50% and 100% of base
        assert!(jittered >= base / 2);
        assert!(jittered <= base);
    }

    #[test]
    fn retriable_status_codes() {
        assert!(ActivationConfig::is_retriable_status(503));
        assert!(ActivationConfig::is_retriable_status(502));
        assert!(ActivationConfig::is_retriable_status(504));
        assert!(!ActivationConfig::is_retriable_status(200));
        assert!(!ActivationConfig::is_retriable_status(404));
        assert!(!ActivationConfig::is_retriable_status(500));
        assert!(!ActivationConfig::is_retriable_status(400));
    }

    #[test]
    fn retriable_error_messages() {
        assert!(ActivationConfig::is_retriable_error("Connection refused"));
        assert!(ActivationConfig::is_retriable_error("ECONNREFUSED"));
        assert!(ActivationConfig::is_retriable_error("HTTP error: 503"));
        assert!(ActivationConfig::is_retriable_error(
            "HTTP error: 504 - timeout"
        ));
        assert!(!ActivationConfig::is_retriable_error("Not found"));
        assert!(!ActivationConfig::is_retriable_error(
            "Internal server error"
        ));
    }

    #[test]
    fn idempotency_key_format() {
        let key = idempotency_key("req-123", 0);
        assert_eq!(key, "req-123:0");
        let key2 = idempotency_key("req-123", 2);
        assert_eq!(key2, "req-123:2");
    }

    // ========================================================================
    // retry_with_activation tests
    // ========================================================================

    #[cfg(not(target_arch = "wasm32"))]
    mod retry_tests {
        use super::*;
        use std::sync::Arc;
        use std::sync::atomic::{AtomicU32, Ordering};

        fn fast_config(max_retries: u32) -> ActivationConfig {
            ActivationConfig {
                max_retries,
                jitter: false,
                initial_backoff: Duration::from_millis(1),
                max_backoff: Duration::from_millis(5),
                ..Default::default()
            }
        }

        #[tokio::test]
        async fn retry_succeeds_on_first_attempt() {
            let config = fast_config(3);
            let result: Result<i32, String> =
                retry_with_activation(&config, |_attempt| async { Ok(42) }).await;
            assert_eq!(result.unwrap(), 42);
        }

        #[tokio::test]
        async fn retry_succeeds_after_retriable_errors() {
            let config = fast_config(3);
            let count = Arc::new(AtomicU32::new(0));

            let result: Result<i32, String> = retry_with_activation(&config, |_attempt| {
                let count = count.clone();
                async move {
                    let n = count.fetch_add(1, Ordering::SeqCst);
                    if n < 2 {
                        Err("HTTP error: 503 - Service Unavailable".to_string())
                    } else {
                        Ok(42)
                    }
                }
            })
            .await;

            assert_eq!(result.unwrap(), 42);
            assert_eq!(count.load(Ordering::SeqCst), 3); // attempts 0, 1, 2
        }

        #[tokio::test]
        async fn retry_stops_on_non_retriable_error() {
            let config = fast_config(3);
            let count = Arc::new(AtomicU32::new(0));

            let result: Result<i32, String> = retry_with_activation(&config, |_attempt| {
                let count = count.clone();
                async move {
                    count.fetch_add(1, Ordering::SeqCst);
                    Err("Internal server error".to_string())
                }
            })
            .await;

            assert!(result.is_err());
            assert_eq!(count.load(Ordering::SeqCst), 1); // only one attempt
        }

        #[tokio::test]
        async fn retry_exhausts_all_attempts() {
            let config = fast_config(2);
            let count = Arc::new(AtomicU32::new(0));

            let result: Result<i32, String> = retry_with_activation(&config, |_attempt| {
                let count = count.clone();
                async move {
                    count.fetch_add(1, Ordering::SeqCst);
                    Err("HTTP error: 503 - cold start".to_string())
                }
            })
            .await;

            assert!(result.is_err());
            assert!(result.unwrap_err().contains("503"));
            assert_eq!(count.load(Ordering::SeqCst), 3); // attempts 0, 1, 2
        }

        #[tokio::test]
        async fn retry_passes_attempt_number() {
            let config = fast_config(2);
            let seen_attempts = Arc::new(std::sync::Mutex::new(Vec::new()));

            let result: Result<i32, String> = retry_with_activation(&config, |attempt| {
                let seen = seen_attempts.clone();
                async move {
                    seen.lock().unwrap().push(attempt);
                    if attempt < 2 {
                        Err("Connection refused".to_string())
                    } else {
                        Ok(99)
                    }
                }
            })
            .await;

            assert_eq!(result.unwrap(), 99);
            assert_eq!(*seen_attempts.lock().unwrap(), vec![0, 1, 2]);
        }

        #[tokio::test]
        async fn retry_zero_retries_makes_single_attempt() {
            let config = fast_config(0);
            let count = Arc::new(AtomicU32::new(0));

            let result: Result<i32, String> = retry_with_activation(&config, |_attempt| {
                let count = count.clone();
                async move {
                    count.fetch_add(1, Ordering::SeqCst);
                    Err("HTTP error: 503".to_string())
                }
            })
            .await;

            assert!(result.is_err());
            assert_eq!(count.load(Ordering::SeqCst), 1);
        }
    }
}
