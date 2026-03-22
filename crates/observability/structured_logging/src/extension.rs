//! Enhanced configuration and management for structured logging
//!
//! This module provides core configuration and management functionality for structured logging
//! without external extension system dependencies.

use crate::error::{Result, StructuredLoggingError};
use observability_core::{
    LogEntry, ObservabilityConfig, ObservabilityManager,
};
use serde::{Deserialize, Serialize};

/// Enhanced configuration for structured logging with performance and convenience features
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnhancedObservabilityConfig {
    /// Base observability configuration
    pub base: ObservabilityConfig,

    /// Performance optimization settings
    #[cfg(feature = "performance-optimized")]
    pub performance: PerformanceConfig,

    /// Correlation enhancement settings
    #[cfg(feature = "correlation-enhanced")]
    pub correlation: CorrelationConfig,

    /// Convenience feature settings
    #[cfg(feature = "convenience")]
    pub convenience: ConvenienceConfig,

    /// Panic handling configuration
    pub panic_handler: crate::panic_handler::PanicHandlerConfig,
}

/// Performance optimization configuration
#[cfg(feature = "performance-optimized")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceConfig {
    /// Enable string interning for common log field values
    pub enable_string_interning: bool,

    /// Initial capacity for string interner
    pub string_interner_capacity: usize,

    /// Enable buffer pooling for log formatting
    pub enable_buffer_pooling: bool,

    /// Initial buffer pool size
    pub buffer_pool_size: usize,

    /// Individual buffer capacity in bytes
    pub buffer_capacity: usize,

    /// Enable zero-allocation fast paths for hot operations
    pub enable_fast_paths: bool,

    /// Enable WASM-specific memory optimizations
    pub enable_wasm_optimizations: bool,
}

#[cfg(feature = "performance-optimized")]
impl Default for PerformanceConfig {
    fn default() -> Self {
        Self {
            enable_string_interning: true,
            string_interner_capacity: 1024,
            enable_buffer_pooling: true,
            buffer_pool_size: 16,
            buffer_capacity: 2048,
            enable_fast_paths: true,
            enable_wasm_optimizations: cfg!(target_arch = "wasm32"),
        }
    }
}

/// Correlation enhancement configuration
#[cfg(feature = "correlation-enhanced")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorrelationConfig {
    /// Enable W3C baggage support
    pub enable_baggage: bool,

    /// Maximum baggage size in bytes
    pub max_baggage_size: usize,

    /// Enable scoped context management
    pub enable_scoped_context: bool,

    /// Maximum context nesting depth
    pub max_context_depth: usize,

    /// Enable automatic context propagation
    pub enable_auto_propagation: bool,
}

#[cfg(feature = "correlation-enhanced")]
impl Default for CorrelationConfig {
    fn default() -> Self {
        Self {
            enable_baggage: true,
            max_baggage_size: 8192,
            enable_scoped_context: true,
            max_context_depth: 16,
            enable_auto_propagation: true,
        }
    }
}

/// Convenience feature configuration
#[cfg(feature = "convenience")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConvenienceConfig {
    /// Enable LLM operation logging
    pub enable_llm_logging: bool,

    /// Enable template rendering logging
    pub enable_template_logging: bool,

    /// Enable A2A message logging
    pub enable_a2a_logging: bool,

    /// Enable convenience macros
    pub enable_convenience_macros: bool,

    /// Enable domain-specific field extraction
    pub enable_domain_fields: bool,
}

#[cfg(feature = "convenience")]
impl Default for ConvenienceConfig {
    fn default() -> Self {
        Self {
            enable_llm_logging: true,
            enable_template_logging: true,
            enable_a2a_logging: true,
            enable_convenience_macros: true,
            enable_domain_fields: true,
        }
    }
}

impl Default for EnhancedObservabilityConfig {
    fn default() -> Self {
        Self {
            base: ObservabilityConfig::default(),

            #[cfg(feature = "performance-optimized")]
            performance: PerformanceConfig::default(),

            #[cfg(feature = "correlation-enhanced")]
            correlation: CorrelationConfig::default(),

            #[cfg(feature = "convenience")]
            convenience: ConvenienceConfig::default(),

            panic_handler: crate::panic_handler::PanicHandlerConfig::default(),
        }
    }
}

impl EnhancedObservabilityConfig {
    /// Create enhanced config with specified base config
    pub fn with_base(base: ObservabilityConfig) -> Self {
        Self {
            base,
            ..Default::default()
        }
    }

    /// Set panic handler configuration  
    pub fn with_panic_handler(mut self, config: crate::panic_handler::PanicHandlerConfig) -> Self {
        self.panic_handler = config;
        self
    }

    /// Enable all performance features (feature-gated)
    #[cfg(feature = "performance-optimized")]
    pub fn with_all_performance_features(mut self) -> Self {
        self.performance.enable_string_interning = true;
        self.performance.enable_buffer_pooling = true;
        self.performance.enable_fast_paths = true;
        self.performance.enable_wasm_optimizations = true;
        self
    }

