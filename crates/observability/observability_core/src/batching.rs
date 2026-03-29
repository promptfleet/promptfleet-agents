//! Smart batching system for telemetry data to optimize performance

use crate::error::{ObservabilityError, ObservabilityResult};
use crate::traits::{LogLevel, SpanStatus};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use web_time::{Duration, Instant};

#[cfg(feature = "structured-logging")]
use serde_json::Value as JsonValue;

/// Configuration for batching behavior
#[derive(Debug, Clone)]
pub struct BatchingConfig {
    /// Maximum number of items in a batch
    pub max_batch_size: usize,
    /// Maximum time to wait before flushing a batch
    pub flush_interval: Duration,
    /// Maximum memory usage in bytes (approximate)
    pub max_memory_bytes: usize,
    /// Whether to drop items when buffer is full
    pub drop_on_overflow: bool,
    /// Minimum batch size to trigger early flush
    pub min_batch_size: usize,
}

impl Default for BatchingConfig {
    fn default() -> Self {
        Self {
            max_batch_size: 100,
            flush_interval: Duration::from_secs(5),
            max_memory_bytes: 1024 * 1024, // 1MB
            drop_on_overflow: true,
            min_batch_size: 10,
        }
    }
}

/// Telemetry data types for batching
#[derive(Debug, Clone)]
pub enum TelemetryData {
    Span(SpanData),
    Metric(MetricData),
    #[cfg(feature = "structured-logging")]
    Log(LogData),
}

impl TelemetryData {
    /// Estimate memory usage of this telemetry data
    pub fn estimated_size(&self) -> usize {
        match self {
            TelemetryData::Span(span) => span.estimated_size(),
            TelemetryData::Metric(metric) => metric.estimated_size(),
            #[cfg(feature = "structured-logging")]
            TelemetryData::Log(log) => log.estimated_size(),
        }
    }
}

impl MemoryEstimator for SpanData {
    fn estimated_size(&self) -> usize {
        self.estimated_size()
    }
}

impl MemoryEstimator for MetricData {
    fn estimated_size(&self) -> usize {
        self.estimated_size()
    }
}

#[cfg(feature = "structured-logging")]
impl MemoryEstimator for LogData {
    fn estimated_size(&self) -> usize {
        self.estimated_size()
    }
}

/// Span data for batching
#[derive(Debug, Clone)]
pub struct SpanData {
    pub span_id: String,
    pub trace_id: String,
    pub parent_span_id: Option<String>,
    pub name: String,
    pub start_time: Instant,
    pub end_time: Option<Instant>,
    pub status: SpanStatus,
    pub attributes: HashMap<String, String>,
}

impl SpanData {
    pub fn new(span_id: String, trace_id: String, name: String) -> Self {
        Self {
            span_id,
            trace_id,
            parent_span_id: None,
            name,
            start_time: Instant::now(),
            end_time: None,
            status: SpanStatus::Ok,
            attributes: HashMap::new(),
        }
    }

    pub fn with_parent(mut self, parent_span_id: String) -> Self {
        self.parent_span_id = Some(parent_span_id);
        self
    }

    pub fn add_attribute(&mut self, key: String, value: String) {
        self.attributes.insert(key, value);
    }

    pub fn end(&mut self) {
        self.end_time = Some(Instant::now());
    }

    pub fn duration(&self) -> Option<Duration> {
        self.end_time.map(|end| end.duration_since(self.start_time))
    }

    fn estimated_size(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.span_id.len()
            + self.trace_id.len()
            + self.parent_span_id.as_ref().map_or(0, |s| s.len())
            + self.name.len()
            + self
                .attributes
                .iter()
                .map(|(k, v)| k.len() + v.len())
                .sum::<usize>()
    }
}

/// Metric data for batching
#[derive(Debug, Clone)]
pub struct MetricData {
    pub name: String,
    pub value: f64,
    pub labels: HashMap<String, String>,
    pub timestamp: Instant,
    pub metric_type: MetricType,
}

