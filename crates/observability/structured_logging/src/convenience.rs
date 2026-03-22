//! Context-aware processors for domain-specific logging operations
//!
//! These processors automatically detect and enhance standard log::info!, log::debug! calls
//! based on context and structured fields, maintaining standard Rust logging interface.

use crate::error::{Result, StructuredLoggingError};
use crate::extension::ConvenienceConfig;
use observability_core::{
    domain::LogProcessor, ports::MetricsPort, BasicMetricType, LogEntry, MetricsEntry,
};
use serde_json::Value;

#[cfg(test)]
use observability_core::{domain::create_log_entry, traits::LogLevel};

/// Thread-local context for domain operations
thread_local! {
    static LLM_CONTEXT: std::cell::RefCell<Option<LLMContext>> = std::cell::RefCell::new(None);
    static TEMPLATE_CONTEXT: std::cell::RefCell<Option<TemplateContext>> = std::cell::RefCell::new(None);
    static A2A_CONTEXT: std::cell::RefCell<Option<A2AContext>> = std::cell::RefCell::new(None);
    static REQUEST_CONTEXT: std::cell::RefCell<Option<RequestContext>> = std::cell::RefCell::new(None);
    /// Thread-local storage for the active MetricsPort implementation
    static METRICS_PORT: std::cell::RefCell<Option<Box<dyn MetricsPort>>> = std::cell::RefCell::new(None);
}

/// Context for LLM operations
#[derive(Debug, Clone)]
pub struct LLMContext {
    pub model: String,
    pub component: String,
    pub operation_start: String, // timestamp string for WASM compatibility
}

/// Context for template operations  
#[derive(Debug, Clone)]
pub struct TemplateContext {
    pub engine: String,
    pub template: String,
    pub component: String,
    pub operation_start: String,
}

/// Context for A2A operations
#[derive(Debug, Clone)]
pub struct A2AContext {
    pub message_type: String,
    pub from_agent: String,
    pub to_agent: String,
    pub component: String,
    pub operation_start: String,
}

/// Context for request operations
#[derive(Debug, Clone)]
pub struct RequestContext {
    pub request_id: String,
    pub user_id: Option<String>,
    pub session_id: Option<String>,
    pub operation_start: String,
}

/// Set LLM context for subsequent standard log calls
#[macro_export]
macro_rules! llm_context {
    ($model:expr) => {
        $crate::convenience::set_llm_context($model, "openai_client_wasm");
    };
    ($model:expr, component: $component:expr) => {
        $crate::convenience::set_llm_context($model, $component);
    };
}

/// Set template context for subsequent standard log calls
#[macro_export]
macro_rules! template_context {
    ($engine:expr, $template:expr) => {
        $crate::convenience::set_template_context($engine, $template, "template_engines");
    };
    ($engine:expr, $template:expr, component: $component:expr) => {
        $crate::convenience::set_template_context($engine, $template, $component);
    };
}

/// Set A2A context for subsequent standard log calls
#[macro_export]
macro_rules! a2a_context {
    ($message_type:expr, from: $from:expr, to: $to:expr) => {
        $crate::convenience::set_a2a_context($message_type, $from, $to, "a2a_jsonrpc_server");
    };
    ($message_type:expr, from: $from:expr, to: $to:expr, component: $component:expr) => {
        $crate::convenience::set_a2a_context($message_type, $from, $to, $component);
    };
}

/// Clear all domain contexts
#[macro_export]
macro_rules! clear_context {
    () => {
        $crate::convenience::clear_all_contexts();
    };
}

/// Set LLM context
pub fn set_llm_context(model: &str, component: &str) {
    let context = LLMContext {
        model: model.to_string(),
        component: component.to_string(),
        operation_start: get_current_timestamp(),
    };
    LLM_CONTEXT.with(|ctx| {
        *ctx.borrow_mut() = Some(context);
    });
}

/// Set template context
pub fn set_template_context(engine: &str, template: &str, component: &str) {
    let context = TemplateContext {
        engine: engine.to_string(),
        template: template.to_string(),
        component: component.to_string(),
        operation_start: get_current_timestamp(),
    };
    TEMPLATE_CONTEXT.with(|ctx| {
        *ctx.borrow_mut() = Some(context);
    });
}

