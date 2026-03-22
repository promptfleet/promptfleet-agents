use std::collections::VecDeque;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};

use futures::Stream;
use tokio::sync::oneshot;

use crate::agent::trace::AgentTraceEvent;

use super::{map_trace_to_agent_io, AgentIoEvent, IoEventContext};

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
            finished: false,
            cancellation_emitted: false,
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
    finished: bool,
    cancellation_emitted: bool,
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
        self.pending.push_back(AgentIoEvent::Custom {
            name: "run_cancelled".to_string(),
            value: serde_json::json!({
                "run_id": self.ctx.run_id,
                "thread_id": self.ctx.thread_id,
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

    fn enqueue_events(&mut self, event: AgentTraceEvent) {
        self.update_summary(&event);

        // When the run completes but an interaction was already requested, replace
        // run_finished with run_input_required so the frontend knows to show the
        // interaction UI rather than treating the run as done.
        let mapped = if matches!(event, AgentTraceEvent::Completed { .. })
            && self.summary.status == RunStatus::InputRequired
        {
            vec![AgentIoEvent::Custom {
                name: "run_input_required".to_string(),
                value: serde_json::json!({
                    "runId": self.ctx.run_id,
                    "threadId": self.ctx.thread_id,
                }),
            }]
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
            Some("run_started"),
            "first event must be run_started, got: {:?}",
            names
        );
    }

    #[tokio::test]
    async fn driver_emits_run_input_required_when_interaction_seen() {
        let events = vec![
            interaction_requested_event(),
            AgentTraceEvent::Completed {
                text: None,
                usage: None,
            },
        ];
        let names = collect_event_names(events).await;
        assert_eq!(names.first().map(String::as_str), Some("run_started"));
        assert!(
            names.contains(&"interaction_requested".to_string()),
            "expected interaction_requested, got: {:?}",
            names
        );
        assert!(
            names.contains(&"run_input_required".to_string()),
            "expected run_input_required, got: {:?}",
            names
        );
        assert!(
            !names.contains(&"run_finished".to_string()),
            "run_finished must not appear when interaction pending, got: {:?}",
            names
        );
    }

    #[tokio::test]
    async fn driver_emits_run_finished_without_interaction() {
        let events = vec![AgentTraceEvent::Completed {
            text: None,
            usage: None,
        }];
        let names = collect_event_names(events).await;
        assert_eq!(names.first().map(String::as_str), Some("run_started"));
        assert!(
            names.contains(&"run_finished".to_string()),
            "expected run_finished, got: {:?}",
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
        });
        let mut agui_stream = driver.drive(stream);
        while agui_stream.next().await.is_some() {}
        let summary = agui_stream.into_summary();
        assert_eq!(summary.status, RunStatus::InputRequired);
    }
}