#[derive(Debug, Clone)]
pub enum MetricType {
    Counter,
    Histogram,
    Gauge,
}

impl MetricData {
    pub fn new(
        name: String,
        value: f64,
        labels: HashMap<String, String>,
        metric_type: MetricType,
    ) -> Self {
        Self {
            name,
            value,
            labels,
            timestamp: Instant::now(),
            metric_type,
        }
    }

    fn estimated_size(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.name.len()
            + self
                .labels
                .iter()
                .map(|(k, v)| k.len() + v.len())
                .sum::<usize>()
    }
}

/// Log data for batching
#[cfg(feature = "structured-logging")]
#[derive(Debug, Clone)]
pub struct LogData {
    pub level: LogLevel,
    pub message: String,
    pub fields: JsonValue,
    pub timestamp: Instant,
    pub trace_id: Option<String>,
    pub span_id: Option<String>,
}

#[cfg(feature = "structured-logging")]
impl LogData {
    pub fn new(level: LogLevel, message: String, fields: JsonValue) -> Self {
        Self {
            level,
            message,
            fields,
            timestamp: Instant::now(),
            trace_id: None,
            span_id: None,
        }
    }

    pub fn with_trace_context(mut self, trace_id: String, span_id: String) -> Self {
        self.trace_id = Some(trace_id);
        self.span_id = Some(span_id);
        self
    }

    fn estimated_size(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.message.len()
            + self.fields.to_string().len() // Approximate JSON size
            + self.trace_id.as_ref().map_or(0, |s| s.len())
            + self.span_id.as_ref().map_or(0, |s| s.len())
    }
}

/// Memory-efficient buffer with overflow handling
pub struct MemoryEfficientBuffer<T> {
    buffer: VecDeque<T>,
    max_size: usize,
    current_memory: AtomicUsize,
    max_memory: usize,
    dropped_count: AtomicU64,
    drop_on_overflow: bool,
}

impl<T> MemoryEfficientBuffer<T>
where
    T: Clone,
{
    pub fn new(max_size: usize, max_memory: usize, drop_on_overflow: bool) -> Self {
        Self {
            buffer: VecDeque::with_capacity(max_size.min(1000)), // Cap initial allocation
            max_size,
            current_memory: AtomicUsize::new(0),
            max_memory,
            dropped_count: AtomicU64::new(0),
            drop_on_overflow,
        }
    }

    pub fn push(&mut self, item: T) -> bool
    where
        T: MemoryEstimator,
    {
        let item_size = item.estimated_size();

        // Check memory constraints
        if self.current_memory.load(Ordering::Relaxed) + item_size > self.max_memory {
            if self.drop_on_overflow {
                self.dropped_count.fetch_add(1, Ordering::Relaxed);
                return false;
            } else {
                // Remove oldest items to make room
                while self.current_memory.load(Ordering::Relaxed) + item_size > self.max_memory
                    && !self.buffer.is_empty()
                {
                    if let Some(old_item) = self.buffer.pop_front() {
                        let old_size = old_item.estimated_size();
                        self.current_memory.fetch_sub(old_size, Ordering::Relaxed);
                    }
                }
            }
        }

        // Check size constraints
        if self.buffer.len() >= self.max_size {
            if self.drop_on_overflow {
                self.dropped_count.fetch_add(1, Ordering::Relaxed);
                return false;
            } else if let Some(old_item) = self.buffer.pop_front() {
                let old_size = old_item.estimated_size();
                self.current_memory.fetch_sub(old_size, Ordering::Relaxed);
            }
        }

        self.buffer.push_back(item);
        self.current_memory.fetch_add(item_size, Ordering::Relaxed);
        true
    }

    pub fn drain(&mut self) -> Vec<T> {
        let items: Vec<T> = self.buffer.drain(..).collect();
        self.current_memory.store(0, Ordering::Relaxed);
        items
    }

    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    pub fn dropped_count(&self) -> u64 {
        self.dropped_count.load(Ordering::Relaxed)
    }

    pub fn memory_usage(&self) -> usize {
        self.current_memory.load(Ordering::Relaxed)
    }
}

