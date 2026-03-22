//! Core observability components for structured logging
//!
//! This module provides core observability functionality including configuration,
//! global logger singleton, and structured logging integration without external dependencies.

use crate::adapters::{LogDirectives, LoggingSetupBuilder, StandardLogAdapter, WasmStdoutAdapter};
use crate::domain::{
    EnhancedContextEnricher, LogKvExtractor, ProcessorChain, StructuredFieldsProcessor,
    TimestampProcessor,
};
use crate::error::{ObservabilityError, ObservabilityResult};
use crate::ports::{StandardLoggingPort, TransportPort};
use crate::traits::LogLevel;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Singleton global logger that initializes once and is shared across all extension instances
pub struct GlobalLoggerSingleton {
    adapter: Arc<StandardLogAdapter>,
    config: ObservabilityConfig,
}

impl GlobalLoggerSingleton {
    /// Get or create the singleton instance
    ///
    /// This is thread-safe and will initialize exactly once
    pub fn get_or_init(
        config: ObservabilityConfig,
    ) -> ObservabilityResult<&'static GlobalLoggerSingleton> {
        static INSTANCE: std::sync::OnceLock<GlobalLoggerSingleton> = std::sync::OnceLock::new();
        static INIT_ERROR: std::sync::OnceLock<String> = std::sync::OnceLock::new();

        // Check if we have an existing instance
        if let Some(instance) = INSTANCE.get() {
            return Ok(instance);
        }

        // Check if we had a previous initialization error
        if let Some(error) = INIT_ERROR.get() {
            return Err(ObservabilityError::logging(format!(
                "Singleton initialization failed: {}",
                error
            )));
        }

        // Try to initialize
        match Self::create_instance(config) {
            Ok(instance) => {
                // This will succeed only for the first caller
                match INSTANCE.set(instance) {
                    Ok(()) => Ok(INSTANCE.get().unwrap()), // Safe: we just set it
                    Err(_) => {
                        // Another thread won the race - use their instance
                        Ok(INSTANCE.get().unwrap()) // Safe: the other thread set it
                    }
                }
            }
            Err(e) => {
                // Store the error for future calls
                let _ = INIT_ERROR.set(e.to_string());
                Err(e)
            }
        }
    }

    /// Create the singleton instance (called exactly once)
    fn create_instance(config: ObservabilityConfig) -> ObservabilityResult<GlobalLoggerSingleton> {
        let directives = config.parse_directives();
        let transport = config.create_transport();
        let processor_chain = if config.structured {
            let mut enricher = EnhancedContextEnricher::new();
            if config.context_enrichment && !config.default_context.is_empty() {
                for (k, v) in &config.default_context {
                    enricher = enricher.with_field(k.clone(), v.clone());
                }
            }

            ProcessorChain::new()
                .add_processor(Box::new(TimestampProcessor))
                .add_processor(Box::new(LogKvExtractor::new()))
                .add_processor(Box::new(enricher))
                .add_processor(Box::new(StructuredFieldsProcessor))
        } else {
            ProcessorChain::new()
        };

        let adapter = LoggingSetupBuilder::new()
            .with_processor_chain(processor_chain)
            .with_transport(transport)
            .with_directives(directives)
            .build()?;

        let adapter_arc = Arc::new(adapter);

        // Set up standard Rust logging (happens exactly once)
        let adapter_ptr = Arc::as_ptr(&adapter_arc) as *const StandardLogAdapter;
        unsafe {
            // Handle the case where the logger is already initialized (e.g., by tests)
            if let Err(e) = log::set_logger(&*adapter_ptr) {
                // If the logger is already initialized, that's fine - it means another
                // instance or test already set it up
                log::warn!("Global Rust logger already initialized: {}", e);
            }
        }

        // Initialize the adapter
        adapter_arc.initialize()?;

        log::info!(
            "🔍 Global logger singleton initialized: Standard Rust logging is now structured"
        );

        Ok(GlobalLoggerSingleton {
            adapter: adapter_arc,
            config,
        })
    }

    /// Get the logger adapter
    pub fn adapter(&self) -> &Arc<StandardLogAdapter> {
        &self.adapter
    }

    /// Get the configuration used
    pub fn config(&self) -> &ObservabilityConfig {
        &self.config
    }
}

