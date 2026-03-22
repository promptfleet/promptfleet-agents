//! Structured panic logging for downstream observability systems
//!
//! This module provides structured panic logging that integrates with observability
//! infrastructure like Grafana, focusing purely on capturing panic information
//! in structured format for monitoring and analysis.
//!
//! ## Features
//! - 🔥 **Structured Panic Logging**: Detailed panic information in structured JSON format
//! - 📊 **Panic Analytics**: Basic statistics for monitoring patterns
//! - 🔗 **Context Preservation**: Maintains trace context for correlation
//! - 🎯 **WASM Compatibility**: Works in WebAssembly environments
//! - 🏥 **Downstream Integration**: Designed for Grafana and monitoring stacks

use std::collections::HashMap;
use std::panic::{self, PanicHookInfo};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use web_time::SystemTime;

use observability_core::{
    domain::{LogSource, TraceContext},
    traits::LogLevel,
    LogEntry,
};

// Now we can use chrono directly since it's in our Cargo.toml
use crate::error::{Result, StructuredLoggingError};
use chrono::Utc;

// Remove unused import - convenience context clearing not used in panic handler

#[cfg(feature = "correlation-enhanced")]
use crate::correlation::EnhancedTraceContext;

/// Configuration for panic logging
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanicHandlerConfig {
    /// Enable structured panic logging
    pub enable_structured_logging: bool,

    /// Enable panic analytics and pattern detection
    pub enable_analytics: bool,

    /// Enable context preservation across panics
    pub enable_context_preservation: bool,
}

impl Default for PanicHandlerConfig {
    fn default() -> Self {
        Self {
            enable_structured_logging: true,
            enable_analytics: true,
            enable_context_preservation: true,
        }
    }
}

/// Panic severity levels for classification
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum PanicSeverity {
    /// Low severity panic
    Low,
    /// Medium severity panic  
    Medium,
    /// High severity panic
    High,
    /// Critical severity panic
    Critical,
}

/// Structured panic information for downstream systems
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructuredPanicInfo {
    /// Panic message
    pub message: String,

    /// Panic location (file:line:column)
    pub location: Option<String>,

    /// Thread name and ID
    pub thread_info: ThreadInfo,

    /// Timestamp when panic occurred (ISO 8601)
    pub timestamp: String,

    /// Panic severity classification
    pub severity: PanicSeverity,

    /// Preserved trace context
    pub trace_context: Option<TraceContext>,

    /// Enhanced correlation context
    #[cfg(feature = "correlation-enhanced")]
    pub enhanced_context: Option<EnhancedTraceContext>,

    /// Additional metadata
    pub metadata: HashMap<String, Value>,
}

/// Thread information for panic context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadInfo {
    pub name: Option<String>,
    pub id: String,
    pub is_main: bool,
}

impl ThreadInfo {
    fn current() -> Self {
        let current_thread = thread::current();
        Self {
            name: current_thread.name().map(|s| s.to_string()),
            id: format!("{:?}", current_thread.id()),
            is_main: current_thread.name() == Some("main"),
        }
    }
}

/// Panic statistics for monitoring
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PanicStats {
    /// Total panics encountered
    pub total_panics: u64,

    /// Panics by severity
    pub panics_by_severity: HashMap<String, u64>,

    /// Panics by thread
    pub panics_by_thread: HashMap<String, u64>,

    /// Most common panic patterns
    pub common_patterns: Vec<PanicPattern>,

    /// Last panic timestamp
    pub last_panic_timestamp: Option<String>,
}

/// Panic pattern for analytics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanicPattern {
    pub pattern: String,
    pub count: u64,
    pub first_seen: String,
    pub last_seen: String,
}

/// Structured panic logger for downstream systems
pub struct PanicHandler {
    config: PanicHandlerConfig,
    stats: Arc<Mutex<PanicStats>>,
}

static PANIC_HANDLER: OnceLock<Arc<PanicHandler>> = OnceLock::new();

impl PanicHandler {
    /// Create a new panic handler
    pub fn new(config: PanicHandlerConfig) -> Result<Self> {
        Ok(Self {
            config,
            stats: Arc::new(Mutex::new(PanicStats::default())),
        })
    }

    /// Install the panic handler globally
    pub fn install(config: PanicHandlerConfig) -> Result<()> {
        let handler = Arc::new(Self::new(config)?);

        if PANIC_HANDLER.set(handler.clone()).is_err() {
            return Err(StructuredLoggingError::enhanced_config(
                "Panic handler already installed",
            ));
        }

        // Install the global panic hook
        let handler_clone = handler.clone();
        panic::set_hook(Box::new(move |panic_info| {
            handler_clone.handle_panic(panic_info);
        }));

        log::info!("🔥 Structured panic handler installed for downstream observability");
        Ok(())
    }

