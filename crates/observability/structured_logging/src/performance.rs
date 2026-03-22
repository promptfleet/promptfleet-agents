//! Performance optimization features for structured logging

use std::collections::HashMap;
use std::io::{self, Write};
use std::sync::{Arc, Mutex, RwLock};
use web_time::{Duration, Instant};

#[cfg(feature = "string-interner")]
use string_interner_crate::StringInterner;

use crate::error::{Result, StructuredLoggingError};
use crate::extension::PerformanceConfig;
use observability_core::domain::TraceContext;
use observability_core::{traits::LogLevel, LogEntry, TransportPort};
use serde_json::Value;

/// Performance statistics for monitoring optimization impact
#[derive(Debug, Clone, Default)]
pub struct PerformanceStats {
    /// Total entries processed
    pub entries_processed: u64,

    /// Total time spent in fast paths
    pub fast_path_time: Duration,

    /// Total time spent in standard paths
    pub standard_path_time: Duration,

    /// Number of string interning hits
    pub string_interner_hits: u64,

    /// Number of string interning misses
    pub string_interner_misses: u64,

    /// Buffer pool hits
    pub buffer_pool_hits: u64,

    /// Buffer pool misses (new allocations)
    pub buffer_pool_misses: u64,

    /// Total bytes written
    pub bytes_written: u64,

    /// Average entry processing time
    pub avg_processing_time_ns: u64,
}

impl PerformanceStats {
    /// Calculate string interning hit ratio
    pub fn string_interner_hit_ratio(&self) -> f64 {
        let total = self.string_interner_hits + self.string_interner_misses;
        if total > 0 {
            self.string_interner_hits as f64 / total as f64
        } else {
            0.0
        }
    }

    /// Calculate buffer pool hit ratio
    pub fn buffer_pool_hit_ratio(&self) -> f64 {
        let total = self.buffer_pool_hits + self.buffer_pool_misses;
        if total > 0 {
            self.buffer_pool_hits as f64 / total as f64
        } else {
            0.0
        }
    }

    /// Calculate fast path usage ratio
    pub fn fast_path_ratio(&self) -> f64 {
        let total = self.fast_path_time + self.standard_path_time;
        if total.as_nanos() > 0 {
            self.fast_path_time.as_nanos() as f64 / total.as_nanos() as f64
        } else {
            0.0
        }
    }
}

/// String interner for common log field values
#[cfg(feature = "string-interner")]
pub struct StringInterningProcessor {
    interner: Arc<RwLock<StringInterner<string_interner_crate::backend::BucketBackend<usize>>>>,
    stats: Arc<Mutex<PerformanceStats>>,
}

#[cfg(feature = "string-interner")]
impl StringInterningProcessor {
    pub fn new(capacity: usize) -> Result<Self> {
        Ok(Self {
            interner: Arc::new(RwLock::new(StringInterner::with_capacity(capacity))),
            stats: Arc::new(Mutex::new(PerformanceStats::default())),
        })
    }

    pub fn intern_string(&self, value: &str) -> Result<String> {
        // Try read lock first for lookup
        if let Ok(interner) = self.interner.read() {
            if let Some(interned) = interner.get(value) {
                // Hit - update stats
                if let Ok(mut stats) = self.stats.lock() {
                    stats.string_interner_hits += 1;
                }
                return Ok(interned.to_string());
            }
        }

        // Miss - need write lock to insert
        if let Ok(mut interner) = self.interner.write() {
            let interned = interner.get_or_intern(value);
            if let Ok(mut stats) = self.stats.lock() {
                stats.string_interner_misses += 1;
            }
            Ok(interned.to_string())
        } else {
            Err(StructuredLoggingError::string_interning(
                "Failed to acquire write lock on string interner",
            ))
        }
    }

    pub fn process_entry(&self, mut entry: LogEntry) -> Result<LogEntry> {
        // Intern common field values
        entry = self.intern_entry_fields(entry)?;
        Ok(entry)
    }

