use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};

use agent_sdk::agent::llm_orchestrator::{LlmInvoker, LlmStreamFuture, LlmStreamInvoker};
use llm_client::{
    ChatMessage, LlmChoice, LlmRequest, LlmResponse, ToolCallRequest, Usage, stream::StreamEvent,
};
use serde_json::{Value, json};

#[derive(Debug, Clone, Default)]
pub struct LlmScenario {
    turns: Vec<Vec<StreamEvent>>,
}

impl LlmScenario {
    pub fn new() -> Self {
        Self { turns: Vec::new() }
    }

    pub fn from_turns(turns: Vec<Vec<StreamEvent>>) -> Self {
        Self { turns }
    }

    pub fn turn(mut self, build: impl FnOnce(TurnScenarioBuilder) -> TurnScenarioBuilder) -> Self {
        let builder = build(TurnScenarioBuilder::new());
        self.turns.push(builder.build());
        self
    }

    pub fn single_text(text: impl Into<String>) -> Self {
        Self::new().turn(|t| t.content(text).done("stop"))
    }

    pub fn single_tool_call(name: impl Into<String>, arguments: Value) -> Self {
        Self::new().turn(|t| t.tool_call(name, arguments).done("tool_calls"))
    }

    pub fn tool_call_then_text(
        name: impl Into<String>,
        arguments: Value,
        text: impl Into<String>,
    ) -> Self {
        let name = name.into();
        Self::new()
            .turn(|t| t.tool_call(name.clone(), arguments).done("tool_calls"))
            .turn(|t| t.content(text).done("stop"))
    }

    pub fn mid_stream_error(partial_text: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new().turn(|t| t.content(partial_text).error(message))
    }

    pub fn turns(&self) -> &[Vec<StreamEvent>] {
        &self.turns
    }

    pub fn into_stream(self) -> impl futures::Stream<Item = StreamEvent> + Send {
        let all: Vec<StreamEvent> = self.turns.into_iter().flatten().collect();
        futures::stream::iter(all)
    }

    pub fn into_stream_invoker(self) -> Arc<dyn LlmStreamInvoker> {
        Arc::new(ScenarioStreamInvoker::new(self.turns))
    }

    pub fn into_request_response_invoker(self) -> Arc<dyn LlmInvoker> {
        Arc::new(ScenarioRequestInvoker::new(self.turns))
    }
}

#[derive(Debug, Clone)]
pub struct TurnScenarioBuilder {
    events: Vec<StreamEvent>,
    next_tool_index: u32,
}

impl TurnScenarioBuilder {
    fn new() -> Self {
        Self {
            events: Vec::new(),
            next_tool_index: 0,
        }
    }

    pub fn stream_start(mut self, id: impl Into<String>, model: impl Into<String>) -> Self {
        self.events.push(StreamEvent::StreamStart {
            id: Some(id.into()),
            model: Some(model.into()),
        });
        self
    }

    pub fn reasoning(mut self, delta: impl Into<String>) -> Self {
        self.events.push(StreamEvent::ReasoningDelta {
            delta: delta.into(),
        });
        self
    }

    pub fn content(mut self, delta: impl Into<String>) -> Self {
        self.events.push(StreamEvent::ContentDelta {
            delta: delta.into(),
        });
        self
    }

    pub fn tool_call(mut self, name: impl Into<String>, arguments: Value) -> Self {
        let index = self.next_tool_index;
        self.next_tool_index += 1;
        let id = format!("call_{index}");
        self.events.push(StreamEvent::ToolCallStart {
            index,
            id,
            name: name.into(),
        });
        self.events.push(StreamEvent::ToolCallDelta {
            index,
            arguments_delta: arguments.to_string(),
        });
        self
    }

    pub fn tool_call_with(
        mut self,
        index: u32,
        id: impl Into<String>,
        name: impl Into<String>,
        arguments: Value,
    ) -> Self {
        self.next_tool_index = self.next_tool_index.max(index + 1);
        self.events.push(StreamEvent::ToolCallStart {
            index,
            id: id.into(),
            name: name.into(),
        });
        self.events.push(StreamEvent::ToolCallDelta {
            index,
            arguments_delta: arguments.to_string(),
        });
        self
    }

    /// Emit a tool call with arguments split into `fragment_count` deltas.
    ///
    /// Simulates how real LLMs stream JSON arguments in multiple chunks.
    pub fn tool_call_fragmented(
        mut self,
        name: impl Into<String>,
        arguments: Value,
        fragment_count: usize,
    ) -> Self {
        let index = self.next_tool_index;
        self.next_tool_index += 1;
        let id = format!("call_{index}");
        self.events.push(StreamEvent::ToolCallStart {
            index,
            id,
            name: name.into(),
        });
        let serialized = arguments.to_string();
        let chunk_size = (serialized.len() + fragment_count - 1) / fragment_count.max(1);
        for chunk in serialized.as_bytes().chunks(chunk_size) {
            let delta = String::from_utf8_lossy(chunk).to_string();
            self.events.push(StreamEvent::ToolCallDelta {
                index,
                arguments_delta: delta,
            });
        }
        self
    }