/// Trait for estimating memory usage
pub trait MemoryEstimator {
    fn estimated_size(&self) -> usize;
}

impl MemoryEstimator for TelemetryData {
    fn estimated_size(&self) -> usize {
        self.estimated_size()
    }
}

/// Batching manager for telemetry data
pub struct BatchingManager {
    config: BatchingConfig,
    span_buffer: Arc<Mutex<MemoryEfficientBuffer<SpanData>>>,
    metric_buffer: Arc<Mutex<MemoryEfficientBuffer<MetricData>>>,
    #[cfg(feature = "structured-logging")]
    log_buffer: Arc<Mutex<MemoryEfficientBuffer<LogData>>>,
    last_flush: Arc<RwLock<Instant>>,
    flush_callback: Arc<
        Mutex<Option<Box<dyn Fn(Vec<TelemetryData>) -> ObservabilityResult<()> + Send + Sync>>>,
    >,
}

impl BatchingManager {
    pub fn new(config: BatchingConfig) -> Self {
        let buffer_size = config.max_batch_size;
        let memory_per_buffer = config.max_memory_bytes / 3; // Divide among span, metric, log buffers

        Self {
            span_buffer: Arc::new(Mutex::new(MemoryEfficientBuffer::new(
                buffer_size,
                memory_per_buffer,
                config.drop_on_overflow,
            ))),
            metric_buffer: Arc::new(Mutex::new(MemoryEfficientBuffer::new(
                buffer_size,
                memory_per_buffer,
                config.drop_on_overflow,
            ))),
            #[cfg(feature = "structured-logging")]
            log_buffer: Arc::new(Mutex::new(MemoryEfficientBuffer::new(
                buffer_size,
                memory_per_buffer,
                config.drop_on_overflow,
            ))),
            last_flush: Arc::new(RwLock::new(Instant::now())),
            flush_callback: Arc::new(Mutex::new(None)),
            config,
        }
    }

    /// Set the flush callback function
    pub fn set_flush_callback<F>(&mut self, callback: F)
    where
        F: Fn(Vec<TelemetryData>) -> ObservabilityResult<()> + Send + Sync + 'static,
    {
        let mut cb = self.flush_callback.lock().unwrap();
        *cb = Some(Box::new(callback));
    }

    /// Add a span to the batch
    pub fn add_span(&self, span: SpanData) -> ObservabilityResult<()> {
        let mut buffer = self
            .span_buffer
            .lock()
            .map_err(|_| ObservabilityError::batching("Failed to acquire span buffer lock"))?;

        if !buffer.push(span) {
            return Err(ObservabilityError::buffer("Span buffer overflow"));
        }

        // Check if we should flush
        self.check_and_flush()?;
        Ok(())
    }

    /// Add a metric to the batch
    pub fn add_metric(&self, metric: MetricData) -> ObservabilityResult<()> {
        let mut buffer = self
            .metric_buffer
            .lock()
            .map_err(|_| ObservabilityError::batching("Failed to acquire metric buffer lock"))?;

        if !buffer.push(metric) {
            return Err(ObservabilityError::buffer("Metric buffer overflow"));
        }

        // Check if we should flush
        self.check_and_flush()?;
        Ok(())
    }

    /// Add a log to the batch
    #[cfg(feature = "structured-logging")]
    pub fn add_log(&self, log: LogData) -> ObservabilityResult<()> {
        let mut buffer = self
            .log_buffer
            .lock()
            .map_err(|_| ObservabilityError::batching("Failed to acquire log buffer lock"))?;

        if !buffer.push(log) {
            return Err(ObservabilityError::buffer("Log buffer overflow"));
        }

        // Check if we should flush
        self.check_and_flush()?;
        Ok(())
    }