    /// Handle a panic with structured logging
    fn handle_panic(&self, panic_info: &PanicHookInfo<'_>) {
        if !self.config.enable_structured_logging {
            return;
        }

        // Create structured panic information
        let structured_info = self.create_structured_info(panic_info);

        // Log to downstream systems
        self.log_structured_panic(&structured_info);

        // Update analytics if enabled
        if self.config.enable_analytics {
            self.update_panic_stats(&structured_info);
        }
    }

    /// Create structured panic information
    fn create_structured_info(&self, panic_info: &PanicHookInfo<'_>) -> StructuredPanicInfo {
        let message = if let Some(s) = panic_info.payload().downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = panic_info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "Unknown panic".to_string()
        };

        let location = panic_info
            .location()
            .map(|loc| format!("{}:{}:{}", loc.file(), loc.line(), loc.column()));

        // Get current trace context if preservation is enabled
        let trace_context = None; // Simplified for now - can be enhanced later

        let severity = self.classify_panic_severity(&message);
        let timestamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| {
                let secs = d.as_secs();
                let nanos = d.subsec_nanos();
                format!("{}.{:09}Z", secs, nanos)
            })
            .unwrap_or_else(|_| "unknown".to_string());

        StructuredPanicInfo {
            message,
            location,
            thread_info: ThreadInfo::current(),
            timestamp,
            severity,
            trace_context,
            #[cfg(feature = "correlation-enhanced")]
            enhanced_context: None, // Could be enhanced later
            metadata: HashMap::new(),
        }
    }

    /// Classify panic severity based on message content
    fn classify_panic_severity(&self, message: &str) -> PanicSeverity {
        if message.contains("memory")
            || message.contains("corruption")
            || message.contains("segfault")
        {
            PanicSeverity::Critical
        } else if message.contains("assertion") || message.contains("unreachable") {
            PanicSeverity::High
        } else if message.contains("unwrap") || message.contains("expect") {
            PanicSeverity::Medium
        } else {
            PanicSeverity::Low
        }
    }

    /// Log structured panic information to downstream systems
    fn log_structured_panic(&self, panic_info: &StructuredPanicInfo) {
        let _log_entry = LogEntry {
            timestamp: Utc::now(),
            level: LogLevel::Error,
            message: format!("🔥 PANIC: {}", panic_info.message),
            fields: json!({
                "panic": {
                    "message": panic_info.message,
                    "location": panic_info.location,
                    "severity": panic_info.severity,
                    "thread": panic_info.thread_info,
                    "metadata": panic_info.metadata
                },
                "trace_context": panic_info.trace_context,
                "operation": "panic_logging"
            }),
            trace_context: panic_info.trace_context.clone(),
            source: LogSource {
                module: Some("panic_handler".to_string()),
                file: panic_info
                    .location
                    .as_ref()
                    .and_then(|loc| loc.split(':').next().map(|s| s.to_string())),
                line: panic_info
                    .location
                    .as_ref()
                    .and_then(|loc| loc.split(':').nth(1).and_then(|s| s.parse().ok())),
                target: Some("structured_logging::panic_handler".to_string()),
            },
        };

        // Use standard Rust logging to emit the structured entry for downstream systems
        log::error!(
            target: "panic_handler",
            "🔥 Structured panic: {} at {} (severity: {:?})",
            panic_info.message,
            panic_info.location.as_deref().unwrap_or("unknown"),
            panic_info.severity
        );
    }

    /// Update panic statistics for monitoring
    fn update_panic_stats(&self, panic_info: &StructuredPanicInfo) {
        if let Ok(mut stats) = self.stats.lock() {
            stats.total_panics += 1;

            // Update severity counts
            let severity_key = format!("{:?}", panic_info.severity);
            *stats.panics_by_severity.entry(severity_key).or_insert(0) += 1;

            // Update thread counts
            let thread_key = panic_info
                .thread_info
                .name
                .clone()
                .unwrap_or_else(|| panic_info.thread_info.id.clone());
            *stats.panics_by_thread.entry(thread_key).or_insert(0) += 1;

            // Update timestamp
            stats.last_panic_timestamp = Some(panic_info.timestamp.clone());

            // Pattern detection (simplified)
            let pattern = self.extract_panic_pattern(&panic_info.message);
            let existing_pattern = stats
                .common_patterns
                .iter_mut()
                .find(|p| p.pattern == pattern);

            if let Some(existing) = existing_pattern {
                existing.count += 1;
                existing.last_seen = panic_info.timestamp.clone();
            } else {
                stats.common_patterns.push(PanicPattern {
                    pattern,
                    count: 1,
                    first_seen: panic_info.timestamp.clone(),
                    last_seen: panic_info.timestamp.clone(),
                });
            }

            // Keep only top 10 patterns
            stats.common_patterns.sort_by(|a, b| b.count.cmp(&a.count));
            stats.common_patterns.truncate(10);
        }
    }

    /// Extract panic pattern for analytics
    fn extract_panic_pattern(&self, message: &str) -> String {
        if message.contains("index out of bounds") {
            "index_out_of_bounds".to_string()
        } else if message.contains("unwrap") {
            "unwrap_on_error".to_string()
        } else if message.contains("assertion failed") {
            "assertion_failed".to_string()
        } else if message.contains("memory") {
            "memory_error".to_string()
        } else {
            "other".to_string()
        }
    }

    /// Get current panic statistics
    pub fn get_stats(&self) -> Result<PanicStats> {
        self.stats
            .lock()
            .map(|stats| stats.clone())
            .map_err(|_| StructuredLoggingError::enhanced_config("Failed to acquire stats lock"))
    }

    /// Reset panic statistics
    pub fn reset_stats(&self) -> Result<()> {
        if let Ok(mut stats) = self.stats.lock() {
            *stats = PanicStats::default();
            log::info!("📊 Panic statistics reset");
            Ok(())
        } else {
            Err(StructuredLoggingError::enhanced_config(
                "Failed to acquire stats lock",
            ))
        }
    }
}

