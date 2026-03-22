//! Correlation enhancement features for structured logging

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use web_time::{Duration, Instant};

#[cfg(feature = "uuid")]
use uuid::Uuid;

use crate::error::{Result, StructuredLoggingError};
use crate::extension::CorrelationConfig;
use observability_core::domain::TraceContext;
use observability_core::LogEntry;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Enhanced trace context with additional correlation capabilities
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnhancedTraceContext {
    /// Base trace context from observability_core
    pub base: TraceContext,

    /// W3C baggage for cross-cutting concerns
    pub baggage: HashMap<String, String>,

    /// Scoped context stack for nested operations
    pub scoped_contexts: Vec<ScopedContext>,

    /// Correlation metadata
    pub correlation_metadata: HashMap<String, Value>,
}

/// Scoped context for nested operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopedContext {
    /// Unique identifier for this scope
    pub scope_id: String,

    /// Scope name (e.g., "llm_request", "template_render")
    pub scope_name: String,

    /// When this scope started (as timestamp string for WASM compatibility)
    pub start_time: String,

    /// Parent scope ID (if nested)
    pub parent_scope_id: Option<String>,

    /// Scope-specific metadata
    pub metadata: HashMap<String, Value>,
}

impl EnhancedTraceContext {
    /// Create new enhanced trace context
    pub fn new(base: TraceContext) -> Self {
        Self {
            base,
            baggage: HashMap::new(),
            scoped_contexts: Vec::new(),
            correlation_metadata: HashMap::new(),
        }
    }

    /// Create from W3C trace headers
    pub fn from_w3c_headers(trace_parent: &str, _trace_state: Option<&str>) -> Result<Self> {
        // Parse traceparent header: 00-{trace_id}-{span_id}-{flags}
        let parts: Vec<&str> = trace_parent.split('-').collect();
        if parts.len() != 4 {
            return Err(StructuredLoggingError::correlation(
                "Invalid traceparent header format",
            ));
        }

        let trace_id = parts[1].to_string();
        let span_id = parts[2].to_string();

        let base = TraceContext {
            trace_id,
            span_id,
            parent_span_id: None,
        };

        Ok(Self::new(base))
    }

    /// Add baggage item
    pub fn add_baggage(&mut self, key: String, value: String) -> Result<()> {
        if self.baggage.len() >= 32 {
            return Err(StructuredLoggingError::baggage(
                "Baggage limit exceeded (max 32 items)",
            ));
        }

        let total_size: usize = self.baggage.iter().map(|(k, v)| k.len() + v.len()).sum();

        if total_size + key.len() + value.len() > 8192 {
            return Err(StructuredLoggingError::baggage(
                "Baggage size limit exceeded (max 8KB)",
            ));
        }

        self.baggage.insert(key, value);
        Ok(())
    }

    /// Get baggage item
    pub fn get_baggage(&self, key: &str) -> Option<&String> {
        self.baggage.get(key)
    }

    /// Remove baggage item
    pub fn remove_baggage(&mut self, key: &str) -> Option<String> {
        self.baggage.remove(key)
    }

    /// Push new scoped context
    pub fn push_scope(
        &mut self,
        scope_name: String,
        metadata: HashMap<String, Value>,
    ) -> Result<String> {
        if self.scoped_contexts.len() >= 16 {
            return Err(StructuredLoggingError::scoped_context(
                "Context nesting depth limit exceeded (max 16)",
            ));
        }

        #[cfg(feature = "uuid")]
        let scope_id = Uuid::new_v4().to_string();
        #[cfg(not(feature = "uuid"))]
        let scope_id = format!("scope_{}", self.scoped_contexts.len());

        let parent_scope_id = self.scoped_contexts.last().map(|ctx| ctx.scope_id.clone());

        // Use a simple timestamp string for WASM compatibility
        let start_time = web_time::SystemTime::now()
            .duration_since(web_time::UNIX_EPOCH)
            .map(|d| d.as_secs().to_string())
            .unwrap_or_else(|_| "0".to_string());

        let scope = ScopedContext {
            scope_id: scope_id.clone(),
            scope_name,
            start_time,
            parent_scope_id,
            metadata,
        };

        self.scoped_contexts.push(scope);
        Ok(scope_id)
    }

