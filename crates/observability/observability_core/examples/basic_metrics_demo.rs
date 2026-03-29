use observability_core::domain::TraceContext;
use observability_core::{
    BasicMetricType, MetricsEntry, MetricsPort, ObservabilityResult, UnifiedWasmStdoutAdapter,
    WasmStdoutMetricsAdapter, create_counter_metric, create_histogram_metric,
};

fn main() -> ObservabilityResult<()> {
    println!("🎯 Basic MetricsPort Demo");

    // Create simple metrics adapter
    let metrics_adapter = WasmStdoutMetricsAdapter::new();

    println!("\n📊 Testing simple metrics emission:");

    // Test counter
    metrics_adapter.emit_counter_simple("requests_total", 1.0)?;

    // Test histogram
    metrics_adapter.emit_histogram_simple("request_duration_ms", 150.0)?;

    // Test gauge
    metrics_adapter.emit_gauge_simple("active_connections", 5.0)?;

    println!("\n🔄 Testing unified adapter (logs + metrics correlation):");

    // Create unified adapter for correlation
    let unified_adapter = UnifiedWasmStdoutAdapter::new();

    // Test correlated metrics
    unified_adapter.emit_counter_simple("llm_requests", 1.0)?;
    unified_adapter.emit_histogram_simple("llm_response_time_ms", 2500.0)?;

    println!("\n📈 Testing MetricsEntry with trace context:");

    // Create metrics with trace context for correlation
    let trace_context = TraceContext {
        trace_id: "trace-123456".to_string(),
        span_id: "span-789".to_string(),
        parent_span_id: Some("parent-456".to_string()),
    };

    let counter_metric = create_counter_metric("a2a_messages_sent", 3.0)
        .with_trace_context(trace_context.clone())
        .with_source(
            Some("a2a_jsonrpc_core".to_string()),
            Some("message_handler".to_string()),
            Some("send_request".to_string()),
        );

    println!("Metric with context: {}", counter_metric.to_json());

    let histogram_metric = create_histogram_metric("template_render_duration_us", 1250.0)
        .with_trace_context(trace_context)
        .with_source(
            Some("template_engines".to_string()),
            Some("sailfish_renderer".to_string()),
            Some("render_template".to_string()),
        );

    println!("Metric with context: {}", histogram_metric.to_json());

    println!("\n✅ Basic metrics demo completed!");

    Ok(())
}