/// Set A2A context
pub fn set_a2a_context(message_type: &str, from_agent: &str, to_agent: &str, component: &str) {
    let context = A2AContext {
        message_type: message_type.to_string(),
        from_agent: from_agent.to_string(),
        to_agent: to_agent.to_string(),
        component: component.to_string(),
        operation_start: get_current_timestamp(),
    };
    A2A_CONTEXT.with(|ctx| {
        *ctx.borrow_mut() = Some(context);
    });
}

/// Set request context
pub fn set_request_context(request_id: &str, user_id: Option<&str>, session_id: Option<&str>) {
    let context = RequestContext {
        request_id: request_id.to_string(),
        user_id: user_id.map(|s| s.to_string()),
        session_id: session_id.map(|s| s.to_string()),
        operation_start: get_current_timestamp(),
    };
    REQUEST_CONTEXT.with(|ctx| {
        *ctx.borrow_mut() = Some(context);
    });
}

/// Clear LLM context only
pub fn clear_llm_context() {
    LLM_CONTEXT.with(|ctx| *ctx.borrow_mut() = None);
}

/// Clear template context only
pub fn clear_template_context() {
    TEMPLATE_CONTEXT.with(|ctx| *ctx.borrow_mut() = None);
}

/// Clear A2A context only
pub fn clear_a2a_context() {
    A2A_CONTEXT.with(|ctx| *ctx.borrow_mut() = None);
}

/// Clear request context only
pub fn clear_request_context() {
    REQUEST_CONTEXT.with(|ctx| *ctx.borrow_mut() = None);
}

/// Clear all contexts
pub fn clear_all_contexts() {
    clear_llm_context();
    clear_template_context();
    clear_a2a_context();
    clear_request_context();
}

/// Get current timestamp as string (WASM compatible)
fn get_current_timestamp() -> String {
    // Use chrono consistently throughout the workspace for timestamps
    chrono::Utc::now().to_rfc3339()
}

/// Processor that automatically enhances standard log calls with domain-specific structure
#[derive(Debug)]
pub struct DomainContextProcessor {
    config: ConvenienceConfig,
}

impl DomainContextProcessor {
    pub fn new(config: ConvenienceConfig) -> Self {
        Self { config }
    }

    /// Detect if this is an LLM operation and enhance accordingly
    fn enhance_llm_entry(&self, mut entry: LogEntry) -> Result<LogEntry> {
        LLM_CONTEXT.with(|ctx| {
            if let Some(llm_ctx) = ctx.borrow().as_ref() {
                // Add LLM-specific fields
                if let serde_json::Value::Object(ref mut map) = entry.fields {
                    map.insert(
                        "component".to_string(),
                        Value::String(llm_ctx.component.clone()),
                    );
                    map.insert("model".to_string(), Value::String(llm_ctx.model.clone()));
                    map.insert(
                        "operation".to_string(),
                        Value::String("llm_request".to_string()),
                    );

                    // Extract duration, tokens, status from existing fields if present
                    if let Some(duration_val) = map.get("duration") {
                        if let Some(duration_ms) = duration_val.as_u64() {
                            map.insert(
                                "duration_ms".to_string(),
                                Value::Number(duration_ms.into()),
                            );
                        }
                    }

                    // Auto-detect tokens from fields
                    if let Some(tokens_val) = map.get("tokens") {
                        map.insert("tokens".to_string(), tokens_val.clone());
                    }

                    // Auto-detect status from fields
                    if let Some(status_val) = map.get("status") {
                        map.insert("status".to_string(), status_val.clone());
                    }
                }

                // Enhance message if it's generic
                if entry.message == "info" || entry.message.is_empty() {
                    entry.message = "LLM request completed".to_string();
                }
            }
        });
        Ok(entry)
    }

