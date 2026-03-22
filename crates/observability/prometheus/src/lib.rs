//! # Prometheus Extension
//!
//! Production-ready Prometheus metrics extension with best practices for structured logging.
//! Features hierarchical federation, cardinality reduction, and push gateway integration.
//!
//! ## Features
//!
//! - `prometheus-federation`: Core Prometheus metrics functionality
//! - `pushgateway-client`: Push gateway client for batch metrics
//! - `cardinality-reduction`: Advanced label optimization
//! - `recording-rules`: Performance recording rules support
//! - `structured-logging`: JSON structured logging support

#[cfg(feature = "cardinality-reduction")]
pub mod cardinality_reduction;
#[cfg(feature = "prometheus-federation")]
pub mod extension;
#[cfg(feature = "prometheus-federation")]
pub mod hierarchical_federation;
#[cfg(feature = "prometheus-federation")]
pub mod plugin;
#[cfg(feature = "pushgateway-client")]
pub mod pushgateway_client;
#[cfg(feature = "recording-rules")]
pub mod recording_rules;

// Re-export main plugin
#[cfg(feature = "prometheus-federation")]
pub use plugin::Prometheus;

// Re-export configuration
#[cfg(feature = "prometheus-federation")]
pub use plugin::PrometheusConfig;

// Re-export cardinality reduction
#[cfg(feature = "cardinality-reduction")]
pub use cardinality_reduction::{CardinalityReducer, CardinalityStats, MetricStats};

// Re-export federation support
#[cfg(feature = "prometheus-federation")]
pub use hierarchical_federation::{FederationConfig, HierarchicalFederation};

// Re-export recording rules
#[cfg(feature = "recording-rules")]
pub use recording_rules::{RecordingRule, RecordingRuleGroup, RecordingRulesManager};

// Re-export push gateway client
#[cfg(feature = "pushgateway-client")]
pub use pushgateway_client::{BatchPushClient, PushGatewayClient};

// 🧩 CORE EXPORTS for standalone usage (extension system removed)
#[cfg(feature = "prometheus-federation")]
pub use extension::{PrometheusExtensionConfig, PrometheusManager};

/// Version information
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Default push gateway endpoint
pub const DEFAULT_PUSHGATEWAY_ENDPOINT: &str = "http://localhost:9091";

/// Default metrics port
pub const DEFAULT_METRICS_PORT: u16 = 9090;

/// Default push interval in seconds
pub const DEFAULT_PUSH_INTERVAL_SECS: u64 = 30;

/// Create a standalone Prometheus manager
#[cfg(feature = "prometheus-federation")]
pub fn create_prometheus_manager() -> Result<PrometheusManager, String> {
    PrometheusManager::from_env()
}

/// Create a Prometheus manager from configuration
#[cfg(feature = "prometheus-federation")]
pub fn create_prometheus_manager_from_config(
    config: PrometheusExtensionConfig,
) -> Result<PrometheusManager, String> {
    PrometheusManager::from_config(config)
}
