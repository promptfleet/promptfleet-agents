//! Phase 3 Integration Test: End-to-End Observability with MetricsPort Bridge
//!
//! This test demonstrates:
//! 1. PrometheusPlugin2025 implementing MetricsPort
//! 2. Convenience helpers using MetricsPort instead of println
//! 3. The bridge working correctly between structured_logging and prometheus_plugin_2025

use observability_core::ports::MetricsPort;
use structured_logging::convenience::{
    clear_all_contexts, clear_metrics_port, emit_a2a_message_latency, emit_counter, emit_histogram,
    emit_llm_request_duration, emit_llm_tokens_used, emit_template_render_duration,
    set_a2a_context, set_llm_context, set_metrics_port,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🎯 Phase 3 Integration Test: MetricsPort Bridge");
    println!("==============================================");

    // 1. Test fallback behavior (no MetricsPort set)
    println!("\n📊 Testing fallback behavior (no MetricsPort):");
    test_fallback_behavior()?;

    // 2. Test with mock MetricsPort
    println!("\n📊 Testing with MockMetricsPort:");
    test_with_mock_metrics_port()?;

    // 3. Test context integration
    println!("\n📊 Testing context integration:");
    test_context_integration()?;

    println!("\n✅ Phase 3 Integration Test completed successfully!");
    Ok(())
}

/// Test fallback behavior when no MetricsPort is set
fn test_fallback_behavior() -> Result<(), Box<dyn std::error::Error>> {
    // Set up LLM context
    set_llm_context("gpt-4", "test_component");

    // These should fall back to println! behavior
    emit_llm_request_duration("gpt-4", 2500)?;
    emit_llm_tokens_used("gpt-4", 1500)?;
    emit_a2a_message_latency(150)?;
    emit_template_render_duration("agent_card.stpl", 45)?;
    emit_counter("test_counter", 42.0)?;
    emit_histogram("test_histogram", 123.5)?;

    clear_all_contexts();
    Ok(())
}

/// Test with a mock MetricsPort implementation
fn test_with_mock_metrics_port() -> Result<(), Box<dyn std::error::Error>> {
    // Create and set mock MetricsPort
    let mock_port = Box::new(MockMetricsPort::new());
    set_metrics_port(mock_port);

    // Set up contexts
    set_llm_context("gpt-4", "openai_client");
    set_a2a_context("request", "agent1", "agent2", "a2a_server");

    // Test convenience functions - these should now use MetricsPort
    emit_llm_request_duration("gpt-4", 2800)?;
    emit_llm_tokens_used("gpt-4", 1750)?;
    emit_a2a_message_latency(200)?;
    emit_template_render_duration("response.stpl", 32)?;
    emit_counter("custom_counter", 99.9)?;
    emit_histogram("custom_histogram", 456.7)?;

    // Clear everything
    clear_all_contexts();
    clear_metrics_port();
    Ok(())
}

/// Test context integration with MetricsPort
fn test_context_integration() -> Result<(), Box<dyn std::error::Error>> {
    let mock_port = Box::new(MockMetricsPort::new());
    set_metrics_port(mock_port);

    // Test different context combinations
    set_llm_context("claude-3", "anthropic_client");
    emit_llm_request_duration("claude-3", 1200)?;

    set_a2a_context("response", "coordinator", "worker", "mesh_handler");
    emit_a2a_message_latency(75)?;

    clear_all_contexts();
    clear_metrics_port();
    Ok(())
}

/// Mock MetricsPort implementation for testing
struct MockMetricsPort {
    counter_calls: std::sync::Arc<std::sync::Mutex<Vec<(String, f64)>>>,
    histogram_calls: std::sync::Arc<std::sync::Mutex<Vec<(String, f64)>>>,
    gauge_calls: std::sync::Arc<std::sync::Mutex<Vec<(String, f64)>>>,
}

impl MockMetricsPort {
    fn new() -> Self {
        Self {
            counter_calls: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            histogram_calls: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            gauge_calls: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        }
    }
}

