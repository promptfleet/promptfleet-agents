use std::collections::VecDeque;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll};

use futures::Stream;
use tokio::sync::oneshot;

use crate::agent::trace::AgentTraceEvent;

use super::{
    AgentIoEvent, Interrupt, IoEventContext, RunFinishedOutcome, map_trace_to_agent_io,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunStatus {
    Completed,
    Failed,
    Cancelled,
    InputRequired,
}

impl RunStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::InputRequired => "input_required",
        }
    }
}

#[derive(Debug, Clone)]
pub struct RunSummary {
    pub full_text: String,
    pub status: RunStatus,
    pub tool_calls_count: u32,
    pub usage: Option<llm_client::Usage>,
}

impl Default for RunSummary {
    fn default() -> Self {
        Self {
            full_text: String::new(),
            status: RunStatus::Completed,
            tool_calls_count: 0,
            usage: None,
        }
    }
}

pub trait StreamEnricher: Send + Sync {
    fn enrich(&self, event: &AgentTraceEvent, ctx: &IoEventContext) -> Vec<AgentIoEvent>;
}

pub struct AgUiDriverConfig {
    pub ctx: IoEventContext,
    pub cancel_flag: Option<Arc<AtomicBool>>,
    pub enrichers: Vec<Box<dyn StreamEnricher>>,
    pub initial_state: serde_json::Value,
    pub initial_messages: Vec<serde_json::Value>,
}

pub struct AgUiStreamDriver {
    config: AgUiDriverConfig,
}

impl AgUiStreamDriver {
    pub fn new(config: AgUiDriverConfig) -> Self {
        Self { config }
    }

    pub fn drive<S>(self, trace_stream: S) -> AgUiStream
    where
        S: Stream<Item = AgentTraceEvent> + Send + 'static,
    {
        AgUiStream {
            trace_stream: Box::pin(trace_stream),
            ctx: self.config.ctx,
            cancel_flag: self.config.cancel_flag,
            enrichers: self.config.enrichers,
            pending: VecDeque::new(),
            summary: RunSummary::default(),
            started: false,
            text_message_started: false,
            text_message_ended: false,
            finished: false,
            cancellation_emitted: false,
            initial_state: self.config.initial_state,
            snapshot_messages: self.config.initial_messages,
            snapshot_turn_active: false,
            snapshot_turn_index: 0,
            snapshot_assistant_content: String::new(),
            snapshot_tool_calls: Vec::new(),
            snapshot_tool_results: Vec::new(),
            pending_interrupts: Vec::new(),
        }
    }
}

pub struct AgUiStream {
    trace_stream: Pin<Box<dyn Stream<Item = AgentTraceEvent> + Send>>,
    ctx: IoEventContext,
    cancel_flag: Option<Arc<AtomicBool>>,
    enrichers: Vec<Box<dyn StreamEnricher>>,
    pending: VecDeque<AgentIoEvent>,
    summary: RunSummary,
    started: bool,
    text_message_started: bool,
    text_message_ended: bool,
    finished: bool,
    cancellation_emitted: bool,
    initial_state: serde_json::Value,
    snapshot_messages: Vec<serde_json::Value>,
    snapshot_turn_active: bool,
    snapshot_turn_index: u32,
    snapshot_assistant_content: String,
    snapshot_tool_calls: Vec<serde_json::Value>,
    snapshot_tool_results: Vec<serde_json::Value>,
    pending_interrupts: Vec<Interrupt>,
}

impl AgUiStream {
    pub fn summary(&self) -> Option<RunSummary> {
        if self.finished {
            Some(self.summary.clone())
        } else {
            None
        }
    }

    pub fn into_summary(self) -> RunSummary {
        self.summary
    }

    pub fn full_text_so_far(&self) -> String {
        self.summary.full_text.clone()
    }

