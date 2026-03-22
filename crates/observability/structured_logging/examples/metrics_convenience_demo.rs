use structured_logging::{
    clear_all_contexts,
    emit_a2a_message_latency,
    emit_counter,
    emit_histogram,
    // Metrics convenience functions
    emit_llm_request_duration,
    emit_llm_tokens_used,
    emit_request_duration,
    emit_template_render_duration,
    set_a2a_context,
    // Context management
    set_llm_context,
    set_request_context,
    set_template_context,
    Result,
};

fn main() -> Result<()> {
    println!("🎯 Structured Logging Metrics Convenience Demo");

    // Clear any existing contexts
    clear_all_contexts();

    println!("\n📊 Testing metrics without context:");

    // Basic metrics without context
    emit_counter("basic_requests", 1.0)?;
    emit_histogram("basic_response_time_ms", 150.0)?;

    println!("\n🤖 Testing LLM context with metrics:");

    // Set LLM context
    set_llm_context("gpt-4", "chat_completion");

    // Log with LLM context (simulated)
    println!("LOG: Processing LLM request model=gpt-4 tokens=150");

    // Emit LLM-specific metrics (automatically includes context)
    emit_llm_request_duration("gpt-4", 2500)?;
    emit_llm_tokens_used("gpt-4", 150)?;

    // Generic metrics will pick up LLM context
    emit_counter("llm_requests_processed", 1.0)?;
    emit_histogram("llm_processing_time_ms", 2500.0)?;

    println!("\n🔄 Testing A2A context with metrics:");

    // Set A2A context
    set_a2a_context(
        "weather_request",
        "weather-agent",
        "chat-agent",
        "a2a_handler",
    );

    // Log with A2A context (simulated)
    println!("LOG: Sending A2A message from=weather-agent to=chat-agent");

    // A2A-specific metrics
    emit_a2a_message_latency(85)?;

    // Generic metrics will pick up both LLM and A2A context
    emit_counter("messages_sent", 1.0)?;

    println!("\n📝 Testing template context with metrics:");

    // Set template context
    set_template_context("sailfish", "weather_response.stpl", "response_generator");

    // Log with template context (simulated)
    println!("LOG: Rendering template engine=sailfish template=weather_response.stpl");

    // Template-specific metrics
    emit_template_render_duration("weather_response.stpl", 1250)?;

    // Generic metrics now have LLM + A2A + Template context
    emit_histogram("template_complexity_score", 7.5)?;

    println!("\n🌐 Testing request context with metrics:");

    // Set request context
    set_request_context("req-abc-123", Some("user-def-456"), Some("session-ghi-789"));

    // Request-specific metrics
    emit_request_duration(3500)?;

    // Final generic metric with all contexts
    emit_counter("requests_completed", 1.0)?;

    // Clear all contexts
    clear_all_contexts();

    println!("\n🧹 Testing after context clearing:");

    // Metrics without context after clearing
    emit_counter("cleanup_operations", 1.0)?;

    println!("\n✅ Metrics convenience demo completed!");

    Ok(())
}