    /// Disable all performance features (feature-gated)
    #[cfg(feature = "performance-optimized")]
    pub fn with_no_performance_features(mut self) -> Self {
        self.performance = PerformanceConfig::default();
        self
    }

    /// Convert to base observability config
    pub fn to_base_config(&self) -> ObservabilityConfig {
        self.base.clone()
    }

    /// Validate configuration
    pub fn validate(&self) -> Result<()> {
        // Additional validation for enhanced features
        #[cfg(feature = "performance-optimized")]
        {
            if self.performance.buffer_pool_size == 0 {
                return Err(StructuredLoggingError::enhanced_config(
                    "Buffer pool size must be greater than 0",
                ));
            }

            if self.performance.buffer_capacity == 0 {
                return Err(StructuredLoggingError::enhanced_config(
                    "Buffer capacity must be greater than 0",
                ));
            }
        }

        Ok(())
    }

    /// Serialize configuration to JSON
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string_pretty(self).map_err(|e| {
            StructuredLoggingError::enhanced_config(format!("Failed to serialize config: {}", e))
        })
    }

    /// Deserialize configuration from JSON
    pub fn from_json(json: &str) -> Result<Self> {
        serde_json::from_str(json).map_err(|e| {
            StructuredLoggingError::enhanced_config(format!("Failed to deserialize config: {}", e))
        })
    }
}

/// Enhanced observability manager with performance and convenience features
pub struct PerformanceExtension {
    /// Base observability manager
    base: ObservabilityManager,

    /// Enhanced configuration
    config: EnhancedObservabilityConfig,

    /// Performance features
    #[cfg(feature = "performance-optimized")]
    performance: Option<crate::performance::PerformanceManager>,

    /// Correlation features
    #[cfg(feature = "correlation-enhanced")]
    correlation: Option<crate::correlation::CorrelationManager>,

    /// Convenience features
    #[cfg(feature = "convenience")]
    convenience: Option<crate::convenience::ConvenienceManager>,
}

impl PerformanceExtension {
    /// Create new enhanced extension
    pub fn new(config: EnhancedObservabilityConfig) -> Result<Self> {
        let base =
            ObservabilityManager::new(config.base.clone()).map_err(StructuredLoggingError::from)?;

        let mut extension = Self {
            base,
            config: config.clone(),

            #[cfg(feature = "performance-optimized")]
            performance: None,

            #[cfg(feature = "correlation-enhanced")]
            correlation: None,

            #[cfg(feature = "convenience")]
            convenience: None,
        };

        // Initialize optional feature modules
        extension.initialize_features()?;

        Ok(extension)
    }

    /// Initialize all configured features
    pub fn initialize(&mut self) -> Result<()> {
        self.base
            .initialize()
            .map_err(StructuredLoggingError::from)?;
        self.initialize_features()?;
        Ok(())
    }

    /// Initialize feature modules based on configuration
    fn initialize_features(&mut self) -> Result<()> {
        #[cfg(feature = "performance-optimized")]
        {
            if self.is_performance_enabled() {
                let manager =
                    crate::performance::PerformanceManager::new(&self.config.performance)?;
                self.performance = Some(manager);
            }
        }

        #[cfg(feature = "correlation-enhanced")]
        {
            if self.is_correlation_enabled() {
                let manager =
                    crate::correlation::CorrelationManager::new(&self.config.correlation)?;
                self.correlation = Some(manager);
            }
        }

        #[cfg(feature = "convenience")]
        {
            if self.is_convenience_enabled() {
                let manager = crate::convenience::ConvenienceManager::new()?;
                self.convenience = Some(manager);
            }
        }

        // Initialize panic handler if configured
        if self.config.panic_handler.enable_structured_logging {
            crate::panic_handler::install_panic_handler_with_config(
                self.config.panic_handler.clone(),
            )
            .map_err(|e| {
                StructuredLoggingError::enhanced_config(format!(
                    "Failed to install panic handler: {}",
                    e
                ))
            })?;
        }

        Ok(())
    }

    /// Check if performance features are enabled
    #[cfg(feature = "performance-optimized")]
    pub fn is_performance_enabled(&self) -> bool {
        self.config.performance.enable_string_interning
            || self.config.performance.enable_buffer_pooling
            || self.config.performance.enable_fast_paths
    }

    #[cfg(not(feature = "performance-optimized"))]
    pub fn is_performance_enabled(&self) -> bool {
        false
    }

    /// Check if correlation features are enabled
    #[cfg(feature = "correlation-enhanced")]
    pub fn is_correlation_enabled(&self) -> bool {
        self.config.correlation.enable_baggage || self.config.correlation.enable_scoped_context
    }

    #[cfg(not(feature = "correlation-enhanced"))]
    pub fn is_correlation_enabled(&self) -> bool {
        false
    }

