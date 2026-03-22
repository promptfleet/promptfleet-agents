//! Domain layer for observability core - Pure business logic
//!
//! This module contains ONLY pure business logic with no external dependencies.
//! No WASM, HTTP, or framework-specific code should be here.

use crate::error::{ObservabilityError, ObservabilityResult};
use crate::traits::LogLevel;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Core log entry - pure data structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub timestamp: DateTime<Utc>,
    pub level: LogLevel,
    pub message: String,
    pub fields: serde_json::Value,
    pub trace_context: Option<TraceContext>,
    pub source: LogSource,
}

/// Source of the log entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogSource {
    pub module: Option<String>,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub target: Option<String>,
}

/// Serializable trace identifiers for log/metric correlation.
///
/// Distinct from [`crate::context::TraceContext`] which is the runtime
/// tracing context with sampling decisions and W3C conversion methods.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceCorrelation {
    pub trace_id: String,
    pub span_id: String,
    pub parent_span_id: Option<String>,
}

/// Backwards-compatible alias.
pub type TraceContext = TraceCorrelation;

/// Core processor interface - transforms log entries
pub trait LogProcessor: Send + Sync + std::fmt::Debug {
    /// Process and transform a log entry
    fn process(&self, entry: LogEntry) -> ObservabilityResult<LogEntry>;

    /// Get processor name for debugging
    fn name(&self) -> &'static str;
}

/// Processor chain pattern (inspired by structlog)
#[derive(Debug)]
pub struct ProcessorChain {
    processors: Vec<Box<dyn LogProcessor>>,
}

impl ProcessorChain {
    /// Create a new empty processor chain
    pub fn new() -> Self {
        Self {
            processors: Vec::new(),
        }
    }

    /// Add a processor to the chain
    pub fn add_processor(mut self, processor: Box<dyn LogProcessor>) -> Self {
        self.processors.push(processor);
        self
    }

    /// Process an entry through the entire chain
    pub fn process(&self, entry: LogEntry) -> ObservabilityResult<LogEntry> {
        let mut processed = entry;

        for processor in &self.processors {
            processed = processor.process(processed).map_err(|e| {
                ObservabilityError::logging(format!(
                    "Processor '{}' failed: {}",
                    processor.name(),
                    e
                ))
            })?;
        }

        Ok(processed)
    }

    /// Get number of processors in the chain
    pub fn len(&self) -> usize {
        self.processors.len()
    }

    /// Check if chain is empty
    pub fn is_empty(&self) -> bool {
        self.processors.is_empty()
    }
}

impl Default for ProcessorChain {
    fn default() -> Self {
        Self::new()
    }
}

// ==================== BUILT-IN PROCESSORS ====================

/// Processor that adds timestamps
#[derive(Debug)]
pub struct TimestampProcessor;

impl LogProcessor for TimestampProcessor {
    fn process(&self, mut entry: LogEntry) -> ObservabilityResult<LogEntry> {
        entry.timestamp = Utc::now();
        Ok(entry)
    }

    fn name(&self) -> &'static str {
        "timestamp"
    }
}

/// Processor that enriches with context
#[derive(Debug)]
pub struct ContextEnricher {
    additional_fields: HashMap<String, serde_json::Value>,
}

impl ContextEnricher {
    pub fn new() -> Self {
        Self {
            additional_fields: HashMap::new(),
        }
    }

    pub fn with_field(
        mut self,
        key: impl Into<String>,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.additional_fields.insert(key.into(), value.into());
        self
    }
}

impl LogProcessor for ContextEnricher {
    fn process(&self, mut entry: LogEntry) -> ObservabilityResult<LogEntry> {
        // Add additional context fields
        if let serde_json::Value::Object(ref mut map) = entry.fields {
            for (key, value) in &self.additional_fields {
                if !map.contains_key(key) {
                    map.insert(key.clone(), value.clone());
                }
            }
        }

        Ok(entry)
    }

    fn name(&self) -> &'static str {
        "context_enricher"
    }
}

/// Processor that structures fields
#[derive(Debug)]
pub struct StructuredFieldsProcessor;