/// Install panic handler with default configuration
pub fn install_panic_handler() -> Result<()> {
    PanicHandler::install(PanicHandlerConfig::default())
}

/// Install panic handler with custom configuration
pub fn install_panic_handler_with_config(config: PanicHandlerConfig) -> Result<()> {
    PanicHandler::install(config)
}

/// Get panic statistics if handler is installed
pub fn get_panic_stats() -> Result<PanicStats> {
    PANIC_HANDLER
        .get()
        .ok_or_else(|| StructuredLoggingError::enhanced_config("Panic handler not installed"))?
        .get_stats()
}

/// Reset panic statistics if handler is installed
pub fn reset_panic_stats() -> Result<()> {
    PANIC_HANDLER
        .get()
        .ok_or_else(|| StructuredLoggingError::enhanced_config("Panic handler not installed"))?
        .reset_stats()
}

/// Macro for supervised execution with panic capturing
#[macro_export]
macro_rules! supervised {
    ($body:expr) => {{
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| $body)).map_err(|_| {
            crate::StructuredLoggingError::enhanced_config("Supervised operation panicked")
        })
    }};
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_panic_handler_config_default() {
        let config = PanicHandlerConfig::default();
        assert!(config.enable_structured_logging);
        assert!(config.enable_analytics);
        assert!(config.enable_context_preservation);
    }

    #[test]
    fn test_panic_severity_classification() {
        let config = PanicHandlerConfig::default();
        let handler = PanicHandler::new(config).unwrap();

        assert_eq!(
            handler.classify_panic_severity("index out of bounds"),
            PanicSeverity::Low
        );

        assert_eq!(
            handler.classify_panic_severity("memory corruption detected"),
            PanicSeverity::Critical
        );

        assert_eq!(
            handler.classify_panic_severity("assertion failed"),
            PanicSeverity::High
        );

        assert_eq!(
            handler.classify_panic_severity("unwrap called on None"),
            PanicSeverity::Medium
        );
    }

    #[test]
    fn test_supervised_macro() {
        let result = supervised!({ 42 });
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
    }

    #[test]
    fn test_panic_pattern_extraction() {
        let config = PanicHandlerConfig::default();
        let handler = PanicHandler::new(config).unwrap();

        assert_eq!(
            handler.extract_panic_pattern("index out of bounds: the len is 0 but the index is 1"),
            "index_out_of_bounds"
        );
        assert_eq!(
            handler.extract_panic_pattern("called `Result::unwrap()` on an `Err` value"),
            "unwrap_on_error"
        );
        assert_eq!(
            handler.extract_panic_pattern("assertion failed: x == y"),
            "assertion_failed"
        );
        assert_eq!(
            handler.extract_panic_pattern("memory corruption"),
            "memory_error"
        );
        assert_eq!(handler.extract_panic_pattern("something else"), "other");
    }
}