    fn intern_entry_fields(&self, mut entry: LogEntry) -> Result<LogEntry> {
        // Intern common field values in the fields object
        if let serde_json::Value::Object(ref mut fields_map) = entry.fields {
            if let Some(Value::String(model)) = fields_map.get("model") {
                let interned = self.intern_string(model)?;
                fields_map.insert("model".to_string(), Value::String(interned));
            }
            if let Some(Value::String(status)) = fields_map.get("status") {
                let interned = self.intern_string(status)?;
                fields_map.insert("status".to_string(), Value::String(interned));
            }
            if let Some(Value::String(operation)) = fields_map.get("operation") {
                let interned = self.intern_string(operation)?;
                fields_map.insert("operation".to_string(), Value::String(interned));
            }
            if let Some(Value::String(component)) = fields_map.get("component") {
                let interned = self.intern_string(component)?;
                fields_map.insert("component".to_string(), Value::String(interned));
            }
        }

        Ok(entry)
    }
}

/// Buffer pool for reusing formatting buffers
pub struct BufferPool {
    buffers: Arc<Mutex<Vec<Vec<u8>>>>,
    capacity: usize,
    buffer_size: usize,
    stats: Arc<Mutex<PerformanceStats>>,
}

impl BufferPool {
    pub fn new(pool_size: usize, buffer_capacity: usize) -> Self {
        let mut buffers = Vec::with_capacity(pool_size);
        for _ in 0..pool_size {
            buffers.push(Vec::with_capacity(buffer_capacity));
        }

        Self {
            buffers: Arc::new(Mutex::new(buffers)),
            capacity: pool_size,
            buffer_size: buffer_capacity,
            stats: Arc::new(Mutex::new(PerformanceStats::default())),
        }
    }

    pub fn get_buffer(&self) -> Result<Vec<u8>> {
        if let Ok(mut buffers) = self.buffers.lock() {
            if let Some(mut buffer) = buffers.pop() {
                buffer.clear();
                if let Ok(mut stats) = self.stats.lock() {
                    stats.buffer_pool_hits += 1;
                }
                Ok(buffer)
            } else {
                // Pool exhausted, create new buffer
                if let Ok(mut stats) = self.stats.lock() {
                    stats.buffer_pool_misses += 1;
                }
                Ok(Vec::with_capacity(self.buffer_size))
            }
        } else {
            Err(StructuredLoggingError::buffer_pool(
                "Failed to acquire buffer pool lock",
            ))
        }
    }

    pub fn return_buffer(&self, buffer: Vec<u8>) -> Result<()> {
        if let Ok(mut buffers) = self.buffers.lock() {
            if buffers.len() < self.capacity {
                buffers.push(buffer);
            }
            // If pool is full, just drop the buffer
        }
        Ok(())
    }
}

/// Fast path logger for hot code paths with minimal allocations
pub struct FastPathLogger {
    buffer_pool: BufferPool,
    #[cfg(feature = "string-interner")]
    string_interner: Option<StringInterningProcessor>,
    stats: Arc<Mutex<PerformanceStats>>,
}

impl FastPathLogger {
    pub fn new(config: &PerformanceConfig) -> Result<Self> {
        let buffer_pool = BufferPool::new(config.buffer_pool_size, config.buffer_capacity);

        #[cfg(feature = "string-interner")]
        let string_interner = if config.enable_string_interning {
            Some(StringInterningProcessor::new(
                config.string_interner_capacity,
            )?)
        } else {
            None
        };

        Ok(Self {
            buffer_pool,
            #[cfg(feature = "string-interner")]
            string_interner,
            stats: Arc::new(Mutex::new(PerformanceStats::default())),
        })
    }