impl LogProcessor for StructuredFieldsProcessor {
    fn process(&self, mut entry: LogEntry) -> ObservabilityResult<LogEntry> {
        // Ensure fields is always an object
        if !entry.fields.is_object() {
            entry.fields = serde_json::json!({});
        }

        // Add standard fields
        if let serde_json::Value::Object(ref mut map) = entry.fields {
            map.insert(
                "timestamp".to_string(),
                serde_json::json!(entry.timestamp.to_rfc3339()),
            );
            map.insert("level".to_string(), serde_json::json!(entry.level.as_str()));
            map.insert("message".to_string(), serde_json::json!(entry.message));

            // Add source information if available
            if let Some(ref module) = entry.source.module {
                map.insert("module".to_string(), serde_json::json!(module));
            }
            if let Some(ref file) = entry.source.file {
                map.insert("file".to_string(), serde_json::json!(file));
            }
            if let Some(line) = entry.source.line {
                map.insert("line".to_string(), serde_json::json!(line));
            }

            // Add trace context if available
            if let Some(ref trace_ctx) = entry.trace_context {
                map.insert(
                    "trace_id".to_string(),
                    serde_json::json!(trace_ctx.trace_id),
                );
                map.insert("span_id".to_string(), serde_json::json!(trace_ctx.span_id));
                if let Some(ref parent) = trace_ctx.parent_span_id {
                    map.insert("parent_span_id".to_string(), serde_json::json!(parent));
                }
            }
        }

        Ok(entry)
    }

    fn name(&self) -> &'static str {
        "structured_fields"
    }
}

/// Filter processor that filters out logs below certain level
#[derive(Debug)]
pub struct LevelFilter {
    min_level: LogLevel,
}

impl LevelFilter {
    pub fn new(min_level: LogLevel) -> Self {
        Self { min_level }
    }
}

impl LogProcessor for LevelFilter {
    fn process(&self, entry: LogEntry) -> ObservabilityResult<LogEntry> {
        if entry.level <= self.min_level {
            Ok(entry)
        } else {
            Err(ObservabilityError::logging("Log level filtered out"))
        }
    }

    fn name(&self) -> &'static str {
        "level_filter"
    }
}

/// Processor that extracts structured fields from log::kv
///
/// This processor supports the log::info!("msg"; "key" => value) syntax
/// by extracting key-value pairs from log::Record and adding them to LogEntry fields
#[derive(Debug)]
pub struct LogKvExtractor;

impl LogKvExtractor {
    pub fn new() -> Self {
        Self
    }

    /// Extract key-value pairs from log::Record
    ///
    /// This function would be called by StandardLogAdapter when processing log::Record
    /// but we'll implement the interface here for the processor chain
    pub fn extract_kv_from_record(record: &log::Record) -> serde_json::Value {
        let mut fields = serde_json::Map::new();

        // Extract key-value pairs using log::kv
        let key_values = record.key_values();
        let mut visitor = LogKvVisitor::new(&mut fields);
        let _ = key_values.visit(&mut visitor);

        serde_json::Value::Object(fields)
    }
}

impl LogProcessor for LogKvExtractor {
    fn process(&self, entry: LogEntry) -> ObservabilityResult<LogEntry> {
        // In the processor chain context, we assume fields are already extracted
        // by StandardLogAdapter, so this processor mainly ensures proper structure

        // Ensure fields is always an object for consistency
        let mut entry = entry;
        if !entry.fields.is_object() {
            entry.fields = serde_json::json!({});
        }

        // Add metadata about kv extraction
        if let serde_json::Value::Object(ref mut map) = entry.fields {
            if !map.contains_key("kv_extracted") {
                map.insert("kv_extracted".to_string(), serde_json::json!(true));
            }
        }

        Ok(entry)
    }

    fn name(&self) -> &'static str {
        "log_kv_extractor"
    }
}

impl Default for LogKvExtractor {
    fn default() -> Self {
        Self::new()
    }
}

/// Visitor for extracting log::kv key-value pairs
struct LogKvVisitor<'a> {
    fields: &'a mut serde_json::Map<String, serde_json::Value>,
}

impl<'a> LogKvVisitor<'a> {
    fn new(fields: &'a mut serde_json::Map<String, serde_json::Value>) -> Self {
        Self { fields }
    }
}

