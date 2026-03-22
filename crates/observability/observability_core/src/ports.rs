//! Ports layer - Abstract interfaces for hexagonal architecture
//!
//! This module defines the ports (interfaces) that the core domain uses
//! to interact with the external world. Adapters implement these ports.

use crate::domain::LogEntry;
use crate::error::ObservabilityResult;
use std::collections::HashMap;

/// Port for transporting log entries to external systems
///
/// This is the interface that different adapters implement:
/// - WasmStdoutAdapter (writes to WASM stdout)
/// - HttpTransportAdapter (sends via HTTP)
/// - BatchingTransportAdapter (batches entries)
pub trait TransportPort: Send + Sync {
    /// Transport a single log entry
    fn transport(&self, entry: &LogEntry) -> ObservabilityResult<()>;

    /// Transport multiple log entries (for batching)
    fn transport_batch(&self, entries: &[LogEntry]) -> ObservabilityResult<()> {
        // Default implementation - transport one by one
        for entry in entries {
            self.transport(entry)?;
        }
        Ok(())
    }
}

/// Port for formatting log entries
///
/// Different formatters can be plugged in:
/// - JsonFormatter (structured JSON output)
/// - PlainTextFormatter (human-readable text)
/// - CompactFormatter (single-line JSON)
pub trait FormatterPort: Send + Sync {
    /// Format a log entry into a string
    fn format(&self, entry: &LogEntry) -> ObservabilityResult<String>;
}

/// Port for standard logging integration
///
/// This port allows us to hook into standard Rust logging infrastructure:
/// - log::Log implementation
/// - tracing::Subscriber implementation
pub trait StandardLoggingPort: Send + Sync {
    /// Initialize the logging system (called once during agent startup)
    fn initialize(&self) -> ObservabilityResult<()>;

    /// Process a log entry from standard logging macros
    fn process_standard_log(&self, entry: LogEntry) -> ObservabilityResult<()>;

    /// Check if logging is enabled for this level
    fn enabled(&self, level: &crate::traits::LogLevel) -> bool;
}

/// Port for accessing context information
///
/// This allows the logging system to enrich entries with context:
/// - Agent ID, request ID, trace ID
/// - User-defined context fields
/// - Thread/task local context
pub trait ContextPort: Send + Sync {
    /// Get current context fields
    fn get_context(&self) -> HashMap<String, serde_json::Value>;

    /// Add a context field
    fn add_context(&self, key: String, value: serde_json::Value);

    /// Remove a context field
    fn remove_context(&self, key: &str);

    /// Clear all context
    fn clear_context(&self);
}

/// Port for batching log entries
///
/// This allows different batching strategies:
/// - TimeBasedBatcher (flush every N seconds)
/// - SizeBasedBatcher (flush when buffer reaches N entries)
/// - HybridBatcher (combination of time and size)
pub trait BatchingPort: Send + Sync {
    /// Add an entry to the batch
    fn add_to_batch(&self, entry: LogEntry) -> ObservabilityResult<()>;

    /// Force flush the current batch
    fn flush_batch(&self) -> ObservabilityResult<()>;

    /// Get current batch size
    fn batch_size(&self) -> usize;
}

/// Port for metrics collection (basic interface for correlation)
pub trait MetricsPort: Send + Sync {
    /// Emit a simple counter metric
    fn emit_counter_simple(&self, name: &str, value: f64) -> ObservabilityResult<()>;

    /// Emit a simple histogram/timing metric
    fn emit_histogram_simple(&self, name: &str, value: f64) -> ObservabilityResult<()>;

    /// Emit a simple gauge metric
    fn emit_gauge_simple(&self, name: &str, value: f64) -> ObservabilityResult<()>;

    /// Check if metrics collection is enabled
    fn is_enabled(&self) -> bool;

    /// Batch emit multiple metrics (optional, has default implementation)
    fn emit_metrics_batch(
        &self,
        entries: &[crate::domain::MetricsEntry],
    ) -> ObservabilityResult<()> {
        // Default implementation - emit one by one
        for entry in entries {
            match entry.metric_type {
                crate::domain::BasicMetricType::Counter => {
                    self.emit_counter_simple(&entry.name, entry.value)?;
                }
                crate::domain::BasicMetricType::Histogram => {
                    self.emit_histogram_simple(&entry.name, entry.value)?;
                }
                crate::domain::BasicMetricType::Gauge => {
                    self.emit_gauge_simple(&entry.name, entry.value)?;
                }
            }
        }
        Ok(())
    }
}
