use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use agent_sdk::a2a::{a2a_sse_stream, map_trace_to_stream_response, A2aSseContext};
use agent_sdk::agent::engine::{
    EngineConfig, EngineError, EngineResult, RequestResponseTurnInvoker, StreamingTurnInvoker,
};
use agent_sdk::agent::skill::{SkillExecutionContext, SkillRegistry};
use agent_sdk::agent::tools::{ToolExecutionResult, ToolRegistry};
use agent_sdk::agent::trace::AgentTraceEvent;
use agent_sdk::agent::{MessageContext, TaskContext, ToolContext};
use agent_sdk::agent_core::{AgentMessage, ContentPart, Role};
use agent_sdk::agui::{agent_io_sse_stream, map_trace_to_agent_io, AgentIoEvent, IoEventContext};
use axum::response::IntoResponse;
use llm_client::ChatMessage;
use serde_json::{json, Value};

use crate::pipeline::InvokerMode;
use crate::scenario::LlmScenario;
use crate::sse::{SseCapture, SseCollector};

#[derive(Debug, Clone)]
pub struct EngineRunCapture {
    engine_result: Result<EngineResult, EngineError>,
    trace_events: Vec<AgentTraceEvent>,
}

impl EngineRunCapture {
    pub fn engine_result(&self) -> &Result<EngineResult, EngineError> {
        &self.engine_result
    }

    pub fn trace_events(&self) -> &[AgentTraceEvent] {
        &self.trace_events
    }
}

pub async fn execute_engine_scenario(
    scenario: LlmScenario,
    tools: ToolRegistry,
    invoker_mode: InvokerMode,
) -> Result<EngineRunCapture, String> {
    let recorder = TraceRecorder::default();
    let mut messages = default_engine_messages();
    let config = EngineConfig::default();
    let model = "benchmark-model";

    let engine_result = match invoker_mode {
        InvokerMode::Streaming => {
            let stream_invoker = scenario.into_stream_invoker();
            let sink_recorder = recorder.clone();
            let sink: Arc<dyn Fn(AgentTraceEvent) + Send + Sync> = Arc::new(move |event| {
                sink_recorder.push(event);
            });
            let invoker = StreamingTurnInvoker::new(stream_invoker, sink);
            let on_event = |event: AgentTraceEvent| recorder.push(event);
            agent_sdk::agent::engine::test_support::execute_messages_with_turn_invoker(
                &invoker,
                model,
                &tools,
                &config,
                &mut messages,
                &on_event,
            )
            .await
        }
        InvokerMode::RequestResponse => {
            let request_invoker = scenario.into_request_response_invoker();
            let invoker = RequestResponseTurnInvoker::new(request_invoker);
            let on_event = |event: AgentTraceEvent| recorder.push(event);
            agent_sdk::agent::engine::test_support::execute_messages_with_turn_invoker(
                &invoker,
                model,
                &tools,
                &config,
                &mut messages,
                &on_event,
            )
            .await
        }
    };

    Ok(EngineRunCapture {
        engine_result,
        trace_events: recorder.into_vec(),
    })
}

pub async fn execute_tool(
    tools: &ToolRegistry,
    name: &str,
    args: Value,
) -> Result<ToolExecutionResult, String> {
    tools.execute(name, args).await
}

pub async fn execute_tool_with_context(
    tools: &ToolRegistry,
    name: &str,
    args: Value,
    ctx: Option<ToolContext>,
) -> Result<ToolExecutionResult, String> {
    tools.execute_with_context(name, args, ctx).await
}

pub async fn execute_skill(
    registry: &SkillRegistry,
    skill_id: &str,
    parameters: Value,
) -> Result<Value, String> {
    registry.execute_skill(skill_id, &parameters).await
}

pub async fn execute_skill_with_context(
    registry: &SkillRegistry,
    skill_id: &str,
    parameters: Value,
    exec_ctx: &SkillExecutionContext,
) -> Result<Value, String> {
    registry
        .execute_skill_with_ctx(skill_id, &parameters, exec_ctx)
        .await
}

pub fn map_agent_io_events(
    trace_events: &[AgentTraceEvent],
    ctx: &IoEventContext,
) -> Vec<AgentIoEvent> {
    let mut mapped = Vec::new();
    for event in trace_events.iter().cloned() {
        mapped.extend(map_trace_to_agent_io(event, ctx));
    }
    mapped
}

pub fn map_a2a_sse_events(
    trace_events: &[AgentTraceEvent],
    ctx: &A2aSseContext,
) -> Vec<a2a_protocol_core::streaming::StreamResponse> {
    let mut mapped = Vec::new();
    for event in trace_events.iter().cloned() {
        mapped.extend(map_trace_to_stream_response(event, ctx));
    }
    mapped
}

pub async fn collect_agent_io_sse(
    events: Vec<AgentIoEvent>,
    timeout: Duration,
) -> Result<SseCapture, String> {
    let response = agent_io_sse_stream(futures::stream::iter(events)).into_response();
    SseCollector::from_response(response)
        .with_timeout(timeout)
        .collect_all()
        .await
}

pub async fn collect_a2a_sse(
    events: Vec<a2a_protocol_core::streaming::StreamResponse>,
    timeout: Duration,
) -> Result<SseCapture, String> {
    let response = a2a_sse_stream(futures::stream::iter(events)).into_response();
    SseCollector::from_response(response)
        .with_timeout(timeout)
        .collect_all()
        .await
}

