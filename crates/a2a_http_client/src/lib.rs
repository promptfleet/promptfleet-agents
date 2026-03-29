//! # A2A HTTP Client
//!
//! **A2A HTTP Client** with WASM default and native testing support
//!
//! ✅ **Target Adaptive**: WASM (Spin SDK) + Native (Reqwest) with identical APIs
//! ✅ **Complete Feature Parity**: All A2A protocol methods work identically
//! ✅ **Testing Excellence**: Native async runtime for comprehensive testing
//! ✅ **Clean Interface**: Same call() method across all targets
//! ✅ **Activation Aware**: Retry with backoff for KEDA scale-to-zero cold starts

// Activation-aware retry logic for cold-start tolerance (KEDA scale-to-zero)
pub mod activation;

// Target-specific implementations
#[cfg(target_arch = "wasm32")]
mod wasm_client;
#[cfg(target_arch = "wasm32")]
use wasm_client as implementation;

#[cfg(not(target_arch = "wasm32"))]
mod native_client;
#[cfg(not(target_arch = "wasm32"))]
use native_client as implementation;

// Unified public interface (identical for both targets)
pub use activation::{ActivationConfig, activation_delay, idempotency_key, retry_with_activation};
pub use implementation::{Client, ClientError, RpcError, check_connectivity};

// Re-export core types for convenience
pub use a2a_protocol_core::{A2A_PROTOCOL_VERSION, data::message::Message, data::task::Task};
pub use protocol_transport_core::{
    JSONRPC_VERSION, JsonRpcError, JsonRpcId, JsonRpcRequest, JsonRpcResponse, StreamingPolicy,
};

// ============================================================================
// Activation-aware call method on Client
// ============================================================================

impl Client {
    /// Call an A2A method with activation-aware retries for cold-start tolerance.
    ///
    /// When KEDA scales an agent to 0 replicas, the KEDA interceptor buffers
    /// the request and triggers scale-up. If the interceptor returns a retriable
    /// error (503, 502, 504, connection refused), this method retries with
    /// exponential backoff per the provided [`ActivationConfig`].
    ///
    /// Non-retriable errors (4xx, 500) are returned immediately without retry.
    ///
    /// An idempotency key is generated per attempt using `request_id` for safe
    /// retries on state-mutating methods like `message/send`. The key is
    /// available for future HTTP header injection (`X-Idempotency-Key`).
    pub async fn call_with_activation(
        &self,
        method: &str,
        params: serde_json::Value,
        config: &ActivationConfig,
        request_id: &str,
    ) -> Result<serde_json::Value, RpcError> {
        let start = web_time::Instant::now();

        for attempt in 0..=config.max_retries {
            if attempt > 0 && start.elapsed() > config.max_cold_start_timeout {
                return Err(RpcError::internal_error(
                    "activation cold-start deadline exceeded",
                ));
            }

            let _idem_key = idempotency_key(request_id, attempt);

            match self.call(method, params.clone()).await {
                Ok(val) => return Ok(val),
                Err(e) => {
                    if attempt < config.max_retries
                        && ActivationConfig::is_retriable_error(&e.to_string())
                    {
                        activation_delay(config.backoff_for_attempt(attempt)).await;
                        continue;
                    }
                    return Err(e);
                }
            }
        }
        Err(RpcError::internal_error("activation retry exhausted"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_client_creation() {
        let client = Client::external("http://localhost:8080/jsonrpc");
        assert_eq!(client.url(), "http://localhost:8080/jsonrpc");
    }

    #[test]
    fn test_with_header() {
        let client = Client::external("http://localhost:8080/jsonrpc")
            .with_header("Authorization".to_string(), "Bearer token".to_string());
        assert!(client.has_header("Authorization"));
    }

    #[test]
    fn test_a2a_version_header() {
        let client = Client::external("http://localhost:8080/jsonrpc");
        assert!(client.has_header("a2a-version"));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn test_native_client_basic_functionality() {
        let client = Client::external("http://localhost:8080/jsonrpc");
        assert_eq!(client.url(), "http://localhost:8080/jsonrpc");
        assert!(client.has_header("a2a-version"));
        assert!(client.has_header("content-type"));
    }
}