    fn maybe_emit_cancelled(&mut self) {
        if self.finished || self.cancellation_emitted {
            return;
        }
        let Some(flag) = &self.cancel_flag else {
            return;
        };
        if !flag.load(Ordering::Relaxed) {
            return;
        }

        self.summary.status = RunStatus::Cancelled;
        self.maybe_emit_text_message_end();
        self.pending.push_back(AgentIoEvent::Custom {
            name: "run_cancelled".to_string(),
            value: serde_json::json!({
                "runId": self.ctx.run_id,
                "threadId": self.ctx.thread_id,
            }),
        });
        self.finished = true;
        self.cancellation_emitted = true;
    }

    fn update_summary(&mut self, event: &AgentTraceEvent) {
        match event {
            AgentTraceEvent::ContentDelta { delta } => {
                self.summary.full_text.push_str(delta);
            }
            AgentTraceEvent::TurnCompleted { finish_reason, .. } => {
                if finish_reason.as_deref() == Some("input_required")
                    && self.summary.status == RunStatus::Completed
                {
                    self.summary.status = RunStatus::InputRequired;
                }
            }
            AgentTraceEvent::InteractionRequested { .. } => {
                if self.summary.status == RunStatus::Completed {
                    self.summary.status = RunStatus::InputRequired;
                }
            }
            AgentTraceEvent::Failed { .. } => {
                if self.summary.status != RunStatus::Cancelled {
                    self.summary.status = RunStatus::Failed;
                }
            }
            AgentTraceEvent::ToolCallCompleted { .. } => {
                self.summary.tool_calls_count += 1;
            }
            AgentTraceEvent::Completed { usage, .. } => {
                self.summary.usage = usage.clone();
            }
            _ => {}
        }
    }

    fn maybe_emit_text_message_start(&mut self) {
        if self.text_message_started {
            return;
        }
        self.text_message_started = true;
        self.pending.push_back(AgentIoEvent::TextMessageStart {
            message_id: self.ctx.message_id.clone(),
            role: "assistant".to_string(),
        });
    }

    fn maybe_emit_text_message_end(&mut self) {
        if !self.text_message_started || self.text_message_ended {
            return;
        }
        self.text_message_ended = true;
        self.pending.push_back(AgentIoEvent::TextMessageEnd {
            message_id: self.ctx.message_id.clone(),
        });
    }

    fn begin_snapshot_turn(&mut self) {
        if self.snapshot_turn_active {
            self.finish_snapshot_turn();
        }
        self.snapshot_turn_active = true;
        self.snapshot_turn_index += 1;
    }

    fn finish_snapshot_turn(&mut self) {
        if !self.snapshot_turn_active {
            return;
        }
        if !self.snapshot_assistant_content.is_empty() || !self.snapshot_tool_calls.is_empty() {
            let message_id = if self.snapshot_turn_index == 1 {
                self.ctx.message_id.clone()
            } else {
                format!("{}-turn-{}", self.ctx.message_id, self.snapshot_turn_index)
            };
            let content = if self.snapshot_assistant_content.is_empty() {
                serde_json::Value::Null
            } else {
                serde_json::Value::String(std::mem::take(
                    &mut self.snapshot_assistant_content,
                ))
            };
            self.snapshot_messages.push(serde_json::json!({
                "id": message_id,
                "role": "assistant",
                "content": content,
                "toolCalls": std::mem::take(&mut self.snapshot_tool_calls),
            }));
        } else {
            self.snapshot_assistant_content.clear();
            self.snapshot_tool_calls.clear();
        }
        self.snapshot_messages
            .append(&mut self.snapshot_tool_results);
        self.snapshot_turn_active = false;
    }

