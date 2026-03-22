//! Auto-instrumentation support for OpenTelemetry

use observability_core::{ObservabilityPlugin, W3CTraceContext};
use std::collections::HashMap;
use std::sync::Arc;

/// Auto-instrumentation wrapper for HTTP requests
pub struct AutoInstrumentedHttpClient {
    inner: reqwest::Client,
    observability: Arc<dyn ObservabilityPlugin>,
}

impl AutoInstrumentedHttpClient {
    /// Create a new auto-instrumented HTTP client
    pub fn new(client: reqwest::Client, observability: Arc<dyn ObservabilityPlugin>) -> Self {
        Self {
            inner: client,
            observability,
        }
    }

    /// Execute a GET request with automatic tracing
    pub async fn get(&self, url: &str) -> Result<reqwest::Response, reqwest::Error> {
        let _span = self
            .observability
            .start_span("http_get", &[("http.method", "GET"), ("http.url", url)]);

        let result = self.inner.get(url).send().await;

        // Add response details to span
        if let Ok(response) = &result {
            _span.add_attribute("http.status_code", &response.status().as_u16().to_string());
        }

        result
    }

    /// Execute a POST request with automatic tracing
    pub async fn post(
        &self,
        url: &str,
        body: impl Into<reqwest::Body>,
    ) -> Result<reqwest::Response, reqwest::Error> {
        let _span = self
            .observability
            .start_span("http_post", &[("http.method", "POST"), ("http.url", url)]);

        let result = self.inner.post(url).body(body).send().await;

        // Add response details to span
        if let Ok(response) = &result {
            _span.add_attribute("http.status_code", &response.status().as_u16().to_string());
        }

        result
    }
}

/// Auto-instrumentation for function calls
pub struct FunctionInstrumentation {
    observability: Arc<dyn ObservabilityPlugin>,
}

impl FunctionInstrumentation {
    /// Create new function instrumentation
    pub fn new(observability: Arc<dyn ObservabilityPlugin>) -> Self {
        Self { observability }
    }

    /// Instrument a function call
    pub async fn instrument<F, R>(&self, name: &str, attributes: &[(&str, &str)], f: F) -> R
    where
        F: std::future::Future<Output = R>,
    {
        let _span = self.observability.start_span(name, attributes);
        f.await
    }

    /// Instrument a synchronous function call
    pub fn instrument_sync<F, R>(&self, name: &str, attributes: &[(&str, &str)], f: F) -> R
    where
        F: FnOnce() -> R,
    {
        let _span = self.observability.start_span(name, attributes);
        f()
    }
}

/// W3C trace context propagation for HTTP headers
pub struct TraceContextPropagator;

impl TraceContextPropagator {
    /// Inject W3C trace context into HTTP headers
    pub fn inject(context: &W3CTraceContext, headers: &mut HashMap<String, String>) {
        let trace_headers = context.to_headers();
        for (key, value) in trace_headers {
            headers.insert(key, value);
        }
    }

    /// Extract W3C trace context from HTTP headers
    pub fn extract(headers: &HashMap<String, String>) -> Option<W3CTraceContext> {
        W3CTraceContext::from_headers(headers).unwrap_or(None)
    }

    /// Inject trace context into reqwest headers
    pub fn inject_reqwest(
        context: &W3CTraceContext,
        builder: reqwest::RequestBuilder,
    ) -> reqwest::RequestBuilder {
        let trace_headers = context.to_headers();
        let mut request_builder = builder;

        for (key, value) in trace_headers {
            request_builder = request_builder.header(key, value);
        }

        request_builder
    }
}

/// Macro for automatic function instrumentation
#[macro_export]
macro_rules! auto_instrument {
    ($observability:expr, $name:expr, $func:expr) => {{
        let instrumentation =
            $crate::auto_instrumentation::FunctionInstrumentation::new($observability);
        instrumentation.instrument_sync($name, &[], || $func)
    }};
    ($observability:expr, $name:expr, $attrs:expr, $func:expr) => {{
        let instrumentation =
            $crate::auto_instrumentation::FunctionInstrumentation::new($observability);
        instrumentation.instrument_sync($name, $attrs, || $func)
    }};
}

/// Macro for automatic async function instrumentation
#[macro_export]
macro_rules! auto_instrument_async {
    ($observability:expr, $name:expr, $func:expr) => {{
        let instrumentation =
            $crate::auto_instrumentation::FunctionInstrumentation::new($observability);
        instrumentation.instrument($name, &[], $func).await
    }};
    ($observability:expr, $name:expr, $attrs:expr, $func:expr) => {{
        let instrumentation =
            $crate::auto_instrumentation::FunctionInstrumentation::new($observability);
        instrumentation.instrument($name, $attrs, $func).await
    }};
}

#[cfg(test)]
mod tests {
    use super::*;
    use observability_core::{NoOpObservabilityPlugin, W3CTraceContext};
    use std::sync::Arc;

    #[test]
    fn test_trace_context_propagation() {
        let context = W3CTraceContext::new_root();
        let mut headers = HashMap::new();

        TraceContextPropagator::inject(&context, &mut headers);

        assert!(headers.contains_key("traceparent"));

        let extracted = TraceContextPropagator::extract(&headers);
        assert!(extracted.is_some());
        assert_eq!(extracted.unwrap().trace_id, context.trace_id);
    }

    #[test]
    fn test_function_instrumentation() {
        let observability = Arc::new(NoOpObservabilityPlugin::new());
        let instrumentation = FunctionInstrumentation::new(observability);

        let result = instrumentation.instrument_sync("test_function", &[("key", "value")], || 42);

        assert_eq!(result, 42);
    }
}