impl<'a> log::kv::Visitor<'a> for LogKvVisitor<'a> {
    fn visit_pair(
        &mut self,
        key: log::kv::Key,
        value: log::kv::Value,
    ) -> Result<(), log::kv::Error> {
        let key_str = key.as_str();

        // Convert log::kv::Value to serde_json::Value
        let json_value = match value.to_borrowed_str() {
            Some(s) => serde_json::json!(s),
            None => {
                // Handle other value types
                if let Some(i) = value.to_i64() {
                    serde_json::json!(i)
                } else if let Some(u) = value.to_u64() {
                    serde_json::json!(u)
                } else if let Some(f) = value.to_f64() {
                    serde_json::json!(f)
                } else if let Some(b) = value.to_bool() {
                    serde_json::json!(b)
                } else {
                    // Fallback to debug representation
                    serde_json::json!(format!("{:?}", value))
                }
            }
        };

        self.fields.insert(key_str.to_string(), json_value);
        Ok(())
    }
}

/// Enhanced processor that combines context enrichment with kv extraction
#[derive(Debug)]
pub struct EnhancedContextEnricher {
    additional_fields: HashMap<String, serde_json::Value>,
    extract_kv: bool,
}

impl EnhancedContextEnricher {
    pub fn new() -> Self {
        Self {
            additional_fields: HashMap::new(),
            extract_kv: true,
        }
    }

    pub fn with_field(
        mut self,
        key: impl Into<String>,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.additional_fields.insert(key.into(), value.into());
        self
    }

    pub fn with_kv_extraction(mut self, extract_kv: bool) -> Self {
        self.extract_kv = extract_kv;
        self
    }
}

impl LogProcessor for EnhancedContextEnricher {
    fn process(&self, mut entry: LogEntry) -> ObservabilityResult<LogEntry> {
        // Add additional context fields (same as ContextEnricher)
        if let serde_json::Value::Object(ref mut map) = entry.fields {
            for (key, value) in &self.additional_fields {
                if !map.contains_key(key) {
                    map.insert(key.clone(), value.clone());
                }
            }

            // Add extraction metadata
            if self.extract_kv {
                map.insert("enhanced_context".to_string(), serde_json::json!(true));
            }
        }

        Ok(entry)
    }

    fn name(&self) -> &'static str {
        "enhanced_context_enricher"
    }
}

impl Default for EnhancedContextEnricher {
    fn default() -> Self {
        Self::new()
    }
}

// ==================== UTILITY FUNCTIONS ====================

/// Build a default processor chain with common processors
pub fn build_default_processor_chain() -> ProcessorChain {
    ProcessorChain::new()
        .add_processor(Box::new(TimestampProcessor))
        .add_processor(Box::new(LogKvExtractor::new()))
        .add_processor(Box::new(EnhancedContextEnricher::new()))
        .add_processor(Box::new(StructuredFieldsProcessor))
}

/// Build an enhanced processor chain with additional features
pub fn build_enhanced_processor_chain() -> ProcessorChain {
    ProcessorChain::new()
        .add_processor(Box::new(TimestampProcessor))
        .add_processor(Box::new(LogKvExtractor::new()))
        .add_processor(Box::new(
            EnhancedContextEnricher::new()
                .with_field("sdk_version", env!("CARGO_PKG_VERSION"))
                .with_field("architecture", "hexagonal"),
        ))
        .add_processor(Box::new(StructuredFieldsProcessor))
}

/// Create a log entry from basic components
pub fn create_log_entry(
    level: LogLevel,
    message: impl Into<String>,
    fields: serde_json::Value,
) -> LogEntry {
    LogEntry {
        timestamp: Utc::now(),
        level,
        message: message.into(),
        fields,
        trace_context: None,
        source: LogSource {
            module: None,
            file: None,
            line: None,
            target: None,
        },
    }
}

/// Basic metric entry for correlation with logs
#[derive(Debug, Clone)]
pub struct MetricsEntry {
    pub name: String,
    pub value: f64,
    pub metric_type: BasicMetricType,
    pub timestamp: DateTime<Utc>,
    pub trace_context: Option<TraceContext>,
    pub source: MetricsSource,
}

/// Simple metric types supported by core
#[derive(Debug, Clone, PartialEq)]
pub enum BasicMetricType {
    /// Counter metric (monotonically increasing)
    Counter,
    /// Histogram/timing metric (distribution of values)
    Histogram,
    /// Gauge metric (current value)
    Gauge,
}

