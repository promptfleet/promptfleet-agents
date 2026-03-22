//! Basic trace context foundation for observability core
//!
//! Provides W3C trace context implementation and basic context management ports.
//! Domain-specific implementations (LLM, A2A, etc.) are provided by higher-level crates.

use crate::error::{ObservabilityError, ObservabilityResult};
use std::collections::HashMap;
use std::fmt;
use uuid::Uuid;

/// W3C Trace Context implementation for distributed tracing
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct W3CTraceContext {
    /// W3C trace ID (32 hex characters)
    pub trace_id: String,
    /// W3C span ID (16 hex characters)  
    pub parent_id: String,
    /// Trace flags (2 hex characters)
    pub trace_flags: String,
    /// Additional trace state
    pub trace_state: Option<String>,
}

impl W3CTraceContext {
    /// Create a new root trace context
    pub fn new_root() -> Self {
        Self {
            trace_id: generate_trace_id(),
            parent_id: generate_span_id(),
            trace_flags: "01".to_string(), // Sampled
            trace_state: None,
        }
    }

    /// Create a child span context
    pub fn new_child(&self) -> Self {
        Self {
            trace_id: self.trace_id.clone(),
            parent_id: generate_span_id(),
            trace_flags: self.trace_flags.clone(),
            trace_state: self.trace_state.clone(),
        }
    }

    /// Create from W3C traceparent header
    pub fn from_traceparent(header: &str) -> ObservabilityResult<Self> {
        let parts: Vec<&str> = header.split('-').collect();

        if parts.len() != 4 {
            return Err(ObservabilityError::trace_context(
                "Invalid traceparent format, expected 4 parts separated by dashes",
            ));
        }

        let version = parts[0];
        if version != "00" {
            return Err(ObservabilityError::trace_context(format!(
                "Unsupported traceparent version: {}",
                version
            )));
        }

        let trace_id = parts[1];
        if trace_id.len() != 32 {
            return Err(ObservabilityError::trace_context(
                "Invalid trace ID length, expected 32 hex characters",
            ));
        }

        let parent_id = parts[2];
        if parent_id.len() != 16 {
            return Err(ObservabilityError::trace_context(
                "Invalid parent ID length, expected 16 hex characters",
            ));
        }

        let trace_flags = parts[3];
        if trace_flags.len() != 2 {
            return Err(ObservabilityError::trace_context(
                "Invalid trace flags length, expected 2 hex characters",
            ));
        }

        Ok(Self {
            trace_id: trace_id.to_string(),
            parent_id: parent_id.to_string(),
            trace_flags: trace_flags.to_string(),
            trace_state: None,
        })
    }

    /// Create from W3C headers
    pub fn from_headers(headers: &HashMap<String, String>) -> ObservabilityResult<Option<Self>> {
        if let Some(traceparent) = headers.get("traceparent") {
            let mut context = Self::from_traceparent(traceparent)?;

            // Parse tracestate if present
            if let Some(tracestate) = headers.get("tracestate") {
                context.trace_state = Some(tracestate.clone());
            }

            Ok(Some(context))
        } else {
            Ok(None)
        }
    }

    /// Generate W3C traceparent header value
    pub fn to_traceparent(&self) -> String {
        format!(
            "00-{}-{}-{}",
            self.trace_id, self.parent_id, self.trace_flags
        )
    }

    /// Generate W3C headers for propagation
    pub fn to_headers(&self) -> HashMap<String, String> {
        let mut headers = HashMap::new();
        headers.insert("traceparent".to_string(), self.to_traceparent());

        if let Some(trace_state) = &self.trace_state {
            headers.insert("tracestate".to_string(), trace_state.clone());
        }

        headers
    }

    /// Check if the trace is sampled
    pub fn is_sampled(&self) -> bool {
        // Check the least significant bit of trace flags
        if let Ok(flags) = u8::from_str_radix(&self.trace_flags, 16) {
            (flags & 0x01) == 0x01
        } else {
            false
        }
    }

