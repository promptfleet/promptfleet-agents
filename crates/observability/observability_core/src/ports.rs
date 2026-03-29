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

/// Port for metrics collection (basic interface for correlation).
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{BasicMetricType, LogEntry, MetricsEntry};
    use crate::error::ObservabilityError;
    use crate::traits::LogLevel;
    use std::sync::Mutex;

    fn dummy_entry() -> LogEntry {
        crate::domain::create_log_entry(LogLevel::Info, "test", serde_json::json!({"key": "value"}))
    }

    // --- TransportPort default batch ---

    struct RecordingTransport {
        entries: Mutex<Vec<String>>,
    }

    impl RecordingTransport {
        fn new() -> Self {
            Self {
                entries: Mutex::new(Vec::new()),
            }
        }
    }

    impl TransportPort for RecordingTransport {
        fn transport(&self, entry: &LogEntry) -> ObservabilityResult<()> {
            self.entries.lock().unwrap().push(entry.message.clone());
            Ok(())
        }
    }

    #[test]
    fn test_transport_batch_default_calls_transport_per_entry() {
        let transport = RecordingTransport::new();
        let entries: Vec<LogEntry> = (0..3).map(|_| dummy_entry()).collect();
        transport.transport_batch(&entries).unwrap();
        assert_eq!(transport.entries.lock().unwrap().len(), 3);
    }

    #[test]
    fn test_transport_batch_empty_is_ok() {
        let transport = RecordingTransport::new();
        transport.transport_batch(&[]).unwrap();
        assert!(transport.entries.lock().unwrap().is_empty());
    }

    // --- MetricsPort default batch ---

    struct RecordingMetrics {
        counters: Mutex<Vec<(String, f64)>>,
        histograms: Mutex<Vec<(String, f64)>>,
        gauges: Mutex<Vec<(String, f64)>>,
    }

    impl RecordingMetrics {
        fn new() -> Self {
            Self {
                counters: Mutex::new(Vec::new()),
                histograms: Mutex::new(Vec::new()),
                gauges: Mutex::new(Vec::new()),
            }
        }
    }

    impl MetricsPort for RecordingMetrics {
        fn emit_counter_simple(&self, name: &str, value: f64) -> ObservabilityResult<()> {
            self.counters
                .lock()
                .unwrap()
                .push((name.to_string(), value));
            Ok(())
        }

        fn emit_histogram_simple(&self, name: &str, value: f64) -> ObservabilityResult<()> {
            self.histograms
                .lock()
                .unwrap()
                .push((name.to_string(), value));
            Ok(())
        }

        fn emit_gauge_simple(&self, name: &str, value: f64) -> ObservabilityResult<()> {
            self.gauges.lock().unwrap().push((name.to_string(), value));
            Ok(())
        }

        fn is_enabled(&self) -> bool {
            true
        }
    }

    #[test]
    fn test_metrics_batch_routes_by_metric_type() {
        let metrics = RecordingMetrics::new();
        let entries = vec![
            MetricsEntry::new("req_total", 1.0, BasicMetricType::Counter),
            MetricsEntry::new("latency_ms", 42.0, BasicMetricType::Histogram),
            MetricsEntry::new("queue_depth", 7.0, BasicMetricType::Gauge),
        ];

        metrics.emit_metrics_batch(&entries).unwrap();

        assert_eq!(metrics.counters.lock().unwrap().len(), 1);
        assert_eq!(metrics.histograms.lock().unwrap().len(), 1);
        assert_eq!(metrics.gauges.lock().unwrap().len(), 1);
        assert_eq!(metrics.counters.lock().unwrap()[0].0, "req_total");
        assert_eq!(metrics.histograms.lock().unwrap()[0].1, 42.0);
        assert_eq!(metrics.gauges.lock().unwrap()[0].1, 7.0);
    }

    #[test]
    fn test_metrics_batch_empty_is_ok() {
        let metrics = RecordingMetrics::new();
        metrics.emit_metrics_batch(&[]).unwrap();
    }

    // --- ContextPort ---

    struct InMemoryContext {
        fields: Mutex<HashMap<String, serde_json::Value>>,
    }

    impl InMemoryContext {
        fn new() -> Self {
            Self {
                fields: Mutex::new(HashMap::new()),
            }
        }
    }

    impl ContextPort for InMemoryContext {
        fn get_context(&self) -> HashMap<String, serde_json::Value> {
            self.fields.lock().unwrap().clone()
        }
        fn add_context(&self, key: String, value: serde_json::Value) {
            self.fields.lock().unwrap().insert(key, value);
        }
        fn remove_context(&self, key: &str) {
            self.fields.lock().unwrap().remove(key);
        }
        fn clear_context(&self) {
            self.fields.lock().unwrap().clear();
        }
    }

    #[test]
    fn test_context_port_add_get_remove_clear() {
        let ctx = InMemoryContext::new();
        assert!(ctx.get_context().is_empty());

        ctx.add_context("agent".into(), serde_json::json!("echo"));
        assert_eq!(ctx.get_context().len(), 1);
        assert_eq!(ctx.get_context()["agent"], serde_json::json!("echo"));

        ctx.add_context("version".into(), serde_json::json!("1.0"));
        assert_eq!(ctx.get_context().len(), 2);

        ctx.remove_context("agent");
        assert_eq!(ctx.get_context().len(), 1);
        assert!(!ctx.get_context().contains_key("agent"));

        ctx.clear_context();
        assert!(ctx.get_context().is_empty());
    }

    // --- FormatterPort ---

    struct JsonFmt;

    impl FormatterPort for JsonFmt {
        fn format(&self, entry: &LogEntry) -> ObservabilityResult<String> {
            serde_json::to_string(entry)
                .map_err(|e| ObservabilityError::serialization(e.to_string()))
        }
    }

    #[test]
    fn test_formatter_port_produces_valid_json() {
        let fmt = JsonFmt;
        let entry = dummy_entry();
        let output = fmt.format(&entry).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(parsed["message"], "test");
    }

    // --- BatchingPort ---

    struct InMemoryBatcher {
        batch: Mutex<Vec<LogEntry>>,
    }

    impl InMemoryBatcher {
        fn new() -> Self {
            Self {
                batch: Mutex::new(Vec::new()),
            }
        }
    }

    impl BatchingPort for InMemoryBatcher {
        fn add_to_batch(&self, entry: LogEntry) -> ObservabilityResult<()> {
            self.batch.lock().unwrap().push(entry);
            Ok(())
        }
        fn flush_batch(&self) -> ObservabilityResult<()> {
            self.batch.lock().unwrap().clear();
            Ok(())
        }
        fn batch_size(&self) -> usize {
            self.batch.lock().unwrap().len()
        }
    }

    #[test]
    fn test_batching_port_add_size_flush() {
        let batcher = InMemoryBatcher::new();
        assert_eq!(batcher.batch_size(), 0);

        batcher.add_to_batch(dummy_entry()).unwrap();
        batcher.add_to_batch(dummy_entry()).unwrap();
        assert_eq!(batcher.batch_size(), 2);

        batcher.flush_batch().unwrap();
        assert_eq!(batcher.batch_size(), 0);
    }

    // --- StandardLoggingPort ---

    struct DummyLogging;

    impl StandardLoggingPort for DummyLogging {
        fn initialize(&self) -> ObservabilityResult<()> {
            Ok(())
        }
        fn process_standard_log(&self, _entry: LogEntry) -> ObservabilityResult<()> {
            Ok(())
        }
        fn enabled(&self, _level: &LogLevel) -> bool {
            true
        }
    }

    #[test]
    fn test_standard_logging_port_initialize_and_enabled() {
        let logging = DummyLogging;
        logging.initialize().unwrap();
        assert!(logging.enabled(&LogLevel::Info));
        assert!(logging.enabled(&LogLevel::Error));
        logging.process_standard_log(dummy_entry()).unwrap();
    }
}
