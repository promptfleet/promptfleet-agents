use crate::agent::tools::ToolRegistry;
use crate::agent::trace::AgentTraceEvent;
use llm_client::ChatMessage;

use super::{EngineConfig, EngineError, EngineResult, LlmTurnInvoker, core_loop};

/// Test-support wrapper around the engine core loop.
///
/// This exposes seam-level execution for reusable test harness crates without
/// making `core_loop` public API.
pub async fn execute_messages_with_turn_invoker<F: Fn(AgentTraceEvent)>(
    invoker: &dyn LlmTurnInvoker,
    model: &str,
    tools: &ToolRegistry,
    config: &EngineConfig,
    messages: &mut Vec<ChatMessage>,
    on_event: &F,
) -> Result<EngineResult, EngineError> {
    core_loop::execute(
        invoker, model, tools, config, messages, on_event, None, None, None,
    )
    .await
}