    /// Set sampling flag
    pub fn set_sampled(&mut self, sampled: bool) {
        if let Ok(mut flags) = u8::from_str_radix(&self.trace_flags, 16) {
            if sampled {
                flags |= 0x01; // Set bit
            } else {
                flags &= !0x01; // Clear bit
            }
            self.trace_flags = format!("{:02x}", flags);
        }
    }

    /// Add or update trace state
    pub fn add_trace_state(&mut self, key: &str, value: &str) {
        let new_entry = format!("{}={}", key, value);

        match &self.trace_state {
            Some(existing) => {
                // Parse existing tracestate and update/add entry
                let mut entries: Vec<&str> = existing.split(',').collect();

                // Check if key already exists and update
                let mut found = false;
                for entry in &mut entries {
                    if entry.starts_with(&format!("{}=", key)) {
                        *entry = &new_entry;
                        found = true;
                        break;
                    }
                }

                if !found {
                    entries.insert(0, &new_entry); // Add new entry at beginning
                }

                self.trace_state = Some(entries.join(","));
            }
            None => {
                self.trace_state = Some(new_entry);
            }
        }
    }

    /// Get value from trace state
    pub fn get_trace_state(&self, key: &str) -> Option<String> {
        self.trace_state.as_ref().and_then(|state| {
            for entry in state.split(',') {
                if let Some((k, v)) = entry.split_once('=') {
                    if k == key {
                        return Some(v.to_string());
                    }
                }
            }
            None
        })
    }
}

impl fmt::Display for W3CTraceContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "trace_id={}, parent_id={}, flags={}",
            self.trace_id, self.parent_id, self.trace_flags
        )
    }
}

/// Simplified trace context for internal use
#[derive(Debug, Clone)]
pub struct TraceContext {
    pub trace_id: String,
    pub span_id: String,
    pub parent_span_id: Option<String>,
    pub sampled: bool,
}

impl TraceContext {
    /// Create a new root trace context
    pub fn new_root() -> Self {
        Self {
            trace_id: generate_trace_id(),
            span_id: generate_span_id(),
            parent_span_id: None,
            sampled: true,
        }
    }

    /// Create a child span
    pub fn new_child(&self) -> Self {
        Self {
            trace_id: self.trace_id.clone(),
            span_id: generate_span_id(),
            parent_span_id: Some(self.span_id.clone()),
            sampled: self.sampled,
        }
    }

    /// Convert to W3C trace context
    pub fn to_w3c(&self) -> W3CTraceContext {
        W3CTraceContext {
            trace_id: self.trace_id.clone(),
            parent_id: self
                .parent_span_id
                .clone()
                .unwrap_or_else(|| "0000000000000000".to_string()),
            trace_flags: if self.sampled {
                "01".to_string()
            } else {
                "00".to_string()
            },
            trace_state: None,
        }
    }

    /// Create from W3C trace context
    pub fn from_w3c(w3c: &W3CTraceContext) -> Self {
        Self {
            trace_id: w3c.trace_id.clone(),
            span_id: generate_span_id(), // Generate new span ID
            parent_span_id: Some(w3c.parent_id.clone()),
            sampled: w3c.is_sampled(),
        }
    }
}

/// Thread-local storage for current trace context
thread_local! {
    static CURRENT_CONTEXT: std::cell::RefCell<Option<TraceContext>> = std::cell::RefCell::new(None);
}

/// Set the current trace context for this thread
pub fn set_current_context(context: TraceContext) {
    CURRENT_CONTEXT.with(|c| {
        *c.borrow_mut() = Some(context);
    });
}

/// Get the current trace context for this thread
pub fn get_current_context() -> Option<TraceContext> {
    CURRENT_CONTEXT.with(|c| c.borrow().clone())
}

/// Clear the current trace context
pub fn clear_current_context() {
    CURRENT_CONTEXT.with(|c| {
        *c.borrow_mut() = None;
    });
}

/// Execute a function with a specific trace context
pub fn with_context<F, R>(context: TraceContext, f: F) -> R
where
    F: FnOnce() -> R,
{
    let previous = get_current_context();
    set_current_context(context);

    let result = f();

    // Restore previous context
    match previous {
        Some(ctx) => set_current_context(ctx),
        None => clear_current_context(),
    }

    result
}