    /// Pop current scoped context
    pub fn pop_scope(&mut self) -> Option<ScopedContext> {
        self.scoped_contexts.pop()
    }

    /// Get current scope
    pub fn current_scope(&self) -> Option<&ScopedContext> {
        self.scoped_contexts.last()
    }

    /// Add correlation metadata
    pub fn add_correlation_metadata(&mut self, key: String, value: Value) {
        self.correlation_metadata.insert(key, value);
    }

    /// Get correlation metadata
    pub fn get_correlation_metadata(&self) -> &HashMap<String, Value> {
        &self.correlation_metadata
    }

    /// Convert to W3C baggage header format
    pub fn to_baggage_header(&self) -> String {
        self.baggage
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<_>>()
            .join(",")
    }

    /// Parse from W3C baggage header
    pub fn from_baggage_header(&mut self, baggage_header: &str) -> Result<()> {
        self.baggage.clear();

        for item in baggage_header.split(',') {
            let item = item.trim();
            if let Some((key, value)) = item.split_once('=') {
                self.add_baggage(key.trim().to_string(), value.trim().to_string())?;
            }
        }

        Ok(())
    }
}

/// W3C baggage support manager
pub struct BaggageManager {
    config: CorrelationConfig,
}

impl BaggageManager {
    pub fn new(config: CorrelationConfig) -> Self {
        Self { config }
    }

    /// Validate baggage item
    pub fn validate_baggage_item(&self, key: &str, value: &str) -> Result<()> {
        if key.is_empty() || key.len() > 256 {
            return Err(StructuredLoggingError::baggage(
                "Baggage key must be 1-256 characters",
            ));
        }

        if value.len() > 1024 {
            return Err(StructuredLoggingError::baggage(
                "Baggage value must be <= 1024 characters",
            ));
        }

        // Check for invalid characters
        if !key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return Err(StructuredLoggingError::baggage(
                "Baggage key contains invalid characters",
            ));
        }

        Ok(())
    }

    /// Add system baggage for agent operations
    pub fn add_system_baggage(
        &self,
        ctx: &mut EnhancedTraceContext,
        operation: &str,
    ) -> Result<()> {
        if !self.config.enable_baggage {
            return Ok(());
        }

        ctx.add_baggage("promptfleet.operation".to_string(), operation.to_string())?;
        ctx.add_baggage(
            "promptfleet.agent".to_string(),
            "spinkube-agent".to_string(),
        )?;

        #[cfg(feature = "uuid")]
        {
            ctx.add_baggage(
                "promptfleet.correlation_id".to_string(),
                Uuid::new_v4().to_string(),
            )?;
        }

        Ok(())
    }
}

/// Scoped context manager for nested operations
pub struct ScopedContextManager {
    config: CorrelationConfig,
    active_contexts: Arc<RwLock<HashMap<String, EnhancedTraceContext>>>,
}