    /// Check if buffers should be flushed and flush if necessary
    fn check_and_flush(&self) -> ObservabilityResult<()> {
        let should_flush = {
            let last_flush = self.last_flush.read().unwrap();
            let elapsed = last_flush.elapsed();

            // Check time-based flush
            if elapsed >= self.config.flush_interval {
                true
            } else {
                // Check size-based flush
                let span_len = self.span_buffer.lock().unwrap().len();
                let metric_len = self.metric_buffer.lock().unwrap().len();
                #[cfg(feature = "structured-logging")]
                let log_len = self.log_buffer.lock().unwrap().len();
                #[cfg(not(feature = "structured-logging"))]
                let log_len = 0;

                let total_items = span_len + metric_len + log_len;
                total_items >= self.config.min_batch_size
            }
        };

        if should_flush {
            self.flush_all()?;
        }

        Ok(())
    }

    /// Force flush all buffers
    pub fn flush_all(&self) -> ObservabilityResult<()> {
        let mut all_data = Vec::new();

        // Drain all buffers
        {
            let mut span_buffer = self.span_buffer.lock().unwrap();
            let spans = span_buffer.drain();
            all_data.extend(spans.into_iter().map(TelemetryData::Span));
        }

        {
            let mut metric_buffer = self.metric_buffer.lock().unwrap();
            let metrics = metric_buffer.drain();
            all_data.extend(metrics.into_iter().map(TelemetryData::Metric));
        }

        #[cfg(feature = "structured-logging")]
        {
            let mut log_buffer = self.log_buffer.lock().unwrap();
            let logs = log_buffer.drain();
            all_data.extend(logs.into_iter().map(TelemetryData::Log));
        }

        // Update last flush time
        {
            let mut last_flush = self.last_flush.write().unwrap();
            *last_flush = Instant::now();
        }

        // Call flush callback if data exists
        if !all_data.is_empty() {
            if let Some(callback) = self.flush_callback.lock().unwrap().as_ref() {
                callback(all_data)?;
            }
        }

        Ok(())
    }

    /// Get buffer statistics
    pub fn get_stats(&self) -> BatchingStats {
        let span_buffer = self.span_buffer.lock().unwrap();
        let metric_buffer = self.metric_buffer.lock().unwrap();

        #[cfg(feature = "structured-logging")]
        let log_buffer = self.log_buffer.lock().unwrap();
        #[cfg(not(feature = "structured-logging"))]
        let log_buffer_len = 0;
        #[cfg(not(feature = "structured-logging"))]
        let log_dropped = 0;
        #[cfg(not(feature = "structured-logging"))]
        let log_memory = 0;

        BatchingStats {
            span_count: span_buffer.len(),
            metric_count: metric_buffer.len(),
            #[cfg(feature = "structured-logging")]
            log_count: log_buffer.len(),
            #[cfg(not(feature = "structured-logging"))]
            log_count: log_buffer_len,
            span_dropped: span_buffer.dropped_count(),
            metric_dropped: metric_buffer.dropped_count(),
            #[cfg(feature = "structured-logging")]
            log_dropped: log_buffer.dropped_count(),
            #[cfg(not(feature = "structured-logging"))]
            log_dropped,
            memory_usage: {
                let mut total = span_buffer.memory_usage() + metric_buffer.memory_usage();
                #[cfg(feature = "structured-logging")]
                {
                    total += log_buffer.memory_usage();
                }
                #[cfg(not(feature = "structured-logging"))]
                {
                    total += log_memory;
                }
                total
            },
            last_flush: *self.last_flush.read().unwrap(),
        }
    }
}

