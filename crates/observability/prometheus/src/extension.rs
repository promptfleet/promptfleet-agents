//! Core Prometheus configuration and management functionality
//!
//! This module provides core Prometheus configuration and management functionality
//! without external extension system dependencies.

#[cfg(feature = "prometheus-federation")]
use crate::{Prometheus, PrometheusConfig};
use serde_json::json;

/// Configuration wrapper for standalone usage
#[derive(Debug, Clone)]
pub struct PrometheusExtensionConfig {
    /// Underlying plugin configuration
    pub base: PrometheusConfig,

    /// Enable push-gateway integration
    pub enable_pushgateway: bool,

    /// Enable hierarchical federation generation
    pub enable_federation: bool,
}

impl Default for PrometheusExtensionConfig {
    fn default() -> Self {
        Self {
            base: PrometheusConfig::default(),
            enable_pushgateway: true,
            enable_federation: true,
        }
    }
}

impl PrometheusExtensionConfig {
    pub fn with_base(base: PrometheusConfig) -> Self {
        Self {
            base,
            ..Default::default()
        }
    }

    /// Load configuration from environment variables
    pub fn from_env() -> Self {
        let pushgateway = std::env::var("PROMETHEUS_PUSHGATEWAY")
            .unwrap_or_else(|_| crate::DEFAULT_PUSHGATEWAY_ENDPOINT.to_string());
        let job =
            std::env::var("PROMETHEUS_JOB_NAME").unwrap_or_else(|_| "spinkube-agent".to_string());
        let instance =
            std::env::var("PROMETHEUS_INSTANCE").unwrap_or_else(|_| "localhost:9090".to_string());

        let base_cfg = PrometheusConfig {
            pushgateway_endpoint: Some(pushgateway),
            job_name: job,
            instance,
            push_interval: web_time::Duration::from_secs(crate::DEFAULT_PUSH_INTERVAL_SECS),
            cardinality_reduction: true,
            max_cardinality: 10000,
            hierarchical_federation: true,
            global_labels: std::collections::HashMap::new(),
        };

        Self {
            base: base_cfg,
            enable_pushgateway: true,
            enable_federation: true,
        }
    }

    /// Create from JSON configuration
    pub fn from_json(value: &serde_json::Value) -> Result<Self, String> {
        // Minimal custom parsing (accept job_name and pushgateway_url)
        let job_name = value
            .get("job_name")
            .and_then(|v| v.as_str())
            .unwrap_or("spinkube-agent")
            .to_string();
        let push_url = value
            .get("pushgateway_url")
            .and_then(|v| v.as_str())
            .unwrap_or(crate::DEFAULT_PUSHGATEWAY_ENDPOINT)
            .to_string();

        let mut base = PrometheusConfig::default();
        base.job_name = job_name;
        base.pushgateway_endpoint = Some(push_url);

        Ok(Self::with_base(base))
    }

    /// Validate configuration
    pub fn validate(&self) -> Result<(), String> {
        if self.base.job_name.is_empty() {
            return Err("Job name cannot be empty".to_string());
        }
        Ok(())
    }

    /// Convert to JSON
    pub fn to_json(&self) -> Result<serde_json::Value, String> {
        Ok(json!({
            "job_name": self.base.job_name,
            "pushgateway_url": self.base.pushgateway_endpoint,
            "push_interval": self.base.push_interval.as_secs(),
            "cardinality_reduction": self.base.cardinality_reduction,
            "max_cardinality": self.base.max_cardinality,
        }))
    }
}

/// Prometheus manager for standalone usage without extension system
#[derive(Default)]
pub struct PrometheusManager {
    plugin: Option<Prometheus>,
    config: Option<PrometheusExtensionConfig>,
}

impl PrometheusManager {
    /// Create a new Prometheus manager
    pub fn new() -> Self {
        Self {
            plugin: None,
            config: None,
        }
    }

    /// Create from configuration
    pub fn from_config(config: PrometheusExtensionConfig) -> Result<Self, String> {
        config.validate()?;
        let plugin = match Prometheus::new(config.base.clone()) {
            Ok(p) => Some(p),
            Err(e) => {
                log::warn!("Failed to create Prometheus plugin: {:?}", e);
                None
            }
        };
        Ok(Self {
            plugin,
            config: Some(config),
        })
    }

    /// Create from environment variables
    pub fn from_env() -> Result<Self, String> {
        let config = PrometheusExtensionConfig::from_env();
        Self::from_config(config)
    }

    /// Initialize the manager
    pub fn initialize(&mut self, config: PrometheusExtensionConfig) -> Result<(), String> {
        if self.plugin.is_none() {
            match Prometheus::new(config.base.clone()) {
                Ok(p) => self.plugin = Some(p),
                Err(e) => log::warn!("Failed to init Prometheus plugin: {:?}", e),
            }
        }
        self.config = Some(config);
        Ok(())
    }

    /// Check if the manager is initialized
    pub fn is_initialized(&self) -> bool {
        self.plugin.is_some()
    }

    /// Get the underlying plugin instance
    pub fn plugin(&self) -> Option<&Prometheus> {
        self.plugin.as_ref()
    }

    /// Get the configuration
    pub fn config(&self) -> Option<&PrometheusExtensionConfig> {
        self.config.as_ref()
    }

    /// Get capabilities
    pub fn capabilities(&self) -> Vec<&'static str> {
        vec![
            "metrics_collection",
            "pushgateway",
            "cardinality_reduction",
            "hierarchical_federation",
        ]
    }

    /// Get version
    pub fn version(&self) -> &'static str {
        crate::VERSION
    }

    /// Get name
    pub fn name(&self) -> &'static str {
        "prometheus"
    }

    /// Check health status
    pub fn health_status(&self) -> (bool, String) {
        if self.is_initialized() {
            (true, "Prometheus metrics manager ready".to_string())
        } else {
            (
                false,
                "Prometheus metrics manager not fully initialized".to_string(),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prometheus_extension_config_default() {
        let config = PrometheusExtensionConfig::default();
        assert!(config.enable_pushgateway);
        assert!(config.enable_federation);
    }

    #[test]
    fn test_prometheus_manager_creation() {
        let manager = PrometheusManager::new();
        assert!(!manager.is_initialized());
        assert_eq!(manager.name(), "prometheus");
        assert!(!manager.capabilities().is_empty());
    }

    #[test]
    fn test_prometheus_config_validation() {
        let mut config = PrometheusExtensionConfig::default();
        assert!(config.validate().is_ok());

        // Test invalid config
        config.base.job_name = "".to_string();
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_prometheus_config_json_serialization() {
        let config = PrometheusExtensionConfig::default();
        let json = config.to_json();
        assert!(json.is_ok());

        let restored = PrometheusExtensionConfig::from_json(&json.unwrap());
        assert!(restored.is_ok());
    }
}