    fn capture_snapshot_event(&mut self, event: &AgentTraceEvent) {
        match event {
            AgentTraceEvent::TurnStarted { .. } => self.begin_snapshot_turn(),
            AgentTraceEvent::ContentDelta { delta } => {
                if !self.snapshot_turn_active {
                    self.begin_snapshot_turn();
                }
                self.snapshot_assistant_content.push_str(delta);
            }
            AgentTraceEvent::ToolCallStarted { id, name, .. } => {
                if !self.snapshot_turn_active {
                    self.begin_snapshot_turn();
                }
                self.snapshot_tool_calls.push(serde_json::json!({
                    "id": id,
                    "type": "function",
                    "function": {
                        "name": name,
                        "arguments": {},
                    },
                }));
            }
            AgentTraceEvent::ToolCallArgsCompleted {
                id,
                name,
                arguments,
                ..
            } => {
                if let Some(tool_call) = self
                    .snapshot_tool_calls
                    .iter_mut()
                    .find(|tool_call| tool_call["id"].as_str() == Some(id.as_str()))
                {
                    tool_call["function"]["name"] = serde_json::json!(name);
                    tool_call["function"]["arguments"] = arguments.clone();
                }
            }
            AgentTraceEvent::ToolCallCompleted {
                id,
                name,
                result,
                success,
                ..
            } => {
                let result_message = serde_json::json!({
                    "id": format!("{}-tool-{}", self.ctx.message_id, id),
                    "role": "tool",
                    "toolCallId": id,
                    "name": name,
                    "content": result,
                    "error": if *success {
                        serde_json::Value::Null
                    } else {
                        serde_json::Value::String(result.to_string())
                    },
                });
                if self.snapshot_turn_active {
                    self.snapshot_tool_results.push(result_message);
                } else {
                    self.snapshot_messages.push(result_message);
                }
            }
            AgentTraceEvent::TurnCompleted { .. } | AgentTraceEvent::Completed { .. } => {
                self.finish_snapshot_turn();
            }
            _ => {}
        }
    }

    fn enqueue_events(&mut self, event: AgentTraceEvent) {
        self.update_summary(&event);
        self.capture_snapshot_event(&event);
        if matches!(event, AgentTraceEvent::ContentDelta { .. }) {
            self.maybe_emit_text_message_start();
        }
        if matches!(
            event,
            AgentTraceEvent::Completed { .. } | AgentTraceEvent::Failed { .. }
        ) {
            self.maybe_emit_text_message_end();
        }

        if let AgentTraceEvent::InteractionRequested { request } = &event {
            self.pending_interrupts.push(interrupt_from_request(request));
        }

        // AG-UI interrupts are terminal outcomes. Snapshot the state and messages
        // needed for replay before emitting the interrupting RUN_FINISHED.
        let mapped = if matches!(event, AgentTraceEvent::Completed { .. })
            && self.summary.status == RunStatus::InputRequired
        {
            vec![
                AgentIoEvent::StateSnapshot {
                    snapshot: self.initial_state.clone(),
                },
                AgentIoEvent::MessagesSnapshot {
                    messages: self.snapshot_messages.clone(),
                },
                AgentIoEvent::RunFinished {
                    thread_id: self.ctx.thread_id.clone(),
                    run_id: self.ctx.run_id.clone(),
                    result: None,
                    outcome: Some(RunFinishedOutcome::Interrupt {
                        interrupts: self.pending_interrupts.clone(),
                    }),
                },
            ]
        } else if matches!(event, AgentTraceEvent::InteractionRequested { .. }) {
            Vec::new()
        } else {
            map_trace_to_agent_io(event.clone(), &self.ctx)
        };

        for io_event in mapped {
            self.pending.push_back(io_event);
        }

        for enricher in &self.enrichers {
            for enriched in enricher.enrich(&event, &self.ctx) {
                self.pending.push_back(enriched);
            }
        }
    }
}