    /// Detect if this is a template operation and enhance accordingly
    fn enhance_template_entry(&self, mut entry: LogEntry) -> Result<LogEntry> {
        TEMPLATE_CONTEXT.with(|ctx| {
            if let Some(template_ctx) = ctx.borrow().as_ref() {
                // Add template-specific fields
                if let serde_json::Value::Object(ref mut map) = entry.fields {
                    map.insert(
                        "component".to_string(),
                        Value::String(template_ctx.component.clone()),
                    );
                    map.insert(
                        "engine".to_string(),
                        Value::String(template_ctx.engine.clone()),
                    );
                    map.insert(
                        "template".to_string(),
                        Value::String(template_ctx.template.clone()),
                    );
                    map.insert(
                        "operation".to_string(),
                        Value::String("template_render".to_string()),
                    );

                    // Extract duration, size from existing fields if present
                    if let Some(duration_val) = map.get("duration") {
                        if let Some(duration_ms) = duration_val.as_u64() {
                            map.insert(
                                "duration_ms".to_string(),
                                Value::Number(duration_ms.into()),
                            );
                        }
                    }

                    if let Some(size_val) = map.get("size") {
                        map.insert("output_size".to_string(), size_val.clone());
                    }
                }

                // Enhance message if it's generic
                if entry.message == "info" || entry.message.is_empty() {
                    entry.message = "Template rendered".to_string();
                }
            }
        });
        Ok(entry)
    }

    /// Detect if this is an A2A operation and enhance accordingly
    fn enhance_a2a_entry(&self, mut entry: LogEntry) -> Result<LogEntry> {
        A2A_CONTEXT.with(|ctx| {
            if let Some(a2a_ctx) = ctx.borrow().as_ref() {
                // Add A2A-specific fields
                if let serde_json::Value::Object(ref mut map) = entry.fields {
                    map.insert(
                        "component".to_string(),
                        Value::String(a2a_ctx.component.clone()),
                    );
                    map.insert(
                        "message_type".to_string(),
                        Value::String(a2a_ctx.message_type.clone()),
                    );
                    map.insert(
                        "from_agent".to_string(),
                        Value::String(a2a_ctx.from_agent.clone()),
                    );
                    map.insert(
                        "to_agent".to_string(),
                        Value::String(a2a_ctx.to_agent.clone()),
                    );
                    map.insert(
                        "protocol".to_string(),
                        Value::String("json-rpc-2.0".to_string()),
                    );
                    map.insert(
                        "operation".to_string(),
                        Value::String("a2a_message".to_string()),
                    );

                    // Extract duration from existing fields if present
                    if let Some(duration_val) = map.get("duration") {
                        if let Some(duration_ms) = duration_val.as_u64() {
                            map.insert(
                                "duration_ms".to_string(),
                                Value::Number(duration_ms.into()),
                            );
                        }
                    }
                }

                // Enhance message if it's generic
                if entry.message == "info" || entry.message.is_empty() {
                    entry.message = "A2A message processed".to_string();
                }
            }
        });
        Ok(entry)
    }

    /// Auto-detect domain from structured fields (fallback when no context set)
    fn auto_detect_and_enhance(&self, mut entry: LogEntry) -> Result<LogEntry> {
        if let serde_json::Value::Object(ref map) = entry.fields {
            // Detect LLM operation from fields
            if map.contains_key("model") || map.contains_key("tokens") {
                if let serde_json::Value::Object(ref mut fields) = entry.fields {
                    fields.insert(
                        "operation".to_string(),
                        Value::String("llm_request".to_string()),
                    );
                    if entry.message == "info" || entry.message.is_empty() {
                        entry.message = "LLM request completed".to_string();
                    }
                }
            }
            // Detect template operation from fields
            else if map.contains_key("engine") || map.contains_key("template") {
                if let serde_json::Value::Object(ref mut fields) = entry.fields {
                    fields.insert(
                        "operation".to_string(),
                        Value::String("template_render".to_string()),
                    );
                    if entry.message == "info" || entry.message.is_empty() {
                        entry.message = "Template rendered".to_string();
                    }
                }
            }
            // Detect A2A operation from fields
            else if map.contains_key("from_agent")
                || map.contains_key("to_agent")
                || map.contains_key("message_type")
            {
                if let serde_json::Value::Object(ref mut fields) = entry.fields {
                    fields.insert(
                        "operation".to_string(),
                        Value::String("a2a_message".to_string()),
                    );
                    fields.insert(
                        "protocol".to_string(),
                        Value::String("json-rpc-2.0".to_string()),
                    );
                    if entry.message == "info" || entry.message.is_empty() {
                        entry.message = "A2A message processed".to_string();
                    }
                }
            }
        }
        Ok(entry)
    }
}