/// Statistics about batching buffers
#[derive(Debug, Clone)]
pub struct BatchingStats {
    pub span_count: usize,
    pub metric_count: usize,
    pub log_count: usize,
    pub span_dropped: u64,
    pub metric_dropped: u64,
    pub log_dropped: u64,
    pub memory_usage: usize,
    pub last_flush: Instant,
}

impl BatchingStats {
    pub fn total_items(&self) -> usize {
        self.span_count + self.metric_count + self.log_count
    }

    pub fn total_dropped(&self) -> u64 {
        self.span_dropped + self.metric_dropped + self.log_dropped
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_efficient_buffer() {
        let mut buffer = MemoryEfficientBuffer::new(3, 1000, true);

        let data1 = MetricData::new(
            "test1".to_string(),
            1.0,
            HashMap::new(),
            MetricType::Counter,
        );
        let data2 = MetricData::new(
            "test2".to_string(),
            2.0,
            HashMap::new(),
            MetricType::Counter,
        );
        let data3 = MetricData::new(
            "test3".to_string(),
            3.0,
            HashMap::new(),
            MetricType::Counter,
        );
        let data4 = MetricData::new(
            "test4".to_string(),
            4.0,
            HashMap::new(),
            MetricType::Counter,
        );

        assert!(buffer.push(data1));
        assert!(buffer.push(data2));
        assert!(buffer.push(data3));

        // Should drop when buffer is full
        assert!(!buffer.push(data4));
        assert_eq!(buffer.dropped_count(), 1);
        assert_eq!(buffer.len(), 3);
    }

    #[test]
    fn test_batching_manager() {
        let config = BatchingConfig {
            max_batch_size: 5,
            min_batch_size: 10, // Set high to avoid auto-flush
            flush_interval: Duration::from_hours(1), // Set very high to avoid time-based flush
            ..Default::default()
        };

        let manager = BatchingManager::new(config);

        // Test that we can get stats without adding anything
        let stats = manager.get_stats();
        assert_eq!(stats.metric_count, 0);
        assert_eq!(stats.total_items(), 0);

        // Test stats calculation methods
        assert_eq!(stats.total_dropped(), 0);
    }

    #[test]
    fn test_span_data_creation() {
        let span = SpanData::new(
            "span1".to_string(),
            "trace1".to_string(),
            "test_span".to_string(),
        )
        .with_parent("parent1".to_string());

        assert_eq!(span.span_id, "span1");
        assert_eq!(span.trace_id, "trace1");
        assert_eq!(span.parent_span_id, Some("parent1".to_string()));
        assert_eq!(span.name, "test_span");
    }

    #[test]
    fn test_metric_data_creation() {
        let mut labels = HashMap::new();
        labels.insert("env".to_string(), "test".to_string());

        let metric = MetricData::new(
            "test_metric".to_string(),
            42.0,
            labels.clone(),
            MetricType::Gauge,
        );

        assert_eq!(metric.name, "test_metric");
        assert_eq!(metric.value, 42.0);
        assert_eq!(metric.labels, labels);
        assert!(matches!(metric.metric_type, MetricType::Gauge));
    }

    #[test]
    fn test_batching_config() {
        let config = BatchingConfig::default();
        assert_eq!(config.max_batch_size, 100);
        assert_eq!(config.flush_interval, Duration::from_secs(5));
        assert_eq!(config.max_memory_bytes, 1024 * 1024);
        assert!(config.drop_on_overflow);
        assert_eq!(config.min_batch_size, 10);
    }

    #[test]
    fn test_telemetry_data_size_estimation() {
        let span = SpanData::new("test".to_string(), "trace".to_string(), "span".to_string());
        let span_data = TelemetryData::Span(span);
        assert!(span_data.estimated_size() > 0);

        let metric = MetricData::new("test".to_string(), 1.0, HashMap::new(), MetricType::Counter);
        let metric_data = TelemetryData::Metric(metric);
        assert!(metric_data.estimated_size() > 0);
    }
}