    pub fn tool_call_delta(mut self, index: u32, arguments_delta: impl Into<String>) -> Self {
        self.events.push(StreamEvent::ToolCallDelta {
            index,
            arguments_delta: arguments_delta.into(),
        });
        self
    }

    pub fn done(mut self, finish_reason: impl Into<String>) -> Self {
        self.events.push(StreamEvent::Done {
            finish_reason: Some(finish_reason.into()),
            usage: None,
        });
        self
    }

    pub fn done_with_usage(mut self, prompt_tokens: u32, completion_tokens: u32) -> Self {
        self.events.push(StreamEvent::Done {
            finish_reason: Some("stop".to_string()),
            usage: Some(Usage {
                prompt_tokens: Some(prompt_tokens),
                completion_tokens: Some(completion_tokens),
                total_tokens: Some(prompt_tokens + completion_tokens),
            }),
        });
        self
    }

    pub fn error(mut self, message: impl Into<String>) -> Self {
        self.events.push(StreamEvent::Error {
            message: message.into(),
        });
        self
    }

    fn build(mut self) -> Vec<StreamEvent> {
        if !matches!(self.events.first(), Some(StreamEvent::StreamStart { .. })) {
            self.events.insert(
                0,
                StreamEvent::StreamStart {
                    id: None,
                    model: None,
                },
            );
        }
        self.events
    }
}

#[derive(Debug)]
struct ScenarioStreamInvoker {
    turns: Mutex<VecDeque<Vec<StreamEvent>>>,
}

impl ScenarioStreamInvoker {
    fn new(turns: Vec<Vec<StreamEvent>>) -> Self {
        Self {
            turns: Mutex::new(turns.into()),
        }
    }
}

impl LlmStreamInvoker for ScenarioStreamInvoker {
    fn request_stream(&self, _req: LlmRequest) -> LlmStreamFuture {
        let events = self
            .turns
            .lock()
            .expect("stream invoker lock poisoned")
            .pop_front()
            .unwrap_or_default();
        Box::pin(async move {
            let stream = futures::stream::iter(events.into_iter().map(Ok));
            Ok(Box::pin(stream) as llm_client::LlmEventStream)
        })
    }
}

#[derive(Debug)]
struct ScenarioRequestInvoker {
    turns: Mutex<VecDeque<Vec<StreamEvent>>>,
}

impl ScenarioRequestInvoker {
    fn new(turns: Vec<Vec<StreamEvent>>) -> Self {
        Self {
            turns: Mutex::new(turns.into()),
        }
    }
}

impl LlmInvoker for ScenarioRequestInvoker {
    fn request(
        &self,
        _req: LlmRequest,
    ) -> std::pin::Pin<Box<dyn core::future::Future<Output = Result<LlmResponse, String>> + Send>>
    {
        let turn = self
            .turns
            .lock()
            .expect("request invoker lock poisoned")
            .pop_front()
            .unwrap_or_default();
        Box::pin(async move { fold_stream_events_to_llm_response(&turn) })
    }
}