impl LogProcessor for DomainContextProcessor {
    fn process(&self, entry: LogEntry) -> observability_core::ObservabilityResult<LogEntry> {
        let mut enhanced_entry = entry;

        // Try context-based enhancement first
        enhanced_entry = self
            .enhance_llm_entry(enhanced_entry)
            .map_err(|e| observability_core::ObservabilityError::logging(e.to_string()))?;

        enhanced_entry = self
            .enhance_template_entry(enhanced_entry)
            .map_err(|e| observability_core::ObservabilityError::logging(e.to_string()))?;

        enhanced_entry = self
            .enhance_a2a_entry(enhanced_entry)
            .map_err(|e| observability_core::ObservabilityError::logging(e.to_string()))?;

        // Auto-detect from fields as fallback
        enhanced_entry = self
            .auto_detect_and_enhance(enhanced_entry)
            .map_err(|e| observability_core::ObservabilityError::logging(e.to_string()))?;

        Ok(enhanced_entry)
    }

    fn name(&self) -> &'static str {
        "domain_context"
    }
}

/// Convenience manager that integrates domain-aware processing into the processor chain
pub struct ConvenienceManager {
    processor: DomainContextProcessor,
    config: ConvenienceConfig,
}

impl ConvenienceManager {
    /// Create new convenience manager
    pub fn new(config: &ConvenienceConfig) -> Result<Self> {
        Ok(Self {
            processor: DomainContextProcessor::new(config.clone()),
            config: config.clone(),
        })
    }

    /// Process log entry through domain-aware enhancement
    pub fn process_entry(&self, entry: LogEntry) -> Result<LogEntry> {
        self.processor
            .process(entry)
            .map_err(StructuredLoggingError::from)
    }

    /// Get the domain context processor for integration with processor chain
    pub fn get_processor(&self) -> &DomainContextProcessor {
        &self.processor
    }
}

/// Set the active MetricsPort implementation
pub fn set_metrics_port(port: Box<dyn MetricsPort>) {
    METRICS_PORT.with(|p| {
        *p.borrow_mut() = Some(port);
    });
}

/// Clear the active MetricsPort implementation
pub fn clear_metrics_port() {
    METRICS_PORT.with(|p| {
        *p.borrow_mut() = None;
    });
}

/// Emit an LLM request duration metric with context
pub fn emit_llm_request_duration(model: &str, duration_ms: u64) -> Result<()> {
    METRICS_PORT.with(|port| {
        if let Some(ref metrics_port) = *port.borrow() {
            let metric_name = "llm_request_duration_ms";
            let _ = metrics_port
                .emit_histogram_simple(metric_name, duration_ms as f64)
                .map_err(|e| println!("[METRICS_ERROR] Failed to emit {}: {}", metric_name, e));
        } else {
            // Fallback to stdout if no MetricsPort is available
            LLM_CONTEXT.with(|ctx| {
                if let Some(ref context) = *ctx.borrow() {
                    println!(
                        "[METRIC] llm_request_duration_ms {} model={} component={}",
                        duration_ms, model, context.component
                    );
                } else {
                    println!(
                        "[METRIC] llm_request_duration_ms {} model={}",
                        duration_ms, model
                    );
                }
            });
        }
    });
    Ok(())
}

/// Emit an LLM token usage metric with context
pub fn emit_llm_tokens_used(model: &str, tokens: u32) -> Result<()> {
    METRICS_PORT.with(|port| {
        if let Some(ref metrics_port) = *port.borrow() {
            let metric_name = "llm_tokens_used";
            let _ = metrics_port
                .emit_counter_simple(metric_name, tokens as f64)
                .map_err(|e| println!("[METRICS_ERROR] Failed to emit {}: {}", metric_name, e));
        } else {
            // Fallback to stdout if no MetricsPort is available
            LLM_CONTEXT.with(|ctx| {
                if let Some(ref context) = *ctx.borrow() {
                    println!(
                        "[METRIC] llm_tokens_used {} model={} component={}",
                        tokens, model, context.component
                    );
                } else {
                    println!("[METRIC] llm_tokens_used {} model={}", tokens, model);
                }
            });
        }
    });
    Ok(())
}