    /// Fast path for LLM request logging with minimal allocations
    pub fn log_llm_request_fast(
        &self,
        model: &str,
        duration_ms: u64,
        tokens: u32,
        status: &str,
    ) -> Result<Vec<u8>> {
        let start = Instant::now();
        let mut buffer = self.buffer_pool.get_buffer()?;

        // Manual JSON construction to avoid serde overhead
        buffer.extend_from_slice(b"{\"timestamp\":\"");
        self.append_timestamp(&mut buffer);
        buffer.extend_from_slice(b"\",\"level\":\"INFO\",\"message\":\"LLM request completed\",\"component\":\"openai_client_wasm\",\"model\":\"");
        buffer.extend_from_slice(model.as_bytes());
        buffer.extend_from_slice(b"\",\"duration_ms\":");
        buffer.extend_from_slice(duration_ms.to_string().as_bytes());
        buffer.extend_from_slice(b",\"tokens\":");
        buffer.extend_from_slice(tokens.to_string().as_bytes());
        buffer.extend_from_slice(b",\"status\":\"");
        buffer.extend_from_slice(status.as_bytes());
        buffer.extend_from_slice(b"\",\"operation\":\"llm_request\"}");

        let elapsed = start.elapsed();
        if let Ok(mut stats) = self.stats.lock() {
            stats.entries_processed += 1;
            stats.fast_path_time += elapsed;
            stats.bytes_written += buffer.len() as u64;
        }

        self.buffer_pool.return_buffer(buffer.clone())?;
        Ok(buffer)
    }

    /// Fast path for A2A message logging
    pub fn log_a2a_message_fast(
        &self,
        message_type: &str,
        from_agent: &str,
        to_agent: &str,
        duration_ms: Option<u64>,
    ) -> Result<Vec<u8>> {
        let start = Instant::now();
        let mut buffer = self.buffer_pool.get_buffer()?;

        buffer.extend_from_slice(b"{\"timestamp\":\"");
        self.append_timestamp(&mut buffer);
        buffer.extend_from_slice(b"\",\"level\":\"INFO\",\"message\":\"A2A message processed\",\"component\":\"a2a_jsonrpc_server\",\"message_type\":\"");
        buffer.extend_from_slice(message_type.as_bytes());
        buffer.extend_from_slice(b"\",\"from_agent\":\"");
        buffer.extend_from_slice(from_agent.as_bytes());
        buffer.extend_from_slice(b"\",\"to_agent\":\"");
        buffer.extend_from_slice(to_agent.as_bytes());
        buffer.extend_from_slice(b"\",\"protocol\":\"json-rpc-2.0\"");

        if let Some(duration) = duration_ms {
            buffer.extend_from_slice(b",\"duration_ms\":");
            buffer.extend_from_slice(duration.to_string().as_bytes());
        }

        buffer.extend_from_slice(b",\"operation\":\"a2a_message\"}");

        let elapsed = start.elapsed();
        if let Ok(mut stats) = self.stats.lock() {
            stats.entries_processed += 1;
            stats.fast_path_time += elapsed;
            stats.bytes_written += buffer.len() as u64;
        }

        self.buffer_pool.return_buffer(buffer.clone())?;
        Ok(buffer)
    }

    fn append_timestamp(&self, buffer: &mut Vec<u8>) {
        // Use WASM-compatible time
        let timestamp = web_time::SystemTime::now()
            .duration_since(web_time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        buffer.extend_from_slice(timestamp.to_string().as_bytes());
    }
}

/// Optimized WASM stdout adapter with performance enhancements
pub struct OptimizedWasmStdoutAdapter {
    fast_path_logger: FastPathLogger,
    config: PerformanceConfig,
    stats: Arc<Mutex<PerformanceStats>>,
}

impl OptimizedWasmStdoutAdapter {
    pub fn new(config: PerformanceConfig) -> Result<Self> {
        let fast_path_logger = FastPathLogger::new(&config)?;

        Ok(Self {
            fast_path_logger,
            config,
            stats: Arc::new(Mutex::new(PerformanceStats::default())),
        })
    }

    /// Write log entry using fast path if possible
    pub fn write_entry_optimized(&self, entry: &LogEntry) -> Result<()> {
        let start = Instant::now();

        // Try fast path for common operations
        if self.config.enable_fast_paths {
            if let serde_json::Value::Object(ref fields_map) = entry.fields {
                if let Some(Value::String(operation)) = fields_map.get("operation") {
                    match operation.as_str() {
                        "llm_request" => {
                            if let Some(json_bytes) = self.try_llm_fast_path(entry)? {
                                return self.write_bytes(&json_bytes);
                            }
                        }
                        "a2a_message" => {
                            if let Some(json_bytes) = self.try_a2a_fast_path(entry)? {
                                return self.write_bytes(&json_bytes);
                            }
                        }
                        _ => {} // Fall through to standard path
                    }
                }
            }
        }

        // Standard path using serde
        let json_string = serde_json::to_string(entry).map_err(StructuredLoggingError::from)?;

        let elapsed = start.elapsed();
        if let Ok(mut stats) = self.stats.lock() {
            stats.entries_processed += 1;
            stats.standard_path_time += elapsed;
            stats.bytes_written += json_string.len() as u64;
        }

        self.write_bytes(json_string.as_bytes())
    }