/// Source of the metric
#[derive(Debug, Clone)]
pub struct MetricsSource {
    pub module: Option<String>,
    pub component: Option<String>,
    pub operation: Option<String>,
}

impl MetricsEntry {
    /// Create a new metrics entry
    pub fn new(name: impl Into<String>, value: f64, metric_type: BasicMetricType) -> Self {
        Self {
            name: name.into(),
            value,
            metric_type,
            timestamp: Utc::now(),
            trace_context: None,
            source: MetricsSource {
                module: None,
                component: None,
                operation: None,
            },
        }
    }

    /// Add trace context for correlation
    pub fn with_trace_context(mut self, trace_context: TraceContext) -> Self {
        self.trace_context = Some(trace_context);
        self
    }

    /// Add source information
    pub fn with_source(
        mut self,
        module: Option<String>,
        component: Option<String>,
        operation: Option<String>,
    ) -> Self {
        self.source = MetricsSource {
            module,
            component,
            operation,
        };
        self
    }

    /// Convert to JSON for transport
    pub fn to_json(&self) -> serde_json::Value {
        let mut json = serde_json::json!({
            "name": self.name,
            "value": self.value,
            "type": match self.metric_type {
                BasicMetricType::Counter => "counter",
                BasicMetricType::Histogram => "histogram",
                BasicMetricType::Gauge => "gauge",
            },
            "timestamp": self.timestamp.to_rfc3339(),
        });

        // Add trace context if present
        if let Some(ref trace_ctx) = self.trace_context {
            json["trace_id"] = serde_json::json!(trace_ctx.trace_id);
            json["span_id"] = serde_json::json!(trace_ctx.span_id);
            if let Some(ref parent) = trace_ctx.parent_span_id {
                json["parent_span_id"] = serde_json::json!(parent);
            }
        }

        // Add source info if present
        if let Some(ref module) = self.source.module {
            json["module"] = serde_json::json!(module);
        }
        if let Some(ref component) = self.source.component {
            json["component"] = serde_json::json!(component);
        }
        if let Some(ref operation) = self.source.operation {
            json["operation"] = serde_json::json!(operation);
        }

        json
    }
}

/// Convenience function to create a counter metric
pub fn create_counter_metric(name: impl Into<String>, value: f64) -> MetricsEntry {
    MetricsEntry::new(name, value, BasicMetricType::Counter)
}

/// Convenience function to create a histogram metric
pub fn create_histogram_metric(name: impl Into<String>, value: f64) -> MetricsEntry {
    MetricsEntry::new(name, value, BasicMetricType::Histogram)
}

/// Convenience function to create a gauge metric
pub fn create_gauge_metric(name: impl Into<String>, value: f64) -> MetricsEntry {
    MetricsEntry::new(name, value, BasicMetricType::Gauge)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_processor_chain() {
        let chain = ProcessorChain::new()
            .add_processor(Box::new(TimestampProcessor))
            .add_processor(Box::new(StructuredFieldsProcessor));

        let entry = create_log_entry(
            LogLevel::Info,
            "Test message",
            serde_json::json!({"key": "value"}),
        );

        let processed = chain.process(entry).unwrap();

        assert_eq!(processed.level, LogLevel::Info);
        assert_eq!(processed.message, "Test message");
        assert!(processed.fields.get("timestamp").is_some());
        assert!(processed.fields.get("level").is_some());
    }

    #[test]
    fn test_context_enricher() {
        let enricher = ContextEnricher::new()
            .with_field("service", "test-service")
            .with_field("version", "1.0.0");

        let entry = create_log_entry(
            LogLevel::Info,
            "Test",
            serde_json::json!({"existing": "field"}),
        );

        let processed = enricher.process(entry).unwrap();

        let fields = processed.fields.as_object().unwrap();
        assert_eq!(fields.get("service").unwrap(), "test-service");
        assert_eq!(fields.get("version").unwrap(), "1.0.0");
        assert_eq!(fields.get("existing").unwrap(), "field");
    }

    #[test]
    fn test_level_filter() {
        let filter = LevelFilter::new(LogLevel::Info);

        let info_entry = create_log_entry(LogLevel::Info, "Info message", serde_json::json!({}));
        let debug_entry = create_log_entry(LogLevel::Debug, "Debug message", serde_json::json!({}));

        assert!(filter.process(info_entry).is_ok());
        assert!(filter.process(debug_entry).is_err());
    }
}