impl ScopedContextManager {
    pub fn new(config: CorrelationConfig) -> Self {
        Self {
            config,
            active_contexts: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Enter a new scope
    pub fn enter_scope(
        &self,
        trace_id: &str,
        scope_name: String,
        metadata: HashMap<String, Value>,
    ) -> Result<String> {
        if !self.config.enable_scoped_context {
            return Err(StructuredLoggingError::feature_not_enabled(
                "scoped-context",
            ));
        }

        let scope_id = if let Ok(mut contexts) = self.active_contexts.write() {
            if let Some(ctx) = contexts.get_mut(trace_id) {
                ctx.push_scope(scope_name, metadata)?
            } else {
                return Err(StructuredLoggingError::scoped_context(
                    "No active trace context found",
                ));
            }
        } else {
            return Err(StructuredLoggingError::scoped_context(
                "Failed to acquire context lock",
            ));
        };

        Ok(scope_id)
    }

    /// Exit current scope
    pub fn exit_scope(&self, trace_id: &str) -> Result<Option<ScopedContext>> {
        if !self.config.enable_scoped_context {
            return Ok(None);
        }

        if let Ok(mut contexts) = self.active_contexts.write() {
            if let Some(ctx) = contexts.get_mut(trace_id) {
                Ok(ctx.pop_scope())
            } else {
                Err(StructuredLoggingError::scoped_context(
                    "No active trace context found",
                ))
            }
        } else {
            Err(StructuredLoggingError::scoped_context(
                "Failed to acquire context lock",
            ))
        }
    }

    /// Get current scope
    pub fn current_scope(&self, trace_id: &str) -> Result<Option<ScopedContext>> {
        if let Ok(contexts) = self.active_contexts.read() {
            if let Some(ctx) = contexts.get(trace_id) {
                Ok(ctx.current_scope().cloned())
            } else {
                Ok(None)
            }
        } else {
            Err(StructuredLoggingError::scoped_context(
                "Failed to acquire context lock",
            ))
        }
    }

    /// Register new trace context
    pub fn register_context(&self, ctx: EnhancedTraceContext) -> Result<()> {
        let trace_id = ctx.base.trace_id.clone();

        if let Ok(mut contexts) = self.active_contexts.write() {
            contexts.insert(trace_id, ctx);
            Ok(())
        } else {
            Err(StructuredLoggingError::scoped_context(
                "Failed to acquire context lock",
            ))
        }
    }

    /// Unregister trace context
    pub fn unregister_context(&self, trace_id: &str) -> Result<()> {
        if let Ok(mut contexts) = self.active_contexts.write() {
            contexts.remove(trace_id);
            Ok(())
        } else {
            Err(StructuredLoggingError::scoped_context(
                "Failed to acquire context lock",
            ))
        }
    }
}

/// W3C baggage support utilities
pub struct W3CBaggageSupport;

impl W3CBaggageSupport {
    /// Extract baggage from HTTP headers
    pub fn extract_from_headers(
        headers: &HashMap<String, String>,
    ) -> Result<HashMap<String, String>> {
        let mut baggage = HashMap::new();

        if let Some(baggage_header) = headers.get("baggage") {
            for item in baggage_header.split(',') {
                let item = item.trim();
                if let Some((key, value)) = item.split_once('=') {
                    let key = key.trim().to_string();
                    let value = value.trim().to_string();

                    // Basic validation
                    if !key.is_empty() && !value.is_empty() {
                        baggage.insert(key, value);
                    }
                }
            }
        }

        Ok(baggage)
    }

    /// Inject baggage into HTTP headers
    pub fn inject_into_headers(
        baggage: &HashMap<String, String>,
        headers: &mut HashMap<String, String>,
    ) {
        if !baggage.is_empty() {
            let baggage_header = baggage
                .iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect::<Vec<_>>()
                .join(",");

            headers.insert("baggage".to_string(), baggage_header);
        }
    }
}

/// Correlation processor for log entries
pub struct CorrelationProcessor {
    baggage_manager: BaggageManager,
    scoped_context_manager: ScopedContextManager,
    config: CorrelationConfig,
}

impl CorrelationProcessor {
    pub fn new(config: CorrelationConfig) -> Self {
        Self {
            baggage_manager: BaggageManager::new(config.clone()),
            scoped_context_manager: ScopedContextManager::new(config.clone()),
            config,
        }
    }

    /// Process log entry to add correlation information
    pub fn process_entry(&self, mut entry: LogEntry) -> Result<LogEntry> {
        // Add correlation fields if trace context is present
        if let Some(trace_ctx) = &entry.trace_context {
            // Add baggage information
            if self.config.enable_baggage {
                if let Ok(enhanced_ctx) = self.get_enhanced_context(&trace_ctx.trace_id) {
                    if !enhanced_ctx.baggage.is_empty() {
                        // Add baggage to the log entry fields
                        if let serde_json::Value::Object(ref mut map) = entry.fields {
                            map.insert(
                                "baggage".to_string(),
                                Value::Object(
                                    enhanced_ctx
                                        .baggage
                                        .iter()
                                        .map(|(k, v)| (k.clone(), Value::String(v.clone())))
                                        .collect(),
                                ),
                            );
                        }
                    }

                    // Add current scope information
                    if let Some(current_scope) = enhanced_ctx.current_scope() {
                        if let serde_json::Value::Object(ref mut map) = entry.fields {
                            map.insert(
                                "scope".to_string(),
                                Value::String(current_scope.scope_name.clone()),
                            );
                            map.insert(
                                "scope_id".to_string(),
                                Value::String(current_scope.scope_id.clone()),
                            );

                            if let Some(parent_id) = &current_scope.parent_scope_id {
                                map.insert(
                                    "parent_scope_id".to_string(),
                                    Value::String(parent_id.clone()),
                                );
                            }
                        }
                    }

                    // Add correlation metadata
                    if let serde_json::Value::Object(ref mut map) = entry.fields {
                        for (key, value) in enhanced_ctx.correlation_metadata.iter() {
                            map.insert(format!("correlation.{}", key), value.clone());
                        }
                    }
                }
            }
        }

        Ok(entry)
    }

    fn get_enhanced_context(&self, trace_id: &str) -> Result<EnhancedTraceContext> {
        if let Ok(contexts) = self.scoped_context_manager.active_contexts.read() {
            if let Some(ctx) = contexts.get(trace_id) {
                Ok(ctx.clone())
            } else {
                Err(StructuredLoggingError::correlation(
                    "Enhanced trace context not found",
                ))
            }
        } else {
            Err(StructuredLoggingError::correlation(
                "Failed to acquire context lock",
            ))
        }
    }
}

/// Correlation manager that coordinates all correlation features
pub struct CorrelationManager {
    processor: CorrelationProcessor,
    config: CorrelationConfig,
}

impl CorrelationManager {
    pub fn new(config: &CorrelationConfig) -> Result<Self> {
        Ok(Self {
            processor: CorrelationProcessor::new(config.clone()),
            config: config.clone(),
        })
    }

    pub fn process_entry(&self, entry: LogEntry) -> Result<LogEntry> {
        self.processor.process_entry(entry)
    }

    pub fn baggage_manager(&self) -> &BaggageManager {
        &self.processor.baggage_manager
    }

    pub fn scoped_context_manager(&self) -> &ScopedContextManager {
        &self.processor.scoped_context_manager
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use observability_core::traits::LogLevel;

    #[test]
    fn test_enhanced_trace_context_creation() {
        let base_ctx = TraceContext {
            trace_id: "test-trace-id".to_string(),
            span_id: "test-span-id".to_string(),
            parent_span_id: None,
        };

        let enhanced_ctx = EnhancedTraceContext::new(base_ctx);
        assert_eq!(enhanced_ctx.base.trace_id, "test-trace-id");
        assert!(enhanced_ctx.baggage.is_empty());
        assert!(enhanced_ctx.scoped_contexts.is_empty());
    }

    #[test]
    fn test_baggage_management() {
        let base_ctx = TraceContext {
            trace_id: "test-trace-id".to_string(),
            span_id: "test-span-id".to_string(),
            parent_span_id: None,
        };

        let mut enhanced_ctx = EnhancedTraceContext::new(base_ctx);

        // Add baggage
        enhanced_ctx
            .add_baggage("key1".to_string(), "value1".to_string())
            .unwrap();
        assert_eq!(
            enhanced_ctx.get_baggage("key1"),
            Some(&"value1".to_string())
        );

        // Remove baggage
        let removed = enhanced_ctx.remove_baggage("key1");
        assert_eq!(removed, Some("value1".to_string()));
        assert_eq!(enhanced_ctx.get_baggage("key1"), None);
    }

    #[test]
    fn test_scoped_context() {
        let base_ctx = TraceContext {
            trace_id: "test-trace-id".to_string(),
            span_id: "test-span-id".to_string(),
            parent_span_id: None,
        };

        let mut enhanced_ctx = EnhancedTraceContext::new(base_ctx);

        // Push scope
        let mut metadata = HashMap::new();
        metadata.insert("operation".to_string(), Value::String("test".to_string()));

        let scope_id = enhanced_ctx
            .push_scope("test_scope".to_string(), metadata)
            .unwrap();
        assert!(!scope_id.is_empty());

        // Check current scope
        let current_scope = enhanced_ctx.current_scope().unwrap();
        assert_eq!(current_scope.scope_name, "test_scope");
        assert_eq!(current_scope.scope_id, scope_id);

        // Pop scope
        let popped_scope = enhanced_ctx.pop_scope().unwrap();
        assert_eq!(popped_scope.scope_name, "test_scope");
        assert!(enhanced_ctx.current_scope().is_none());
    }

    #[test]
    fn test_baggage_header_conversion() {
        let base_ctx = TraceContext {
            trace_id: "test-trace-id".to_string(),
            span_id: "test-span-id".to_string(),
            parent_span_id: None,
        };

        let mut enhanced_ctx = EnhancedTraceContext::new(base_ctx);
        enhanced_ctx
            .add_baggage("key1".to_string(), "value1".to_string())
            .unwrap();
        enhanced_ctx
            .add_baggage("key2".to_string(), "value2".to_string())
            .unwrap();

        let header = enhanced_ctx.to_baggage_header();
        assert!(header.contains("key1=value1"));
        assert!(header.contains("key2=value2"));

        // Test parsing
        let mut new_ctx = EnhancedTraceContext::new(TraceContext {
            trace_id: "test2".to_string(),
            span_id: "test2".to_string(),
            parent_span_id: None,
        });

        new_ctx.from_baggage_header(&header).unwrap();
        assert_eq!(new_ctx.get_baggage("key1"), Some(&"value1".to_string()));
        assert_eq!(new_ctx.get_baggage("key2"), Some(&"value2".to_string()));
    }

    #[test]
    fn test_baggage_manager() {
        let config = CorrelationConfig::default();
        let manager = BaggageManager::new(config);

        // Test validation
        assert!(manager
            .validate_baggage_item("valid_key", "valid_value")
            .is_ok());
        assert!(manager.validate_baggage_item("", "value").is_err());
        assert!(manager
            .validate_baggage_item("key", &"x".repeat(2000))
            .is_err());
    }

    #[test]
    fn test_scoped_context_manager() {
        let config = CorrelationConfig::default();
        let manager = ScopedContextManager::new(config);

        // Register context
        let base_ctx = TraceContext {
            trace_id: "test-trace-id".to_string(),
            span_id: "test-span-id".to_string(),
            parent_span_id: None,
        };
        let enhanced_ctx = EnhancedTraceContext::new(base_ctx);

        manager.register_context(enhanced_ctx).unwrap();

        // Enter scope
        let mut metadata = HashMap::new();
        metadata.insert("test".to_string(), Value::String("value".to_string()));

        let scope_id = manager
            .enter_scope("test-trace-id", "test_scope".to_string(), metadata)
            .unwrap();
        assert!(!scope_id.is_empty());

        // Check current scope
        let current_scope = manager.current_scope("test-trace-id").unwrap();
        assert!(current_scope.is_some());
        assert_eq!(current_scope.unwrap().scope_name, "test_scope");

        // Exit scope
        let exited_scope = manager.exit_scope("test-trace-id").unwrap();
        assert!(exited_scope.is_some());
        assert_eq!(exited_scope.unwrap().scope_name, "test_scope");
    }
}