    fn try_llm_fast_path(&self, entry: &LogEntry) -> Result<Option<Vec<u8>>> {
        let fields = if let serde_json::Value::Object(ref fields_map) = entry.fields {
            fields_map
        } else {
            return Err(StructuredLoggingError::fast_path(
                "LLM entry missing fields",
            ));
        };

        let model = fields
            .get("model")
            .and_then(|v| v.as_str())
            .ok_or_else(|| StructuredLoggingError::fast_path("Missing model field"))?;

        let duration_ms = fields
            .get("duration_ms")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| StructuredLoggingError::fast_path("Missing duration_ms field"))?;

        let tokens = fields
            .get("tokens")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| StructuredLoggingError::fast_path("Missing tokens field"))?;

        let status = fields
            .get("status")
            .and_then(|v| v.as_str())
            .ok_or_else(|| StructuredLoggingError::fast_path("Missing status field"))?;

        let json_bytes = self.fast_path_logger.log_llm_request_fast(
            model,
            duration_ms,
            tokens as u32,
            status,
        )?;

        Ok(Some(json_bytes))
    }

    fn try_a2a_fast_path(&self, entry: &LogEntry) -> Result<Option<Vec<u8>>> {
        let fields = if let serde_json::Value::Object(ref fields_map) = entry.fields {
            fields_map
        } else {
            return Err(StructuredLoggingError::fast_path(
                "A2A entry missing fields",
            ));
        };

        let message_type = fields
            .get("message_type")
            .and_then(|v| v.as_str())
            .ok_or_else(|| StructuredLoggingError::fast_path("Missing message_type field"))?;

        let from_agent = fields
            .get("from_agent")
            .and_then(|v| v.as_str())
            .ok_or_else(|| StructuredLoggingError::fast_path("Missing from_agent field"))?;

        let to_agent = fields
            .get("to_agent")
            .and_then(|v| v.as_str())
            .ok_or_else(|| StructuredLoggingError::fast_path("Missing to_agent field"))?;

        let duration_ms = fields.get("duration_ms").and_then(|v| v.as_u64());

        let json_bytes = self.fast_path_logger.log_a2a_message_fast(
            message_type,
            from_agent,
            to_agent,
            duration_ms,
        )?;

        Ok(Some(json_bytes))
    }

    fn write_bytes(&self, bytes: &[u8]) -> Result<()> {
        io::stdout().write_all(bytes)?;
        io::stdout().write_all(b"\n")?;
        io::stdout().flush()?;
        Ok(())
    }

    pub fn get_stats(&self) -> Result<PerformanceStats> {
        if let Ok(stats) = self.stats.lock() {
            Ok(stats.clone())
        } else {
            Err(StructuredLoggingError::performance(
                "Failed to acquire stats lock",
            ))
        }
    }
}

impl TransportPort for OptimizedWasmStdoutAdapter {
    fn transport(&self, entry: &LogEntry) -> observability_core::ObservabilityResult<()> {
        self.write_entry_optimized(entry)
            .map_err(|e| observability_core::ObservabilityError::transport(e.to_string()))
    }
}

/// Performance manager that coordinates all optimization features
pub struct PerformanceManager {
    #[cfg(feature = "string-interner")]
    string_interner: Option<StringInterningProcessor>,
    optimized_adapter: OptimizedWasmStdoutAdapter,
    config: PerformanceConfig,
}