impl MetricsPort for MockMetricsPort {
    fn emit_counter_simple(
        &self,
        name: &str,
        value: f64,
    ) -> observability_core::ObservabilityResult<()> {
        println!(
            "  🔢 MockMetricsPort::emit_counter_simple({}, {})",
            name, value
        );
        if let Ok(mut calls) = self.counter_calls.lock() {
            calls.push((name.to_string(), value));
        }
        Ok(())
    }

    fn emit_histogram_simple(
        &self,
        name: &str,
        value: f64,
    ) -> observability_core::ObservabilityResult<()> {
        println!(
            "  📊 MockMetricsPort::emit_histogram_simple({}, {})",
            name, value
        );
        if let Ok(mut calls) = self.histogram_calls.lock() {
            calls.push((name.to_string(), value));
        }
        Ok(())
    }

    fn emit_gauge_simple(
        &self,
        name: &str,
        value: f64,
    ) -> observability_core::ObservabilityResult<()> {
        println!(
            "  📈 MockMetricsPort::emit_gauge_simple({}, {})",
            name, value
        );
        if let Ok(mut calls) = self.gauge_calls.lock() {
            calls.push((name.to_string(), value));
        }
        Ok(())
    }

    fn is_enabled(&self) -> bool {
        true
    }
}

/// Additional test to demonstrate the Prometheus plugin MetricsPort implementation
#[cfg(feature = "prometheus-federation")]
fn test_prometheus_plugin_integration() -> Result<(), Box<dyn std::error::Error>> {
    use prometheus_plugin_2025::{PrometheusConfig2025, PrometheusPlugin2025};

    println!("\n📊 Testing PrometheusPlugin2025 MetricsPort implementation:");

    // Create Prometheus plugin with test configuration
    let config = PrometheusConfig2025::builder()
        .with_job_name("test_agent")
        .with_instance("test_instance")
        .build();

    let prometheus_plugin = PrometheusPlugin2025::new(config)?;

    // Test MetricsPort implementation directly
    prometheus_plugin.emit_counter_simple("test_prometheus_counter", 1.0)?;
    prometheus_plugin.emit_histogram_simple("test_prometheus_histogram", 500.0)?;
    prometheus_plugin.emit_gauge_simple("test_prometheus_gauge", 75.5)?;

    println!("  ✅ PrometheusPlugin2025 MetricsPort implementation working!");

    // Test with convenience helpers
    let boxed_plugin = Box::new(prometheus_plugin) as Box<dyn MetricsPort>;
    set_metrics_port(boxed_plugin);

    set_llm_context("gpt-4", "test_prometheus_integration");
    emit_llm_request_duration("gpt-4", 3000)?;
    emit_llm_tokens_used("gpt-4", 2000)?;

    clear_all_contexts();
    clear_metrics_port();

    println!("  ✅ PrometheusPlugin2025 integration with convenience helpers working!");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_metrics_port() {
        let mock_port = MockMetricsPort::new();

        // Test basic functionality
        assert!(mock_port.emit_counter_simple("test", 1.0).is_ok());
        assert!(mock_port.emit_histogram_simple("test", 2.0).is_ok());
        assert!(mock_port.emit_gauge_simple("test", 3.0).is_ok());
        assert!(mock_port.is_enabled());

        // Verify calls were recorded
        if let Ok(counter_calls) = mock_port.counter_calls.lock() {
            assert_eq!(counter_calls.len(), 1);
        };
        if let Ok(histogram_calls) = mock_port.histogram_calls.lock() {
            assert_eq!(histogram_calls.len(), 1);
        };
        if let Ok(gauge_calls) = mock_port.gauge_calls.lock() {
            assert_eq!(gauge_calls.len(), 1);
        };
    }

    #[test]
    fn test_convenience_functions_integration() {
        // This test would verify the convenience functions work properly
        // In a real test environment, we'd set up proper test fixtures

        let mock_port = Box::new(MockMetricsPort::new());
        set_metrics_port(mock_port);

        // Test that functions don't panic and execute properly
        assert!(emit_counter("test_counter", 1.0).is_ok());
        assert!(emit_histogram("test_histogram", 2.0).is_ok());

        clear_metrics_port();
    }
}