/// Emit an A2A message latency metric with context
pub fn emit_a2a_message_latency(latency_ms: u64) -> Result<()> {
    METRICS_PORT.with(|port| {
        if let Some(ref metrics_port) = *port.borrow() {
            let metric_name = "a2a_message_latency_ms";
            let _ = metrics_port
                .emit_histogram_simple(metric_name, latency_ms as f64)
                .map_err(|e| println!("[METRICS_ERROR] Failed to emit {}: {}", metric_name, e));
        } else {
            // Fallback to stdout if no MetricsPort is available
            A2A_CONTEXT.with(|ctx| {
                if let Some(ref context) = *ctx.borrow() {
                    println!(
                        "[METRIC] a2a_message_latency_ms {} from={} to={} type={}",
                        latency_ms, context.from_agent, context.to_agent, context.message_type
                    );
                } else {
                    println!("[METRIC] a2a_message_latency_ms {}", latency_ms);
                }
            });
        }
    });
    Ok(())
}

/// Emit a template render duration metric with context  
pub fn emit_template_render_duration(template: &str, duration_ms: u64) -> Result<()> {
    METRICS_PORT.with(|port| {
        if let Some(ref metrics_port) = *port.borrow() {
            let metric_name = "template_render_duration_ms";
            let _ = metrics_port.emit_histogram_simple(metric_name, duration_ms as f64)
                .map_err(|e| println!("[METRICS_ERROR] Failed to emit {}: {}", metric_name, e));
        } else {
            // Fallback to stdout if no MetricsPort is available
            TEMPLATE_CONTEXT.with(|ctx| {
                if let Some(ref context) = *ctx.borrow() {
                    println!("[METRIC] template_render_duration_ms {} engine={} template={} component={}", 
                             duration_ms, context.engine, template, context.component);
                } else {
                    println!("[METRIC] template_render_duration_ms {} template={}", duration_ms, template);
                }
            });
        }
    });
    Ok(())
}

/// Emit request processing duration metric with context
pub fn emit_request_duration(duration_ms: u64) -> Result<()> {
    METRICS_PORT.with(|port| {
        if let Some(ref metrics_port) = *port.borrow() {
            let metric_name = "request_duration_ms";
            let _ = metrics_port
                .emit_histogram_simple(metric_name, duration_ms as f64)
                .map_err(|e| println!("[METRICS_ERROR] Failed to emit {}: {}", metric_name, e));
        } else {
            // Fallback to stdout if no MetricsPort is available
            REQUEST_CONTEXT.with(|ctx| {
                if let Some(ref context) = *ctx.borrow() {
                    let mut metric = format!(
                        "[METRIC] request_duration_ms {} request_id={}",
                        duration_ms, context.request_id
                    );
                    if let Some(ref user_id) = context.user_id {
                        metric.push_str(&format!(" user_id={}", user_id));
                    }
                    if let Some(ref session_id) = context.session_id {
                        metric.push_str(&format!(" session_id={}", session_id));
                    }
                    println!("{}", metric);
                } else {
                    println!("[METRIC] request_duration_ms {}", duration_ms);
                }
            });
        }
    });
    Ok(())
}