fn interrupt_from_request(request: &crate::interaction::InteractionRequest) -> Interrupt {
    let response_schema = match request.kind {
        crate::interaction::InteractionKind::Confirmation => serde_json::json!({
            "type": "object",
            "properties": {
                "approved": { "type": "boolean" }
            },
            "required": ["approved"],
            "additionalProperties": false
        }),
        crate::interaction::InteractionKind::Question => serde_json::json!({
            "type": "object",
            "properties": {
                "selectedOptionId": { "type": "string" },
                "freeText": { "type": "string" }
            },
            "anyOf": [
                { "required": ["selectedOptionId"] },
                { "required": ["freeText"] }
            ],
            "additionalProperties": false
        }),
    };
    let expires_at = request.timeout_ms.and_then(|timeout_ms| {
        chrono::Duration::try_milliseconds(timeout_ms as i64)
            .map(|duration| (chrono::Utc::now() + duration).to_rfc3339())
    });
    Interrupt {
        id: request.interaction_id.clone(),
        reason: match request.kind {
            crate::interaction::InteractionKind::Confirmation => "confirmation".to_string(),
            crate::interaction::InteractionKind::Question => "input_required".to_string(),
        },
        message: Some(request.question.clone()),
        tool_call_id: None,
        response_schema: Some(response_schema),
        expires_at,
        metadata: Some(serde_json::json!({ "promptfleet": request })),
    }
}

impl AgUiStream {
    /// Splits this stream into a plain event stream and a [`SummaryHandle`].
    ///
    /// The returned stream yields the same `AgentIoEvent` items. Once the stream
    /// completes, the [`SummaryHandle`] resolves to the accumulated [`RunSummary`].
    /// This enables passing the event stream to SSE (or any other sink) while still
    /// capturing the post-run summary asynchronously.
    pub fn with_summary_handle(
        self,
    ) -> (
        impl Stream<Item = AgentIoEvent> + Send + 'static,
        SummaryHandle,
    ) {
        let (tx, rx) = oneshot::channel();
        let tracked = TrackedAgUiStream {
            inner: self,
            tx: Some(tx),
        };
        (tracked, SummaryHandle { rx })
    }
}

impl Stream for AgUiStream {
    type Item = AgentIoEvent;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();

        if !this.started {
            this.started = true;
            return Poll::Ready(Some(AgentIoEvent::RunStarted {
                thread_id: this.ctx.thread_id.clone(),
                run_id: this.ctx.run_id.clone(),
            }));
        }

