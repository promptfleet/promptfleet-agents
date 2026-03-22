use std::sync::{Arc, Mutex};
use std::time::Duration;

use agent_sdk::a2a::{a2a_sse_stream, map_trace_to_stream_response, A2aSseContext};
use agent_sdk::agent::engine::{
    EngineConfig, EngineError, EngineResult, RequestResponseTurnInvoker, StreamingTurnInvoker,
};
use agent_sdk::agent::tools::ToolRegistry;
use agent_sdk::agent::trace::AgentTraceEvent;
use agent_sdk::agui::{agent_io_sse_stream, map_trace_to_agent_io, AgentIoEvent, IoEventContext};
use axum::response::IntoResponse;
use serde_json::Value;

use crate::scenario::LlmScenario;
use crate::sse::{SseCapture, SseCollector};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapperKind {
    AgentIo,
    A2aSse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvokerMode {
    Streaming,
    RequestResponse,
}

#[derive(Debug, Clone)]
pub struct TestPipeline {
    scenario: LlmScenario,
    tools: ToolRegistry,
    mapper: MapperKind,
    invoker_mode: InvokerMode,
    model: String,
    messages: Vec<serde_json::Value>,
    io_ctx: IoEventContext,
    a2a_ctx: A2aSseContext,
    sse_timeout: Duration,
}

impl TestPipeline {
    pub fn new() -> TestPipelineBuilder {
        TestPipelineBuilder::default()
    }

    pub async fn run(self) -> Result<PipelineResult, String> {
        let recorder = TraceRecorder::default();
        let mut messages = self.messages;
        let config = EngineConfig::default();

        let engine_result = match self.invoker_mode {
            InvokerMode::Streaming => {
                let stream_invoker = self.scenario.clone().into_stream_invoker();
                let sink_recorder = recorder.clone();
                let sink: Arc<dyn Fn(AgentTraceEvent) + Send + Sync> = Arc::new(move |event| {
                    sink_recorder.push(event);
                });
                let invoker = StreamingTurnInvoker::new(stream_invoker, sink);
                let on_event = |event: AgentTraceEvent| recorder.push(event);
                agent_sdk::agent::engine::test_support::execute_messages_with_turn_invoker(
                    &invoker,
                    &self.model,
                    &self.tools,
                    &config,
                    &mut messages,
                    &on_event,
                )
                .await
            }
            InvokerMode::RequestResponse => {
                let request_invoker = self.scenario.clone().into_request_response_invoker();
                let invoker = RequestResponseTurnInvoker::new(request_invoker);
                let on_event = |event: AgentTraceEvent| recorder.push(event);
                agent_sdk::agent::engine::test_support::execute_messages_with_turn_invoker(
                    &invoker,
                    &self.model,
                    &self.tools,
                    &config,
                    &mut messages,
                    &on_event,
                )
                .await
            }
        };

        let trace_events = recorder.into_vec();

        let mut mapped_agent_io = Vec::new();
        let mut mapped_a2a_sse = Vec::new();

        match self.mapper {
            MapperKind::AgentIo => {
                for event in trace_events.iter().cloned() {
                    mapped_agent_io.extend(map_trace_to_agent_io(event, &self.io_ctx));
                }
            }
            MapperKind::A2aSse => {
                for event in trace_events.iter().cloned() {
                    mapped_a2a_sse.extend(map_trace_to_stream_response(event, &self.a2a_ctx));
                }
            }
        }

        let sse_capture = match self.mapper {
            MapperKind::AgentIo => {
                let response = agent_io_sse_stream(futures::stream::iter(mapped_agent_io.clone()))
                    .into_response();
                Some(
                    SseCollector::from_response(response)
                        .with_timeout(self.sse_timeout)
                        .collect_all()
                        .await?,
                )
            }
            MapperKind::A2aSse => {
                let response =
                    a2a_sse_stream(futures::stream::iter(mapped_a2a_sse.clone())).into_response();
                Some(
                    SseCollector::from_response(response)
                        .with_timeout(self.sse_timeout)
                        .collect_all()
                        .await?,
                )
            }
        };

        Ok(PipelineResult {
            engine_result,
            trace_events,
            mapped_agent_io,
            mapped_a2a_sse,
            sse_capture,
        })
    }
}

#[derive(Debug, Clone)]
pub struct TestPipelineBuilder {
    scenario: Option<LlmScenario>,
    tools: ToolRegistry,
    mapper: MapperKind,
    invoker_mode: InvokerMode,
    model: String,
    messages: Vec<serde_json::Value>,
    io_ctx: IoEventContext,
    a2a_ctx: A2aSseContext,
    sse_timeout: Duration,
}

impl Default for TestPipelineBuilder {
    fn default() -> Self {
        Self {
            scenario: None,
            tools: ToolRegistry::new(),
            mapper: MapperKind::AgentIo,
            invoker_mode: InvokerMode::Streaming,
            model: "test-model".to_string(),
            messages: vec![serde_json::json!({"role": "user", "content": "test"})],
            io_ctx: IoEventContext {
                thread_id: "thread-test".to_string(),
                run_id: "run-test".to_string(),
                message_id: "msg-test".to_string(),
            },
            a2a_ctx: A2aSseContext {
                task_id: "task-test".to_string(),
                context_id: "ctx-test".to_string(),
                jsonrpc_id: serde_json::json!("req-test"),
            },
            sse_timeout: Duration::from_secs(5),
        }
    }
}

impl TestPipelineBuilder {
    pub fn with_scenario(mut self, scenario: LlmScenario) -> Self {
        self.scenario = Some(scenario);
        self
    }

    pub fn with_tools(mut self, tools: ToolRegistry) -> Self {
        self.tools = tools;
        self
    }

    pub fn with_mapper(mut self, mapper: MapperKind) -> Self {
        self.mapper = mapper;
        self
    }

    pub fn with_invoker_mode(mut self, mode: InvokerMode) -> Self {
        self.invoker_mode = mode;
        self
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }

    pub fn with_messages(mut self, messages: Vec<serde_json::Value>) -> Self {
        self.messages = messages;
        self
    }

    pub fn with_sse_timeout(mut self, timeout: Duration) -> Self {
        self.sse_timeout = timeout;
        self
    }

    pub fn build(self) -> Result<TestPipeline, String> {
        let scenario = self
            .scenario
            .ok_or_else(|| "TestPipelineBuilder: scenario is required".to_string())?;

        Ok(TestPipeline {
            scenario,
            tools: self.tools,
            mapper: self.mapper,
            invoker_mode: self.invoker_mode,
            model: self.model,
            messages: self.messages,
            io_ctx: self.io_ctx,
            a2a_ctx: self.a2a_ctx,
            sse_timeout: self.sse_timeout,
        })
    }
}

#[derive(Debug, Clone)]
pub struct PipelineResult {
    engine_result: Result<EngineResult, EngineError>,
    trace_events: Vec<AgentTraceEvent>,
    mapped_agent_io: Vec<AgentIoEvent>,
    mapped_a2a_sse: Vec<a2a_protocol_core::streaming::StreamResponse>,
    sse_capture: Option<SseCapture>,
}

impl PipelineResult {
    pub fn engine_result(&self) -> &Result<EngineResult, EngineError> {
        &self.engine_result
    }

    pub fn trace_events(&self) -> &[AgentTraceEvent] {
        &self.trace_events
    }

    pub fn mapped_events(&self) -> Vec<MappedEvent> {
        if !self.mapped_agent_io.is_empty() {
            return self
                .mapped_agent_io
                .iter()
                .cloned()
                .map(MappedEvent::AgentIo)
                .collect();
        }

        self.mapped_a2a_sse
            .iter()
            .cloned()
            .map(MappedEvent::A2aSse)
            .collect()
    }

    pub fn mapped_agent_io(&self) -> &[AgentIoEvent] {
        &self.mapped_agent_io
    }

    pub fn mapped_a2a_sse(&self) -> &[a2a_protocol_core::streaming::StreamResponse] {
        &self.mapped_a2a_sse
    }

    pub fn sse_frames(&self) -> Vec<(String, Value)> {
        self.sse_capture
            .as_ref()
            .map(|c| {
                c.frames
                    .iter()
                    .map(|f| {
                        (
                            f.event.clone(),
                            f.data_json
                                .clone()
                                .unwrap_or_else(|| serde_json::json!(f.data_raw)),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn http_capture(&self) -> Option<&SseCapture> {
        self.sse_capture.as_ref()
    }
}

#[derive(Debug, Clone)]
pub enum MappedEvent {
    AgentIo(AgentIoEvent),
    A2aSse(a2a_protocol_core::streaming::StreamResponse),
}

impl MappedEvent {
    pub fn wire_type(&self) -> &'static str {
        match self {
            Self::AgentIo(event) => event.wire_type(),
            Self::A2aSse(event) => event.event_name(),
        }
    }
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
    use crate::scenario::LlmScenario;
    use agent_sdk::agent::tools::{ToolExecutor, ToolSpec};
    use agent_sdk::agent::trace::AgentTraceEvent;
    use llm_client::StreamEvent;

    fn register_echo_tool(tools: &mut ToolRegistry) {
        tools.register(ToolSpec {
            name: "echo".to_string(),
            description: Some("Echo input".to_string()),
            parameters: serde_json::json!({"type":"object"}),
            strict: false,
            parallel_ok: false,
            executor: ToolExecutor::Simple(Arc::new(|args| {
                Box::pin(async move { Ok(serde_json::json!({"echoed": args})) })
            })),
        });
    }

    #[tokio::test]
    async fn test_pipeline_happy_path_text_flow() {
        let result = TestPipeline::new()
            .with_scenario(LlmScenario::single_text("hello"))
            .build()
            .expect("pipeline build")
            .run()
            .await
            .expect("pipeline run");

        assert!(result.engine_result().is_ok());
        assert!(result
            .trace_events()
            .iter()
            .any(|e| matches!(e, AgentTraceEvent::Completed { .. })));
        assert!(result.http_capture().is_some());
    }

    #[tokio::test]
    async fn test_pipeline_tool_call_flow() {
        let mut tools = ToolRegistry::new();
        register_echo_tool(&mut tools);

        let result = TestPipeline::new()
            .with_scenario(LlmScenario::tool_call_then_text(
                "echo",
                serde_json::json!({"text": "test"}),
                "done",
            ))
            .with_tools(tools)
            .build()
            .expect("pipeline build")
            .run()
            .await
            .expect("pipeline run");

        let mapped = result.mapped_events();
        assert!(mapped.iter().any(|e| e.wire_type() == "tool_call_result"));
    }

    #[tokio::test]
    async fn test_pipeline_tool_call_fragmented_args() {
        let scenario = LlmScenario::from_turns(vec![
            vec![
                StreamEvent::StreamStart {
                    id: Some("resp-1".to_string()),
                    model: Some("test".to_string()),
                },
                StreamEvent::ToolCallStart {
                    index: 0,
                    id: "call_frag".to_string(),
                    name: "echo".to_string(),
                },
                StreamEvent::ToolCallDelta {
                    index: 0,
                    arguments_delta: "{\"text\"".to_string(),
                },
                StreamEvent::ToolCallDelta {
                    index: 0,
                    arguments_delta: ":\"split\"}".to_string(),
                },
                StreamEvent::Done {
                    finish_reason: Some("tool_calls".to_string()),
                    usage: None,
                },
            ],
            vec![
                StreamEvent::StreamStart {
                    id: Some("resp-2".to_string()),
                    model: Some("test".to_string()),
                },
                StreamEvent::ContentDelta {
                    delta: "done".to_string(),
                },
                StreamEvent::Done {
                    finish_reason: Some("stop".to_string()),
                    usage: None,
                },
            ],
        ]);

        let mut tools = ToolRegistry::new();
        register_echo_tool(&mut tools);

        let result = TestPipeline::new()
            .with_scenario(scenario)
            .with_tools(tools)
            .build()
            .expect("pipeline build")
            .run()
            .await
            .expect("pipeline run");

        let arg_deltas = result
            .trace_events()
            .iter()
            .filter(|e| matches!(e, AgentTraceEvent::ToolCallArgsDelta { .. }))
            .count();
        assert_eq!(arg_deltas, 2, "fragmented args should emit two deltas");

        let completed = result
            .trace_events()
            .iter()
            .find_map(|e| match e {
                AgentTraceEvent::ToolCallCompleted { result, .. } => Some(result.clone()),
                _ => None,
            })
            .expect("tool result");
        assert_eq!(completed["echoed"]["text"], "split");
    }

    #[tokio::test]
    async fn test_pipeline_parallel_tool_calls() {
        let scenario = LlmScenario::from_turns(vec![
            vec![
                StreamEvent::StreamStart {
                    id: Some("resp-1".to_string()),
                    model: Some("test".to_string()),
                },
                StreamEvent::ToolCallStart {
                    index: 0,
                    id: "call_a".to_string(),
                    name: "echo".to_string(),
                },
                StreamEvent::ToolCallDelta {
                    index: 0,
                    arguments_delta: "{\"text\":\"a\"}".to_string(),
                },
                StreamEvent::ToolCallStart {
                    index: 1,
                    id: "call_b".to_string(),
                    name: "echo".to_string(),
                },
                StreamEvent::ToolCallDelta {
                    index: 1,
                    arguments_delta: "{\"text\":\"b\"}".to_string(),
                },
                StreamEvent::Done {
                    finish_reason: Some("tool_calls".to_string()),
                    usage: None,
                },
            ],
            vec![
                StreamEvent::StreamStart {
                    id: Some("resp-2".to_string()),
                    model: Some("test".to_string()),
                },
                StreamEvent::ContentDelta {
                    delta: "both done".to_string(),
                },
                StreamEvent::Done {
                    finish_reason: Some("stop".to_string()),
                    usage: None,
                },
            ],
        ]);

        let mut tools = ToolRegistry::new();
        register_echo_tool(&mut tools);

        let result = TestPipeline::new()
            .with_scenario(scenario)
            .with_tools(tools)
            .build()
            .expect("pipeline build")
            .run()
            .await
            .expect("pipeline run");

        let tool_results = result
            .trace_events()
            .iter()
            .filter_map(|e| match e {
                AgentTraceEvent::ToolCallCompleted { id, .. } => Some(id.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(tool_results.len(), 2);
        assert!(tool_results.contains(&"call_a".to_string()));
        assert!(tool_results.contains(&"call_b".to_string()));
    }

    #[tokio::test]
    async fn test_pipeline_reasoning_lifecycle_maps_to_agent_io() {
        let result = TestPipeline::new()
            .with_scenario(LlmScenario::new().turn(|t| {
                t.reasoning("think")
                    .reasoning(" harder")
                    .content("done")
                    .done("stop")
            }))
            .build()
            .expect("pipeline build")
            .run()
            .await
            .expect("pipeline run");

        let types: Vec<&str> = result
            .mapped_events()
            .iter()
            .map(|e| e.wire_type())
            .collect();
        assert_eq!(
            types,
            vec![
                "step_started",
                "reasoning_start",
                "reasoning_message_start",
                "reasoning_message_content",
                "reasoning_message_content",
                "reasoning_message_end",
                "reasoning_end",
                "text_message_content",
                "step_finished",
                "run_finished",
            ]
        );
    }

    #[tokio::test]
    async fn test_pipeline_mid_stream_error_maps_to_terminal_run_error() {
        let result = TestPipeline::new()
            .with_scenario(LlmScenario::mid_stream_error("partial", "upstream 502"))
            .build()
            .expect("pipeline build")
            .run()
            .await
            .expect("pipeline run");

        assert!(result.engine_result().is_err());
        let types: Vec<&str> = result
            .mapped_events()
            .iter()
            .map(|e| e.wire_type())
            .collect();
        assert_eq!(types.last().copied(), Some("run_error"));

        let capture = result.http_capture().expect("sse capture");
        capture.assert_terminal("run_event");
        let last_index = capture.frames.len().saturating_sub(1);
        capture.assert_json_path_eq(last_index, "type", serde_json::json!("run_error"));
    }

    #[tokio::test]
    async fn test_pipeline_a2a_mapper_preserves_task_status_envelope() {
        let result = TestPipeline::new()
            .with_mapper(MapperKind::A2aSse)
            .with_scenario(LlmScenario::single_text("hello"))
            .build()
            .expect("pipeline build")
            .run()
            .await
            .expect("pipeline run");

        let capture = result.http_capture().expect("sse capture");
        assert_eq!(
            capture.frames.first().map(|f| f.event.as_str()),
            Some("statusUpdate")
        );
        capture.assert_json_path_eq(0, "jsonrpc", serde_json::json!("2.0"));
        let last = capture.frames.len().saturating_sub(1);
        capture.assert_json_path_eq(
            last,
            "result.statusUpdate.status.state",
            serde_json::json!("TASK_STATE_COMPLETED"),
        );
    }

    #[tokio::test]
    async fn test_cross_invoker_parity_on_final_text() {
        let scenario =
            LlmScenario::tool_call_then_text("echo", serde_json::json!({"text": "parity"}), "done");

        let mut tools = ToolRegistry::new();
        register_echo_tool(&mut tools);

        let streaming = TestPipeline::new()
            .with_scenario(scenario.clone())
            .with_tools(tools.clone())
            .with_invoker_mode(InvokerMode::Streaming)
            .build()
            .expect("streaming pipeline build")
            .run()
            .await
            .expect("streaming pipeline run");

        let request = TestPipeline::new()
            .with_scenario(scenario)
            .with_tools(tools)
            .with_invoker_mode(InvokerMode::RequestResponse)
            .build()
            .expect("request pipeline build")
            .run()
            .await
            .expect("request pipeline run");

        let stream_text = streaming
            .engine_result()
            .as_ref()
            .expect("streaming result")
            .text
            .clone();
        let request_text = request
            .engine_result()
            .as_ref()
            .expect("request result")
            .text
            .clone();

        assert_eq!(stream_text, request_text);

        let stream_tool_results = streaming
            .trace_events()
            .iter()
            .filter_map(|event| match event {
                AgentTraceEvent::ToolCallCompleted {
                    name,
                    result,
                    success,
                    ..
                } => Some((name.clone(), result.clone(), *success)),
                _ => None,
            })
            .collect::<Vec<_>>();

        let request_tool_results = request
            .trace_events()
            .iter()
            .filter_map(|event| match event {
                AgentTraceEvent::ToolCallCompleted {
                    name,
                    result,
                    success,
                    ..
                } => Some((name.clone(), result.clone(), *success)),
                _ => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(stream_tool_results, request_tool_results);
    }

    #[tokio::test]
    async fn test_cross_invoker_parity_on_failure_contract() {
        let scenario = LlmScenario::mid_stream_error("partial", "upstream timeout");

        let streaming = TestPipeline::new()
            .with_scenario(scenario.clone())
            .with_invoker_mode(InvokerMode::Streaming)
            .build()
            .expect("streaming pipeline build")
            .run()
            .await
            .expect("streaming pipeline run");

        let request = TestPipeline::new()
            .with_scenario(scenario)
            .with_invoker_mode(InvokerMode::RequestResponse)
            .build()
            .expect("request pipeline build")
            .run()
            .await
            .expect("request pipeline run");

        let stream_err = streaming
            .engine_result()
            .as_ref()
            .err()
            .map(ToString::to_string)
            .expect("streaming failure");
        let request_err = request
            .engine_result()
            .as_ref()
            .err()
            .map(ToString::to_string)
            .expect("request-response failure");

        assert!(stream_err.contains("upstream timeout"));
        assert!(request_err.contains("upstream timeout"));

        let streaming_terminal = streaming
            .mapped_events()
            .last()
            .map(|e| e.wire_type())
            .expect("streaming terminal event");
        let request_terminal = request
            .mapped_events()
            .last()
            .map(|e| e.wire_type())
            .expect("request terminal event");
        assert_eq!(streaming_terminal, "run_error");
        assert_eq!(request_terminal, "run_error");
    }
}