/// Generic counter metric with automatic context detection
pub fn emit_counter(name: &str, value: f64) -> Result<()> {
    METRICS_PORT.with(|port| {
        if let Some(ref metrics_port) = *port.borrow() {
            let _ = metrics_port
                .emit_counter_simple(name, value)
                .map_err(|e| println!("[METRICS_ERROR] Failed to emit counter {}: {}", name, e));
        } else {
            // Fallback to stdout with context detection
            let mut labels = Vec::new();

            // Auto-detect active contexts and add as labels
            LLM_CONTEXT.with(|ctx| {
                if let Some(ref context) = *ctx.borrow() {
                    labels.push(format!("llm_component={}", context.component));
                }
            });

            A2A_CONTEXT.with(|ctx| {
                if let Some(ref context) = *ctx.borrow() {
                    labels.push(format!("from_agent={}", context.from_agent));
                    labels.push(format!("to_agent={}", context.to_agent));
                    labels.push(format!("message_type={}", context.message_type));
                }
            });

            TEMPLATE_CONTEXT.with(|ctx| {
                if let Some(ref context) = *ctx.borrow() {
                    labels.push(format!("template_engine={}", context.engine));
                }
            });

            REQUEST_CONTEXT.with(|ctx| {
                if let Some(ref context) = *ctx.borrow() {
                    labels.push(format!("request_id={}", context.request_id));
                    if let Some(ref user_id) = context.user_id {
                        labels.push(format!("user_id={}", user_id));
                    }
                }
            });

            let labels_str = if labels.is_empty() {
                String::new()
            } else {
                format!(" {}", labels.join(" "))
            };

            println!("[METRIC] {} counter {}{}", name, value, labels_str);
        }
    });
    Ok(())
}

/// Generic histogram metric with automatic context detection
pub fn emit_histogram(name: &str, value: f64) -> Result<()> {
    METRICS_PORT.with(|port| {
        if let Some(ref metrics_port) = *port.borrow() {
            let _ = metrics_port
                .emit_histogram_simple(name, value)
                .map_err(|e| println!("[METRICS_ERROR] Failed to emit histogram {}: {}", name, e));
        } else {
            // Fallback to stdout with context detection
            let mut labels = Vec::new();

            // Auto-detect active contexts (same logic as emit_counter)
            LLM_CONTEXT.with(|ctx| {
                if let Some(ref context) = *ctx.borrow() {
                    labels.push(format!("llm_component={}", context.component));
                }
            });

            A2A_CONTEXT.with(|ctx| {
                if let Some(ref context) = *ctx.borrow() {
                    labels.push(format!("from_agent={}", context.from_agent));
                    labels.push(format!("to_agent={}", context.to_agent));
                }
            });

            let labels_str = if labels.is_empty() {
                String::new()
            } else {
                format!(" {}", labels.join(" "))
            };

            println!("[METRIC] {} histogram {}{}", name, value, labels_str);
        }
    });
    Ok(())
}

/// Convenience function to log LLM requests with structured format
pub fn log_llm_request(
    model: &str,
    duration: web_time::Duration,
    tokens: u32,
    status: &str,
) -> Result<LogEntry> {
    use observability_core::domain::create_log_entry;
    use observability_core::traits::LogLevel;

    let entry = create_log_entry(
        LogLevel::Info,
        &format!(
            "LLM request completed: {} tokens in {}ms",
            tokens,
            duration.as_millis()
        ),
        serde_json::json!({
            "model": model,
            "duration_ms": duration.as_millis(),
            "tokens": tokens,
            "status": status,
            "operation": "llm_request",
            "component": "openai_client_wasm"
        }),
    );

    Ok(entry)
}

/// Convenience function to log template rendering with structured format
pub fn log_template_render(
    engine: &str,
    template: &str,
    duration: web_time::Duration,
    size: usize,
    status: &str,
) -> Result<LogEntry> {
    use observability_core::domain::create_log_entry;
    use observability_core::traits::LogLevel;

    let entry = create_log_entry(
        LogLevel::Info,
        &format!(
            "Template rendered: {} using {} in {}ms",
            template,
            engine,
            duration.as_millis()
        ),
        serde_json::json!({
            "engine": engine,
            "template": template,
            "duration_ms": duration.as_millis(),
            "output_size": size,
            "status": status,
            "operation": "template_render",
            "component": "template_engines"
        }),
    );

    Ok(entry)
}

