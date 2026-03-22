//! Core OTEL configuration and management functionality
//!
//! This module provides core OpenTelemetry configuration and management functionality
//! without external extension system dependencies.

use crate::plugin::{Otel, OtelBuilder, OtelConfig};
use serde::{Deserialize, Serialize};

/// Enhanced OTEL configuration for standalone usage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OtelExtensionConfig {
    /// Base OTEL configuration
    pub base: OtelConfig,

    /// Enable auto-instrumentation
    pub auto_instrumentation: bool,

    /// Enable OTLP export
    pub enable_otlp_export: bool,

    /// Enable trace correlation
    pub enable_trace_correlation: bool,

    /// Enable W3C trace context propagation
    pub enable_w3c_propagation: bool,
}

impl Default for OtelExtensionConfig {
    fn default() -> Self {
        Self {
            base: OtelConfig::default(),
            auto_instrumentation: true,
            enable_otlp_export: true,
            enable_trace_correlation: true,
            enable_w3c_propagation: true,
        }
    }
}

impl OtelExtensionConfig {
    /// Create with custom base config
    pub fn with_base(base: OtelConfig) -> Self {
        Self {
            base,
            ..Default::default()
        }
    }

    /// Convert to base OTEL config
    pub fn to_base_config(&self) -> OtelConfig {
        self.base.clone()
    }

    /// Validate configuration
    pub fn validate(&self) -> Result<(), String> {
        if self.base.service_name.is_empty() {
            return Err("Service name cannot be empty".to_string());
        }
        if self.base.otlp_endpoint.is_empty() {
            return Err("OTLP endpoint cannot be empty".to_string());
        }
        Ok(())
    }

    /// Convert to JSON
    pub fn to_json(&self) -> Result<serde_json::Value, String> {
        serde_json::to_value(self).map_err(|e| format!("Failed to serialize OTEL config: {}", e))
    }

    /// Create from JSON
    pub fn from_json(value: &serde_json::Value) -> Result<Self, String> {
        serde_json::from_value(value.clone())
            .map_err(|e| format!("Invalid OTEL extension config: {}", e))
    }
}

/// OTEL Manager for standalone usage without extension system
///
/// This provides a simple interface to create and manage Otel instances
/// without requiring the full extension system infrastructure.
#[derive(Default)]
pub struct OtelManager {
    plugin: Option<Otel>,
    config: Option<OtelExtensionConfig>,
}

impl OtelManager {
    /// Create a new OTEL manager
    pub fn new() -> Self {
        Self {
            plugin: None,
            config: None,
        }
    }

    /// Initialize with extension instance
    pub fn with_plugin(plugin: Otel, config: OtelExtensionConfig) -> Self {
        Self {
            plugin: Some(plugin),
            config: Some(config),
        }
    }

    /// Create from configuration
    pub fn from_config(config: OtelExtensionConfig) -> Result<Self, String> {
        // Validate configuration
        config.validate()?;

        // Create extension instance synchronously (WASM-compatible)
        let base_config = config.to_base_config();
        match OtelBuilder::new()
            .with_endpoint(base_config.otlp_endpoint)
            .with_service_name(base_config.service_name)
            .with_batch_size(base_config.batch_size)
            .build_sync()
        {
            Ok(plugin) => {
                log::info!("🔗 OTEL Extension manager created successfully");
                Ok(Self::with_plugin(plugin, config))
            }
            Err(e) => {
                log::warn!(
                    "⚠️ Failed to create OTEL extension: {:?}, using uninitialized manager",
                    e
                );
                // Return uninitialized manager instead of failing
                let mut manager = Self::new();
                manager.config = Some(config);
                Ok(manager)
            }
        }
    }

    /// Create from environment variables
    pub fn from_env() -> Result<Self, String> {
        let base_config = OtelConfig::from_env();
        let config = OtelExtensionConfig::with_base(base_config);
        Self::from_config(config)
    }

    /// Initialize the manager
    pub fn initialize(&mut self, config: OtelExtensionConfig) -> Result<(), String> {
        // Validate configuration
        config.validate()?;

        // Store configuration
        self.config = Some(config.clone());

        // Try to create extension if not already created
        if self.plugin.is_none() {
            let base_config = config.to_base_config();
            match OtelBuilder::new()
                .with_endpoint(base_config.otlp_endpoint)
                .with_service_name(base_config.service_name)
                .with_batch_size(base_config.batch_size)
                .build_sync()
            {
                Ok(plugin) => {
                    self.plugin = Some(plugin);
                    log::info!("🔗 OTEL Extension manager initialized successfully");
                }
                Err(e) => {
                    log::warn!("⚠️ Failed to initialize OTEL extension: {:?}", e);
                    // Don't fail initialization - continue with uninitialized extension
                }
            }
        }

        Ok(())
    }

    /// Get the underlying extension instance
    pub fn plugin(&self) -> Option<&Otel> {
        self.plugin.as_ref()
    }

    /// Get the configuration
    pub fn config(&self) -> Option<&OtelExtensionConfig> {
        self.config.as_ref()
    }

    /// Check if the manager is initialized
    pub fn is_initialized(&self) -> bool {
        self.plugin.is_some() && self.config.is_some()
    }

    /// Get capabilities
    pub fn capabilities(&self) -> Vec<&'static str> {
        vec![
            "otlp_export",
            "auto_instrumentation",
            "trace_correlation",
            "w3c_propagation",
            "sampling",
            "resource_attributes",
            "metrics_collection",
            "structured_logging",
        ]
    }

    /// Get version
    pub fn version(&self) -> &'static str {
        crate::VERSION
    }

    /// Get name
    pub fn name(&self) -> &'static str {
        "otel"
    }

    /// Check health status
    pub fn health_status(&self) -> (bool, String) {
        if self.is_initialized() {
            (
                true,
                "OTEL Extension manager ready for enhanced observability".to_string(),
            )
        } else {
            (
                false,
                "OTEL Extension manager not fully initialized".to_string(),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_otel_extension_config_default() {
        let config = OtelExtensionConfig::default();
        assert!(config.auto_instrumentation);
        assert!(config.enable_otlp_export);
        assert!(config.enable_trace_correlation);
        assert!(config.enable_w3c_propagation);
    }

    #[test]
    fn test_otel_manager_creation() {
        let manager = OtelManager::new();
        assert!(!manager.is_initialized());
        assert_eq!(manager.name(), "otel");
        assert!(!manager.capabilities().is_empty());
    }

    #[test]
    fn test_otel_extension_config_validation() {
        let mut config = OtelExtensionConfig::default();
        assert!(config.validate().is_ok());

        // Test invalid config
        config.base.service_name = "".to_string();
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_otel_config_json_serialization() {
        let config = OtelExtensionConfig::default();
        let json = config.to_json();
        assert!(json.is_ok());

        let restored = OtelExtensionConfig::from_json(&json.unwrap());
        assert!(restored.is_ok());
    }
}
