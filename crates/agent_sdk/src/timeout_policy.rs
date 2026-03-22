//! Unified timeout policy for the agent platform.
//!
//! `TimeoutPolicy` is the single root configuration that propagates through
//! every sub-component (A2A client, LLM client, coordination runtime,
//! activation retries). It replaces scattered hardcoded defaults with an
//! explicit three-clock streaming model.
//!
//! Unconditional: compiled for both WASM and native targets.

use protocol_transport_core::StreamingPolicy;
use serde::{Deserialize, Serialize};

/// Top-level timeout configuration.
///
/// Propagated from CRD → `AgentRuntimeConfig` → `AgentBuilder` → sub-components.
/// OSS users get `TimeoutPolicy::streaming_default()` automatically via
/// `AgentBuilder::from_config_path()`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeoutPolicy {
    /// TCP + TLS handshake timeout (ms). Default: 10_000.
    pub connect_ms: u64,
    /// Time until first data chunk arrives (ms). Default: 45_000.
    pub first_byte_ms: u64,
    /// Max silence between consecutive chunks (ms). Resets on each chunk. Default: 90_000.
    pub idle_ms: u64,
    /// Total activation budget for cold-start retries (ms). Default: 60_000.
    pub activation_budget_ms: u64,
    /// Wall-clock ceiling for LLM execution loops (ms).
    /// `Some(120_000)` = 2 min default. `None` = unlimited.
    pub wall_clock_ms: Option<u64>,
}

impl Default for TimeoutPolicy {
    fn default() -> Self {
        Self::streaming_default()
    }
}

impl TimeoutPolicy {
    /// Streaming-optimized defaults. Used for agents that serve SSE streams.
    pub fn streaming_default() -> Self {
        Self {
            connect_ms: 10_000,
            first_byte_ms: 45_000,
            idle_ms: 90_000,
            activation_budget_ms: 60_000,
            wall_clock_ms: Some(120_000),
        }
    }

    /// RPC (non-streaming) defaults. Total wall-clock 30s, no idle.
    pub fn rpc_default() -> Self {
        Self {
            connect_ms: 10_000,
            first_byte_ms: 30_000,
            idle_ms: 30_000,
            activation_budget_ms: 60_000,
            wall_clock_ms: Some(30_000),
        }
    }

    /// Derive a `StreamingPolicy` for transport-level clients.
    pub fn to_streaming_policy(&self) -> StreamingPolicy {
        StreamingPolicy {
            connect_ms: self.connect_ms,
            first_byte_ms: self.first_byte_ms,
            idle_ms: self.idle_ms,
        }
    }

    /// Derive an `ActivationConfig` from this policy.
    #[cfg(feature = "a2a-client")]
    pub fn to_activation_config(&self) -> a2a_http_client::ActivationConfig {
        use std::time::Duration;
        a2a_http_client::ActivationConfig {
            max_cold_start_timeout: Duration::from_millis(self.activation_budget_ms),
            ..a2a_http_client::ActivationConfig::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streaming_default_values() {
        let p = TimeoutPolicy::streaming_default();
        assert_eq!(p.connect_ms, 10_000);
        assert_eq!(p.first_byte_ms, 45_000);
        assert_eq!(p.idle_ms, 90_000);
        assert_eq!(p.activation_budget_ms, 60_000);
        assert_eq!(p.wall_clock_ms, Some(120_000));
    }

    #[test]
    fn rpc_default_values() {
        let p = TimeoutPolicy::rpc_default();
        assert_eq!(p.wall_clock_ms, Some(30_000));
    }

    #[test]
    fn to_streaming_policy_derives_correctly() {
        let p = TimeoutPolicy::streaming_default();
        let sp = p.to_streaming_policy();
        assert_eq!(sp.connect_ms, 10_000);
        assert_eq!(sp.first_byte_ms, 45_000);
        assert_eq!(sp.idle_ms, 90_000);
    }

    #[test]
    fn default_is_streaming() {
        let d = TimeoutPolicy::default();
        let s = TimeoutPolicy::streaming_default();
        assert_eq!(d.connect_ms, s.connect_ms);
        assert_eq!(d.idle_ms, s.idle_ms);
        assert_eq!(d.wall_clock_ms, s.wall_clock_ms);
    }
}