/// Header injector for W3C trace context propagation
pub struct HeaderInjector<'a>(pub &'a mut HashMap<String, String>);

impl<'a> HeaderInjector<'a> {
    pub fn inject(&mut self, context: &W3CTraceContext) {
        let headers = context.to_headers();
        for (key, value) in headers {
            self.0.insert(key, value);
        }
    }
}

/// Header extractor for W3C trace context propagation
pub struct HeaderExtractor<'a>(pub &'a HashMap<String, String>);

impl<'a> HeaderExtractor<'a> {
    pub fn extract(&self) -> ObservabilityResult<Option<W3CTraceContext>> {
        W3CTraceContext::from_headers(self.0)
    }
}

/// Generate a new trace ID (32 hex characters)
fn generate_trace_id() -> String {
    format!("{:032x}", Uuid::new_v4().as_u128())
}

/// Generate a new span ID (16 hex characters)
fn generate_span_id() -> String {
    format!("{:016x}", Uuid::new_v4().as_u64_pair().0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_w3c_trace_context_creation() {
        let context = W3CTraceContext::new_root();
        assert_eq!(context.trace_id.len(), 32);
        assert_eq!(context.parent_id.len(), 16);
        assert_eq!(context.trace_flags, "01");
        assert!(context.is_sampled());
    }

    #[test]
    fn test_w3c_trace_context_child() {
        let parent = W3CTraceContext::new_root();
        let child = parent.new_child();

        assert_eq!(parent.trace_id, child.trace_id);
        assert_ne!(parent.parent_id, child.parent_id);
        assert_eq!(parent.trace_flags, child.trace_flags);
    }

    #[test]
    fn test_traceparent_parsing() {
        let traceparent = "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01";
        let context = W3CTraceContext::from_traceparent(traceparent).unwrap();

        assert_eq!(context.trace_id, "0af7651916cd43dd8448eb211c80319c");
        assert_eq!(context.parent_id, "b7ad6b7169203331");
        assert_eq!(context.trace_flags, "01");
        assert!(context.is_sampled());
    }

    #[test]
    fn test_traceparent_generation() {
        let context = W3CTraceContext {
            trace_id: "0af7651916cd43dd8448eb211c80319c".to_string(),
            parent_id: "b7ad6b7169203331".to_string(),
            trace_flags: "01".to_string(),
            trace_state: None,
        };

        let traceparent = context.to_traceparent();
        assert_eq!(
            traceparent,
            "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01"
        );
    }

    #[test]
    fn test_trace_state_management() {
        let mut context = W3CTraceContext::new_root();
        context.add_trace_state("congo", "t61rcWkgMzE");
        context.add_trace_state("rojo", "00f067aa0ba902b7");

        assert_eq!(
            context.get_trace_state("congo"),
            Some("t61rcWkgMzE".to_string())
        );
        assert_eq!(
            context.get_trace_state("rojo"),
            Some("00f067aa0ba902b7".to_string())
        );
        assert_eq!(context.get_trace_state("nonexistent"), None);
    }

    #[test]
    fn test_basic_trace_context() {
        let ctx = TraceContext::new_root();
        assert!(!ctx.trace_id.is_empty());
        assert!(!ctx.span_id.is_empty());
        assert!(ctx.sampled);
        assert!(ctx.parent_span_id.is_none());
    }

    #[test]
    fn test_thread_local_context() {
        let ctx = TraceContext::new_root();
        let trace_id = ctx.trace_id.clone();

        set_current_context(ctx);

        let retrieved = get_current_context().unwrap();
        assert_eq!(retrieved.trace_id, trace_id);

        clear_current_context();
        assert!(get_current_context().is_none());
    }

    #[test]
    fn test_scoped_context() {
        let ctx = TraceContext::new_root();
        let trace_id = ctx.trace_id.clone();

        let result = with_context(ctx, || {
            let current = get_current_context().unwrap();
            current.trace_id
        });

        assert_eq!(result, trace_id);
        // Context should be cleared after the closure
        assert!(get_current_context().is_none());
    }
}