impl PerformanceManager {
    pub fn new(config: &PerformanceConfig) -> Result<Self> {
        #[cfg(feature = "string-interner")]
        let string_interner = if config.enable_string_interning {
            Some(StringInterningProcessor::new(
                config.string_interner_capacity,
            )?)
        } else {
            None
        };

        let optimized_adapter = OptimizedWasmStdoutAdapter::new(config.clone())?;

        Ok(Self {
            #[cfg(feature = "string-interner")]
            string_interner,
            optimized_adapter,
            config: config.clone(),
        })
    }

    pub fn process_entry(&self, mut entry: LogEntry) -> Result<LogEntry> {
        // Apply string interning if enabled
        #[cfg(feature = "string-interner")]
        if let Some(ref interner) = self.string_interner {
            entry = interner.process_entry(entry)?;
        }

        Ok(entry)
    }

    pub fn get_stats(&self) -> PerformanceStats {
        let mut combined_stats = PerformanceStats::default();

        // Combine stats from optimized adapter
        if let Ok(adapter_stats) = self.optimized_adapter.get_stats() {
            combined_stats.entries_processed += adapter_stats.entries_processed;
            combined_stats.fast_path_time += adapter_stats.fast_path_time;
            combined_stats.standard_path_time += adapter_stats.standard_path_time;
            combined_stats.bytes_written += adapter_stats.bytes_written;
        }

        // Add string interner stats if available
        #[cfg(feature = "string-interner")]
        if let Some(ref interner) = self.string_interner {
            if let Ok(interner_stats) = interner.stats.lock() {
                combined_stats.string_interner_hits += interner_stats.string_interner_hits;
                combined_stats.string_interner_misses += interner_stats.string_interner_misses;
            }
        }

        combined_stats
    }

    pub fn reset_stats(&self) -> Result<()> {
        // Reset optimized adapter stats
        if let Ok(mut stats) = self.optimized_adapter.stats.lock() {
            *stats = PerformanceStats::default();
        }

        // Reset string interner stats if available
        #[cfg(feature = "string-interner")]
        if let Some(ref interner) = self.string_interner {
            if let Ok(mut stats) = interner.stats.lock() {
                *stats = PerformanceStats::default();
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_performance_stats_calculations() {
        let mut stats = PerformanceStats::default();
        stats.string_interner_hits = 80;
        stats.string_interner_misses = 20;
        stats.buffer_pool_hits = 90;
        stats.buffer_pool_misses = 10;

        assert_eq!(stats.string_interner_hit_ratio(), 0.8);
        assert_eq!(stats.buffer_pool_hit_ratio(), 0.9);
    }

    #[test]
    fn test_buffer_pool() {
        let pool = BufferPool::new(2, 1024);

        let buffer1 = pool.get_buffer().unwrap();
        let buffer2 = pool.get_buffer().unwrap();

        assert_eq!(buffer1.capacity(), 1024);
        assert_eq!(buffer2.capacity(), 1024);

        pool.return_buffer(buffer1).unwrap();
        pool.return_buffer(buffer2).unwrap();
    }

    #[cfg(feature = "string-interner")]
    #[test]
    fn test_string_interning() {
        let processor = StringInterningProcessor::new(100).unwrap();

        let result1 = processor.intern_string("test-value").unwrap();
        let result2 = processor.intern_string("test-value").unwrap();

        assert_eq!(result1, result2);
        assert_eq!(result1, "test-value");
    }

    #[test]
    fn test_fast_path_logger() {
        let config = PerformanceConfig::default();
        let logger = FastPathLogger::new(&config).unwrap();

        let result = logger.log_llm_request_fast("gpt-4", 250, 1500, "success");
        assert!(result.is_ok());

        let json_bytes = result.unwrap();
        let json_str = String::from_utf8(json_bytes).unwrap();
        assert!(json_str.contains("gpt-4"));
        assert!(json_str.contains("250"));
        assert!(json_str.contains("1500"));
        assert!(json_str.contains("success"));
    }

    #[test]
    fn test_optimized_adapter_creation() {
        let config = PerformanceConfig::default();
        let adapter = OptimizedWasmStdoutAdapter::new(config);
        assert!(adapter.is_ok());
    }

    #[test]
    fn test_performance_manager_creation() {
        let config = PerformanceConfig::default();
        let manager = PerformanceManager::new(&config);
        assert!(manager.is_ok());
    }
}