/// Fold scenario [`StreamEvent`]s into an [`LlmResponse`] (same semantics as the scenario
/// request/response invoker). Used by [`scenario_openai_http`](crate::scenario_openai_http)
/// to synthesize OpenAI Chat Completions JSON fixtures.
pub fn fold_stream_events_to_llm_response(turn: &[StreamEvent]) -> Result<LlmResponse, String> {
    #[derive(Debug)]
    struct ToolCallAcc {
        id: String,
        name: String,
        args: String,
    }

    let mut content = String::new();
    let mut usage: Option<Usage> = None;
    let mut finish_reason: Option<String> = None;
    let mut tools: BTreeMap<u32, ToolCallAcc> = BTreeMap::new();

    for event in turn {
        match event {
            StreamEvent::ContentDelta { delta } => content.push_str(delta),
            StreamEvent::ToolCallStart { index, id, name } => {
                tools.entry(*index).or_insert_with(|| ToolCallAcc {
                    id: id.clone(),
                    name: name.clone(),
                    args: String::new(),
                });
            }
            StreamEvent::ToolCallDelta {
                index,
                arguments_delta,
            } => {
                let entry = tools.entry(*index).or_insert_with(|| ToolCallAcc {
                    id: format!("call_{index}"),
                    name: format!("tool_{index}"),
                    args: String::new(),
                });
                entry.args.push_str(arguments_delta);
            }
            StreamEvent::Done {
                finish_reason: fr,
                usage: u,
            } => {
                finish_reason = fr.clone();
                usage = u.clone();
            }
            StreamEvent::Error { message } => return Err(message.clone()),
            StreamEvent::StreamStart { .. } | StreamEvent::ReasoningDelta { .. } => {}
        }
    }

    let tool_calls: Option<Vec<ToolCallRequest>> = if tools.is_empty() {
        None
    } else {
        Some(
            tools
                .into_iter()
                .map(|(_index, tc)| {
                    let arguments = serde_json::from_str(&tc.args)
                        .unwrap_or_else(|_| json!({ "_raw": tc.args }));
                    ToolCallRequest {
                        id: tc.id,
                        name: tc.name,
                        arguments,
                    }
                })
                .collect(),
        )
    };

    Ok(LlmResponse {
        id: None,
        created: None,
        model: None,
        choices: vec![LlmChoice {
            index: 0,
            message: ChatMessage {
                role: "assistant".into(),
                content: if content.is_empty() {
                    None
                } else {
                    Some(content)
                },
                tool_calls,
                ..Default::default()
            },
            finish_reason,
        }],
        usage,
        tool_calls: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;

    #[tokio::test]
    async fn test_builder_emits_expected_stream_events() {
        let scenario = LlmScenario::new().turn(|t| {
            t.reasoning("think")
                .tool_call("search", json!({"q": "rust"}))
                .done("tool_calls")
        });

        let turn = &scenario.turns()[0];
        assert!(matches!(turn[0], StreamEvent::StreamStart { .. }));
        assert!(matches!(turn[1], StreamEvent::ReasoningDelta { .. }));
        assert!(matches!(turn[2], StreamEvent::ToolCallStart { .. }));
        assert!(matches!(turn[3], StreamEvent::ToolCallDelta { .. }));
        assert!(matches!(turn[4], StreamEvent::Done { .. }));
    }

    #[tokio::test]
    async fn test_request_response_conversion_is_deterministic() {
        let scenario = LlmScenario::tool_call_then_text("echo", json!({"text": "x"}), "done");
        let invoker = scenario.into_request_response_invoker();

        let first = invoker
            .request(LlmRequest::default())
            .await
            .expect("first turn response");
        let second = invoker
            .request(LlmRequest::default())
            .await
            .expect("second turn response");

        assert_eq!(
            first
                .choices
                .first()
                .and_then(|c| c.finish_reason.as_deref()),
            Some("tool_calls")
        );
        let tc0 = first
            .choices
            .first()
            .and_then(|c| c.message.tool_calls.as_ref())
            .and_then(|t| t.first());
        assert_eq!(tc0.map(|t| t.name.as_str()), Some("echo"));
        assert_eq!(
            second
                .choices
                .first()
                .and_then(|c| c.message.content.as_deref()),
            Some("done")
        );
    }

    #[tokio::test]
    async fn test_into_stream_flattens_turns() {
        let scenario = LlmScenario::single_text("hello").turn(|t| t.content("world").done("stop"));
        let mut stream = scenario.into_stream();
        let mut count = 0usize;
        while stream.next().await.is_some() {
            count += 1;
        }
        assert!(count >= 4);
    }

    #[test]
    fn test_tool_call_fragmented_splits_into_n_deltas() {
        let args = json!({"query": "rust async"});
        let scenario = LlmScenario::new().turn(|t| {
            t.tool_call_fragmented("search", args.clone(), 3)
                .done("tool_calls")
        });
        let turn = &scenario.turns()[0];

        assert!(matches!(turn[1], StreamEvent::ToolCallStart { .. }));

        let deltas: Vec<&StreamEvent> = turn
            .iter()
            .filter(|e| matches!(e, StreamEvent::ToolCallDelta { .. }))
            .collect();
        assert_eq!(
            deltas.len(),
            3,
            "expected 3 fragments, got {}",
            deltas.len()
        );

        let mut reassembled = String::new();
        for d in &deltas {
            if let StreamEvent::ToolCallDelta {
                arguments_delta, ..
            } = d
            {
                reassembled.push_str(arguments_delta);
            }
        }
        let parsed: Value = serde_json::from_str(&reassembled)
            .expect("fragments should reassemble into valid JSON");
        assert_eq!(parsed, args);
    }

    #[test]
    fn test_tool_call_fragmented_single_chunk() {
        let args = json!({"x": 1});
        let scenario =
            LlmScenario::new().turn(|t| t.tool_call_fragmented("fn", args, 1).done("tool_calls"));
        let turn = &scenario.turns()[0];
        let deltas = turn
            .iter()
            .filter(|e| matches!(e, StreamEvent::ToolCallDelta { .. }))
            .count();
        assert_eq!(deltas, 1);
    }
}