        loop {
            if let Some(next) = this.pending.pop_front() {
                return Poll::Ready(Some(next));
            }

            this.maybe_emit_cancelled();
            if let Some(next) = this.pending.pop_front() {
                return Poll::Ready(Some(next));
            }
            if this.finished {
                return Poll::Ready(None);
            }

            match this.trace_stream.as_mut().poll_next(cx) {
                Poll::Ready(Some(event)) => {
                    this.enqueue_events(event);
                    continue;
                }
                Poll::Ready(None) => {
                    this.maybe_emit_text_message_end();
                    if let Some(next) = this.pending.pop_front() {
                        return Poll::Ready(Some(next));
                    }
                    this.finished = true;
                    return Poll::Ready(None);
                }
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

// ---------------------------------------------------------------------------
// SummaryHandle — async awaitable run summary
// ---------------------------------------------------------------------------

/// Handle that resolves to the [`RunSummary`] once the associated stream completes.
pub struct SummaryHandle {
    rx: oneshot::Receiver<RunSummary>,
}

impl SummaryHandle {
    /// Await the final [`RunSummary`]. Returns `None` if the stream was dropped
    /// before completion (e.g. client disconnect).
    pub async fn get(self) -> Option<RunSummary> {
        self.rx.await.ok()
    }
}

// ---------------------------------------------------------------------------
// TrackedAgUiStream — forwards events and sends summary on completion
// ---------------------------------------------------------------------------

struct TrackedAgUiStream {
    inner: AgUiStream,
    tx: Option<oneshot::Sender<RunSummary>>,
}

impl Stream for TrackedAgUiStream {
    type Item = AgentIoEvent;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        match Pin::new(&mut this.inner).poll_next(cx) {
            Poll::Ready(None) => {
                if let Some(tx) = this.tx.take() {
                    if let Some(summary) = this.inner.summary() {
                        let _ = tx.send(summary);
                    }
                }
                Poll::Ready(None)
            }
            other => other,
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use futures::StreamExt;

    use crate::agent::trace::AgentTraceEvent;
    use crate::interaction::{InteractionKind, InteractionRequest};
    use crate::streaming::{AgentIoEvent, IoEventContext};

    use super::{AgUiDriverConfig, AgUiStreamDriver, RunStatus};

    fn test_ctx() -> IoEventContext {
        IoEventContext {
            thread_id: "thread-1".to_string(),
            run_id: "run-1".to_string(),
            message_id: "msg-1".to_string(),
        }
    }

    fn interaction_requested_event() -> AgentTraceEvent {
        AgentTraceEvent::InteractionRequested {
            request: InteractionRequest {
                interaction_id: "ix-1".to_string(),
                kind: InteractionKind::Confirmation,
                question: "Proceed?".to_string(),
                options: vec![],
                allow_free_text: true,
                allow_cancel: true,
                default_option_id: None,
                timeout_ms: None,
                continuation_id: None,
                source_node: None,
                metadata: None,
            },
        }
    }

    /// Returns the semantic event name: uses `name` field for `Custom` events,
    /// otherwise falls back to `wire_type()`.
    fn event_name(e: &AgentIoEvent) -> String {
        if let AgentIoEvent::Custom { name, .. } = e {
            name.clone()
        } else {
            e.wire_type().to_string()
        }
    }

    async fn collect_event_names(events: Vec<AgentTraceEvent>) -> Vec<String> {
        let stream = futures::stream::iter(events);
        let driver = AgUiStreamDriver::new(AgUiDriverConfig {
            ctx: test_ctx(),
            cancel_flag: None,
            enrichers: vec![],
            initial_state: serde_json::Value::Null,
            initial_messages: Vec::new(),
        });
        driver
            .drive(stream)
            .collect::<Vec<_>>()
            .await
            .iter()
            .map(event_name)
            .collect()
    }

    #[tokio::test]
    async fn driver_emits_run_started_first() {
        let events = vec![AgentTraceEvent::Completed {
            text: None,
            usage: None,
        }];
        let names = collect_event_names(events).await;
        assert_eq!(
            names.first().map(String::as_str),
            Some("RUN_STARTED"),
            "first event must be RUN_STARTED, got: {:?}",
            names
        );
    }

    #[tokio::test]
    async fn driver_emits_standard_interrupt_outcome_with_snapshots() {
        let events = vec![
            interaction_requested_event(),
            AgentTraceEvent::Completed {
                text: None,
                usage: None,
            },
        ];
        let names = collect_event_names(events).await;
        assert_eq!(
            names,
            vec![
                "RUN_STARTED",
                "STATE_SNAPSHOT",
                "MESSAGES_SNAPSHOT",
                "RUN_FINISHED"
            ]
        );
    }

    #[tokio::test]
    async fn driver_interrupt_binds_prompt_and_response_schema() {
        let stream = futures::stream::iter(vec![
            interaction_requested_event(),
            AgentTraceEvent::Completed {
                text: None,
                usage: None,
            },
        ]);
        let events = AgUiStreamDriver::new(AgUiDriverConfig {
            ctx: test_ctx(),
            cancel_flag: None,
            enrichers: vec![],
            initial_state: serde_json::json!({ "incidentId": "INC-1042" }),
            initial_messages: vec![serde_json::json!({
                "id": "user-1",
                "role": "user",
                "content": "Investigate"
            })],
        })
        .drive(stream)
        .collect::<Vec<_>>()
        .await;

        assert!(matches!(
            &events[1],
            AgentIoEvent::StateSnapshot { snapshot }
                if snapshot["incidentId"] == "INC-1042"
        ));
        assert!(matches!(
            &events[2],
            AgentIoEvent::MessagesSnapshot { messages }
                if messages.len() == 1 && messages[0]["id"] == "user-1"
        ));
        assert!(matches!(
            &events[3],
            AgentIoEvent::RunFinished {
                outcome: Some(super::RunFinishedOutcome::Interrupt { interrupts }),
                ..
            } if interrupts.len() == 1
                && interrupts[0].id == "ix-1"
                && interrupts[0].reason == "confirmation"
                && interrupts[0].response_schema.as_ref().is_some_and(|schema| {
                    schema["required"] == serde_json::json!(["approved"])
                })
        ));
    }

    #[tokio::test]
    async fn test_driver_interrupt_snapshot_preserves_tool_call_and_pending_result() {
        let stream = futures::stream::iter(vec![
            AgentTraceEvent::TurnStarted {
                turn: 1,
                response_id: None,
                model: None,
            },
            AgentTraceEvent::ToolCallStarted {
                index: 0,
                id: "call-1".to_string(),
                name: "ask_confirmation".to_string(),
                arguments: serde_json::Value::Null,
            },
            AgentTraceEvent::ToolCallArgsCompleted {
                index: 0,
                id: "call-1".to_string(),
                name: "ask_confirmation".to_string(),
                arguments: serde_json::json!({ "question": "Proceed?" }),
            },
            AgentTraceEvent::ToolCallCompleted {
                index: 0,
                id: "call-1".to_string(),
                name: "ask_confirmation".to_string(),
                result: serde_json::json!({
                    "interaction_id": "ix-1",
                    "status": "interaction_pending"
                }),
                duration_ms: 1,
                success: true,
            },
            interaction_requested_event(),
            AgentTraceEvent::TurnCompleted {
                turn: 1,
                finish_reason: Some("tool_calls".to_string()),
            },
            AgentTraceEvent::Completed {
                text: None,
                usage: None,
            },
        ]);
        let events = AgUiStreamDriver::new(AgUiDriverConfig {
            ctx: test_ctx(),
            cancel_flag: None,
            enrichers: vec![],
            initial_state: serde_json::Value::Null,
            initial_messages: vec![serde_json::json!({
                "id": "user-1",
                "role": "user",
                "content": "Proceed with care"
            })],
        })
        .drive(stream)
        .collect::<Vec<_>>()
        .await;
        let messages = events
            .iter()
            .find_map(|event| match event {
                AgentIoEvent::MessagesSnapshot { messages } => Some(messages),
                _ => None,
            })
            .expect("messages snapshot");

        assert!(
            messages.len() == 3
                && messages[1]["role"] == "assistant"
                && messages[1]["toolCalls"][0]["id"] == "call-1"
                && messages[1]["toolCalls"][0]["function"]["arguments"]["question"]
                    == "Proceed?"
                && messages[2]["role"] == "tool"
                && messages[2]["toolCallId"] == "call-1"
                && messages[2]["content"]["interaction_id"] == "ix-1",
            "interrupt snapshot must preserve the assistant tool call and its pending result: {messages:?}"
        );
    }

    #[tokio::test]
    async fn driver_emits_run_finished_without_interaction() {
        let events = vec![AgentTraceEvent::Completed {
            text: None,
            usage: None,
        }];
        let names = collect_event_names(events).await;
        assert_eq!(names.first().map(String::as_str), Some("RUN_STARTED"));
        assert!(
            names.contains(&"RUN_FINISHED".to_string()),
            "expected RUN_FINISHED, got: {:?}",
            names
        );
        assert!(
            !names.contains(&"run_input_required".to_string()),
            "run_input_required must not appear without interaction, got: {:?}",
            names
        );
    }

    #[tokio::test]
    async fn driver_status_is_input_required_when_interaction_seen() {
        let events = vec![
            interaction_requested_event(),
            AgentTraceEvent::Completed {
                text: None,
                usage: None,
            },
        ];
        let stream = futures::stream::iter(events);
        let driver = AgUiStreamDriver::new(AgUiDriverConfig {
            ctx: test_ctx(),
            cancel_flag: None,
            enrichers: vec![],
            initial_state: serde_json::Value::Null,
            initial_messages: Vec::new(),
        });
        let mut agui_stream = driver.drive(stream);
        while agui_stream.next().await.is_some() {}
        let summary = agui_stream.into_summary();
        assert_eq!(summary.status, RunStatus::InputRequired);
    }
}