/// Convenience function to log A2A messages with structured format
pub fn log_a2a_message(
    message_type: &str,
    from_agent: &str,
    to_agent: &str,
    duration: Option<web_time::Duration>,
    status: &str,
) -> Result<LogEntry> {
    use observability_core::domain::create_log_entry;
    use observability_core::traits::LogLevel;

    let mut fields = serde_json::json!({
        "message_type": message_type,
        "from_agent": from_agent,
        "to_agent": to_agent,
        "status": status,
        "operation": "a2a_message",
        "protocol": "json-rpc-2.0",
        "component": "a2a_jsonrpc_server"
    });

    if let Some(dur) = duration {
        fields["duration_ms"] = serde_json::json!(dur.as_millis());
    }

    let message = if let Some(dur) = duration {
        format!(
            "A2A message processed: {} from {} to {} in {}ms",
            message_type,
            from_agent,
            to_agent,
            dur.as_millis()
        )
    } else {
        format!(
            "A2A message processed: {} from {} to {}",
            message_type, from_agent, to_agent
        )
    };

    let entry = create_log_entry(LogLevel::Info, &message, fields);

    Ok(entry)
}

/// Convenience macros for structured logging
#[macro_export]
macro_rules! llm_log {
    ($model:expr, $duration:expr, $tokens:expr, $status:expr) => {
        $crate::convenience::log_llm_request($model, $duration, $tokens, $status)
    };
}

#[macro_export]
macro_rules! template_log {
    ($engine:expr, $template:expr, $duration:expr, $size:expr, $status:expr) => {
        $crate::convenience::log_template_render($engine, $template, $duration, $size, $status)
    };
}

#[macro_export]
macro_rules! a2a_log {
    ($message_type:expr, $from:expr, $to:expr, $status:expr) => {
        $crate::convenience::log_a2a_message($message_type, $from, $to, None, $status)
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_llm_context_enhancement() {
        let config = ConvenienceConfig::default();
        let processor = DomainContextProcessor::new(config);

        // Set LLM context
        set_llm_context("gpt-4", "openai_client");

        // Create a basic log entry
        let entry = create_log_entry(
            LogLevel::Info,
            "Request completed",
            serde_json::json!({
                "duration": 250,
                "tokens": 1500,
                "status": "success"
            }),
        );

        let enhanced = processor.process(entry).unwrap();

        // Check that LLM-specific fields were added
        if let serde_json::Value::Object(ref map) = enhanced.fields {
            assert_eq!(
                map.get("model").unwrap(),
                &Value::String("gpt-4".to_string())
            );
            assert_eq!(
                map.get("component").unwrap(),
                &Value::String("openai_client".to_string())
            );
            assert_eq!(
                map.get("operation").unwrap(),
                &Value::String("llm_request".to_string())
            );
        }

        clear_all_contexts();
    }

    #[test]
    fn test_template_context_enhancement() {
        let config = ConvenienceConfig::default();
        let processor = DomainContextProcessor::new(config);

        // Set template context
        set_template_context("Sailfish", "agent_card.stpl", "template_engine");

        let entry = create_log_entry(
            LogLevel::Debug,
            "Render completed",
            serde_json::json!({
                "duration": 50,
                "size": 2048
            }),
        );

        let enhanced = processor.process(entry).unwrap();

        if let serde_json::Value::Object(ref map) = enhanced.fields {
            assert_eq!(
                map.get("engine").unwrap(),
                &Value::String("Sailfish".to_string())
            );
            assert_eq!(
                map.get("template").unwrap(),
                &Value::String("agent_card.stpl".to_string())
            );
            assert_eq!(
                map.get("operation").unwrap(),
                &Value::String("template_render".to_string())
            );
        }

        clear_all_contexts();
    }

    #[test]
    fn test_auto_detection() {
        let config = ConvenienceConfig::default();
        let processor = DomainContextProcessor::new(config);

        // Test auto-detection without context
        let entry = create_log_entry(
            LogLevel::Info,
            "info",
            serde_json::json!({
                "model": "gpt-4",
                "tokens": 1000
            }),
        );

        let enhanced = processor.process(entry).unwrap();

        if let serde_json::Value::Object(ref map) = enhanced.fields {
            assert_eq!(
                map.get("operation").unwrap(),
                &Value::String("llm_request".to_string())
            );
        }
        assert_eq!(enhanced.message, "LLM request completed");
    }
}