    /// Check if convenience features are enabled
    #[cfg(feature = "convenience")]
    pub fn is_convenience_enabled(&self) -> bool {
        self.config.convenience.enable_llm_logging
            || self.config.convenience.enable_template_logging
            || self.config.convenience.enable_a2a_logging
    }

    #[cfg(not(feature = "convenience"))]
    pub fn is_convenience_enabled(&self) -> bool {
        false
    }

    /// Get base observability manager
    pub fn base(&self) -> &ObservabilityManager {
        &self.base
    }

    /// Get enhanced configuration
    pub fn config(&self) -> &EnhancedObservabilityConfig {
        &self.config
    }

    /// Process log entry through all enabled features
    pub fn process_log_entry(&self, entry: LogEntry) -> Result<LogEntry> {
        let mut processed_entry = entry;

        // Apply performance optimizations
        #[cfg(feature = "performance-optimized")]
        if let Some(ref perf) = self.performance {
            processed_entry = perf.process_entry(processed_entry)?;
        }

        // Apply correlation enhancements
        #[cfg(feature = "correlation-enhanced")]
        if let Some(ref corr) = self.correlation {
            processed_entry = corr.process_entry(processed_entry)?;
        }

        // Apply convenience enhancements
        #[cfg(feature = "convenience")]
        if let Some(ref conv) = self.convenience {
            processed_entry = conv.process_entry(processed_entry)?;
        }

        Ok(processed_entry)
    }

    /// Get performance statistics (if enabled)
    #[cfg(feature = "performance-optimized")]
    pub fn get_performance_stats(&self) -> Option<crate::performance::PerformanceStats> {
        self.performance.as_ref().map(|p| p.get_stats())
    }

    /// Reset performance statistics (if enabled)
    #[cfg(feature = "performance-optimized")]
    pub fn reset_performance_stats(&self) -> Result<()> {
        if let Some(ref perf) = self.performance {
            perf.reset_stats()?;
        }
        Ok(())
    }

    /// Get panic statistics if panic handler is enabled
    pub fn get_panic_stats(&self) -> Result<crate::panic_handler::PanicStats> {
        crate::panic_handler::get_panic_stats()
    }

    /// Reset panic statistics if panic handler is enabled
    pub fn reset_panic_stats(&self) -> Result<()> {
        crate::panic_handler::reset_panic_stats()
    }

    /// Get manager capabilities
    pub fn capabilities(&self) -> Vec<&'static str> {
        let mut caps = vec!["enhanced_logging"];

        if self.is_performance_enabled() {
            caps.push("performance_optimization");
        }
        if self.is_correlation_enabled() {
            caps.push("correlation_enhancement");
        }
        if self.is_convenience_enabled() {
            caps.push("convenience_apis");
        }
        if self.config.panic_handler.enable_structured_logging {
            caps.push("panic_handling");
        }

        caps
    }
}

/// Create performance extension from configuration
pub fn create_performance_extension(
    config: EnhancedObservabilityConfig,
) -> Result<PerformanceExtension> {
    PerformanceExtension::new(config)
}

/// Create performance extension from configuration file
pub fn create_performance_extension_from_config(config_path: &str) -> Result<PerformanceExtension> {
    let config_str = std::fs::read_to_string(config_path).map_err(|e| {
        StructuredLoggingError::enhanced_config(format!("Failed to read config file: {}", e))
    })?;
    let config = EnhancedObservabilityConfig::from_json(&config_str)?;
    PerformanceExtension::new(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_enhanced_config_default() {
        let config = EnhancedObservabilityConfig::default();

        // Verify base config exists
        assert_eq!(config.base.level, "info");
        assert_eq!(config.base.format, "compact");
        assert!(config.base.structured);
    }

    #[test]
    fn test_enhanced_config_with_base() {
        let base = ObservabilityConfig {
            level: "debug".to_string(),
            format: "json".to_string(),
            structured: true,
            context_enrichment: true,
            default_context: std::collections::HashMap::new(),
        };

        let config = EnhancedObservabilityConfig::with_base(base);
        assert_eq!(config.base.level, "debug");
        assert_eq!(config.base.format, "json");
    }

    #[cfg(feature = "performance-optimized")]
    #[test]
    fn test_performance_config_default() {
        let config = PerformanceConfig::default();
        assert!(config.enable_string_interning);
        assert!(config.enable_buffer_pooling);
        assert!(config.enable_fast_paths);
    }

    #[test]
    fn test_performance_extension_creation() {
        let config = EnhancedObservabilityConfig::default();
        let result = PerformanceExtension::new(config);
        assert!(result.is_ok());
    }

    #[test]
    fn test_config_serialization() {
        let config = EnhancedObservabilityConfig::default();
        let json = config.to_json().unwrap();
        let restored = EnhancedObservabilityConfig::from_json(&json).unwrap();

        assert_eq!(config.base.level, restored.base.level);
        assert_eq!(config.base.format, restored.base.format);
    }
}