/// Configuration for observability
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObservabilityConfig {
    /// Minimum log level to process
    pub level: String, // "error", "warn", "info", "debug", "trace"

    /// Output format: "json", "compact", "plain"
    pub format: String,

    /// Enable structured logging features
    pub structured: bool,

    /// Enable context enrichment
    pub context_enrichment: bool,

    /// Additional context fields to always include
    #[serde(default)]
    pub default_context: std::collections::HashMap<String, serde_json::Value>,
}

impl Default for ObservabilityConfig {
    fn default() -> Self {
        Self {
            level: "info".to_string(),
            format: "compact".to_string(),
            structured: true,
            context_enrichment: true,
            default_context: std::collections::HashMap::new(),
        }
    }
}

impl ObservabilityConfig {
    /// Parse log level from string (global level only, ignores per-crate directives).
    pub fn parse_level(&self) -> ObservabilityResult<LogLevel> {
        let global = self.parse_directives().global_level();
        Ok(global)
    }

    /// Parse `RUST_LOG`-style directives from the level string.
    ///
    /// Accepts both simple levels (`"info"`) and per-crate directives
    /// (`"info,agent_sdk=debug,a2a_protocol_core=trace"`).
    pub fn parse_directives(&self) -> LogDirectives {
        LogDirectives::parse(&self.level)
    }

    /// Create transport based on format configuration
    pub fn create_transport(&self) -> Arc<dyn TransportPort> {
        match self.format.as_str() {
            "json" => Arc::new(WasmStdoutAdapter::with_json_formatter()),
            "plain" => Arc::new(WasmStdoutAdapter::with_plain_text_formatter()),
            _ => Arc::new(WasmStdoutAdapter::with_compact_formatter()),
        }
    }

    /// Validate configuration
    pub fn validate(&self) -> ObservabilityResult<()> {
        // Validate level string: must contain at least one valid level token
        let directives = self.parse_directives();
        let has_any_valid = !self.level.is_empty()
            && self.level.split(',').any(|p| {
                let p = p.trim();
                if p.contains('=') {
                    let (_, lvl) = p.split_once('=').unwrap();
                    LogDirectives::str_to_level(lvl.trim()).is_some()
                } else {
                    LogDirectives::str_to_level(p).is_some()
                }
            });
        if !has_any_valid {
            return Err(ObservabilityError::configuration(format!(
                "Invalid log level: '{}'. Expected e.g. 'info' or 'info,crate_name=debug'",
                self.level
            )));
        }
        let _ = directives;

        // Validate format
        match self.format.as_str() {
            "json" | "compact" | "plain" => {}
            _ => {
                return Err(ObservabilityError::configuration(format!(
                    "Invalid format: {}. Must be 'json', 'compact', or 'plain'",
                    self.format
                )))
            }
        }

        Ok(())
    }
}

/// Core observability manager
///
/// This provides structured logging integration with standard Rust logging macros
/// (log::info!, log::debug!, etc.) without external extension system dependencies
pub struct ObservabilityManager {
    config: ObservabilityConfig,
    singleton_ref: &'static GlobalLoggerSingleton,
}

impl Default for ObservabilityManager {
    fn default() -> Self {
        let config = ObservabilityConfig::default();
        Self::new(config).expect("Failed to create default observability manager")
    }
}

impl ObservabilityManager {
    /// Create a new observability manager with configuration
    pub fn new(config: ObservabilityConfig) -> ObservabilityResult<Self> {
        // Validate configuration first
        config.validate()?;

        // Get or initialize the singleton
        let singleton_ref = GlobalLoggerSingleton::get_or_init(config.clone())?;

        Ok(Self {
            config,
            singleton_ref,
        })
    }

    /// Initialize global logging (convenience method)
    pub fn initialize(&mut self) -> ObservabilityResult<()> {
        // Initialization already happened in the singleton
        log::info!("🔍 Observability manager initialized: Using singleton global logger");
        Ok(())
    }

    /// Get current configuration
    pub fn config(&self) -> &ObservabilityConfig {
        &self.config
    }

    /// Check if logging is enabled for a level
    pub fn is_enabled(&self, level: LogLevel) -> bool {
        StandardLoggingPort::enabled(self.singleton_ref.adapter().as_ref(), &level)
    }