pub fn default_io_context() -> IoEventContext {
    IoEventContext {
        thread_id: "perf-thread".to_string(),
        run_id: "perf-run".to_string(),
        message_id: "perf-message".to_string(),
    }
}

pub fn default_a2a_context() -> A2aSseContext {
    A2aSseContext {
        task_id: "perf-task".to_string(),
        context_id: "perf-context".to_string(),
        jsonrpc_id: json!("perf-request"),
    }
}

pub fn default_tool_context() -> ToolContext {
    ToolContext::new(Arc::new(|_| {}), Arc::new(AtomicBool::new(false)))
}

pub fn default_skill_execution_context() -> SkillExecutionContext {
    let runtime_message = AgentMessage::new(
        Role::User,
        vec![
            ContentPart::Text("benchmark skill invocation".to_string()),
            ContentPart::Data(json!({"skill": "perf"})),
        ],
    );
    let message_ctx = MessageContext::from_runtime_message(
        runtime_message,
        HashMap::new(),
        HashMap::new(),
        false,
        None,
    );
    let task_ctx = TaskContext::create_new(Some("perf-context".to_string()));
    SkillExecutionContext::new(message_ctx, Some(task_ctx))
}

fn default_engine_messages() -> Vec<ChatMessage> {
    vec![ChatMessage {
        role: "user".into(),
        content: Some("benchmark".into()),
        ..Default::default()
    }]
}

#[derive(Clone, Default)]
struct TraceRecorder {
    events: Arc<Mutex<Vec<AgentTraceEvent>>>,
}

impl TraceRecorder {
    fn push(&self, event: AgentTraceEvent) {
        self.events
            .lock()
            .expect("trace recorder lock poisoned")
            .push(event);
    }

    fn into_vec(self) -> Vec<AgentTraceEvent> {
        self.events
            .lock()
            .expect("trace recorder lock poisoned")
            .clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_sdk::agent::skill::SkillDefinition;
    use agent_sdk::agent::tools::{ToolExecutor, ToolSpec};

    fn test_tools() -> ToolRegistry {
        let mut tools = ToolRegistry::new();
        tools.register(ToolSpec {
            name: "echo".to_string(),
            description: Some("Echo tool".to_string()),
            parameters: json!({"type":"object"}),
            strict: false,
            parallel_ok: false,
            executor: ToolExecutor::Simple(Arc::new(|args| {
                Box::pin(async move { Ok(json!({"echoed": args})) })
            })),
        });
        tools
    }

    fn test_skills() -> SkillRegistry {
        let mut skills = SkillRegistry::new();
        skills
            .skill(
                "lookup",
                |params| async move { Ok(json!({"result": params})) },
            )
            .llm_callable(true)
            .register()
            .expect("register lookup skill");
        skills
            .register_contextual_skill_for_test(
                "lookup_ctx",
                |params, exec_ctx| async move {
                    Ok(json!({
                        "result": params,
                        "has_task": exec_ctx.task_ctx.is_some(),
                    }))
                },
                SkillDefinition {
                    id: "lookup_ctx".to_string(),
                    name: "lookup_ctx".to_string(),
                    description: "Context-aware lookup".to_string(),
                    input_modes: vec!["application/json".to_string()],
                    output_modes: vec!["application/json".to_string()],
                    schema: None,
                    examples: None,
                    tags: None,
                    instructions: None,
                    expose: true,
                    llm_callable: false,
                },
            )
            .expect("register contextual skill");
        skills
    }

    #[tokio::test]
    async fn test_execute_engine_scenario_and_mapping_helpers() {
        let capture = execute_engine_scenario(
            LlmScenario::tool_call_then_text("echo", json!({"q": "perf"}), "done"),
            test_tools(),
            InvokerMode::Streaming,
        )
        .await
        .expect("engine capture");

        assert!(capture.engine_result().is_ok());

        let agent_io = map_agent_io_events(capture.trace_events(), &default_io_context());
        assert!(!agent_io.is_empty());

        let sse = collect_agent_io_sse(agent_io, Duration::from_secs(1))
            .await
            .expect("collect AG-UI SSE");
        assert!(!sse.frames.is_empty());
    }

    #[tokio::test]
    async fn test_execute_tool_and_skill_helpers() {
        let tool_result = execute_tool(&test_tools(), "echo", json!({"value": 1}))
            .await
            .expect("tool result");
        assert_eq!(tool_result.output["echoed"]["value"], json!(1));

        let skills = test_skills();
        let skill_result = execute_skill(&skills, "lookup", json!({"value": 2}))
            .await
            .expect("skill result");
        assert_eq!(skill_result["result"]["value"], json!(2));
    }

    #[tokio::test]
    async fn test_execute_skill_with_context_helper() {
        let skills = test_skills();
        let exec_ctx = default_skill_execution_context();
        let result =
            execute_skill_with_context(&skills, "lookup_ctx", json!({"value": 3}), &exec_ctx)
                .await
                .expect("skill with context result");

        assert_eq!(result["result"]["value"], json!(3));
        assert_eq!(result["has_task"], json!(true));
    }
}