    /// Get the global logger instance (always available with singleton pattern)
    pub fn global_logger() -> Option<Arc<StandardLogAdapter>> {
        // With singleton pattern, try to get the instance with default config
        GlobalLoggerSingleton::get_or_init(ObservabilityConfig::default())
            .ok()
            .map(|singleton| singleton.adapter().clone())
    }

    /// Get capabilities as strings
    pub fn capabilities(&self) -> Vec<String> {
        let mut caps = vec![
            "structured_logging".to_string(),
            "standard_rust_logging".to_string(),
            "context_enrichment".to_string(),
        ];

        if self.config.structured {
            caps.push("json_output".to_string());
        }

        caps.push(format!("log_level_{}", self.config.level));
        caps.push(format!("format_{}", self.config.format));

        caps
    }
}

/// Factory function for creating observability manager from JSON configuration
pub fn create_observability_manager(
    config: Option<serde_json::Value>,
) -> ObservabilityResult<ObservabilityManager> {
    let obs_config = match config {
        Some(value) => serde_json::from_value(value).map_err(|e| {
            ObservabilityError::configuration(format!("Invalid observability config: {}", e))
        })?,
        None => ObservabilityConfig::default(),
    };

    ObservabilityManager::new(obs_config)
}

/// Convenience functions for common logging patterns
///
/// These are optional - users can still use standard log::info! etc.
/// But these provide some additional structured logging capabilities
pub mod convenience {
    use super::*;

    /// Log with additional structured fields
    pub fn log_with_fields(
        level: LogLevel,
        message: &str,
        fields: serde_json::Value,
    ) -> ObservabilityResult<()> {
        if let Some(logger) = ObservabilityManager::global_logger() {
            if StandardLoggingPort::enabled(logger.as_ref(), &level) {
                let entry = crate::domain::create_log_entry(level, message, fields);
                logger.process_standard_log(entry)?;
            }
        }
        Ok(())
    }

    /// Add context that will be included in all subsequent log entries
    /// (This would integrate with a context adapter)
    pub fn add_log_context(key: &str, value: serde_json::Value) {
        // TODO: Integrate with ContextPort implementation
        // For now, this is a placeholder for future context integration
        log::debug!("Adding log context: {} = {}", key, value);
    }

    /// Log structured info with fields
    pub fn info_with_fields(message: &str, fields: serde_json::Value) -> ObservabilityResult<()> {
        log_with_fields(LogLevel::Info, message, fields)
    }

    /// Log structured error with fields
    pub fn error_with_fields(message: &str, fields: serde_json::Value) -> ObservabilityResult<()> {
        log_with_fields(LogLevel::Error, message, fields)
    }

    /// Log structured debug with fields
    pub fn debug_with_fields(message: &str, fields: serde_json::Value) -> ObservabilityResult<()> {
        log_with_fields(LogLevel::Debug, message, fields)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_observability_config_default() {
        let config = ObservabilityConfig::default();
        assert_eq!(config.level, "info");
        assert_eq!(config.format, "compact");
        assert!(config.structured);
    }

    #[test]
    fn test_config_parse_level() {
        let config = ObservabilityConfig {
            level: "debug".to_string(),
            ..Default::default()
        };

        assert!(matches!(config.parse_level().unwrap(), LogLevel::Debug));
    }

    #[test]
    fn test_manager_creation() {
        let config = ObservabilityConfig::default();
        let manager = ObservabilityManager::new(config);
        assert!(manager.is_ok());
    }

    #[test]
    fn test_manager_capabilities() {
        let config = ObservabilityConfig::default();
        let manager = ObservabilityManager::new(config).unwrap();
        let caps = manager.capabilities();

        assert!(caps.contains(&"structured_logging".to_string()));
        assert!(caps.contains(&"standard_rust_logging".to_string()));
        assert!(caps.contains(&"log_level_info".to_string()));
    }

    #[test]
    fn test_manager_default() {
        let manager = ObservabilityManager::default();
        assert_eq!(manager.config.level, "info");
    }

    #[test]
    fn test_config_validation() {
        let config = ObservabilityConfig {
            level: "invalid".to_string(),
            ..Default::default()
        };
        assert!(config.validate().is_err());

        let config = ObservabilityConfig {
            format: "invalid".to_string(),
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }
}
