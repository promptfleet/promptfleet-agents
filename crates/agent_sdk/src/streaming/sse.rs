use std::{convert::Infallible, time::Duration};

use a2a_protocol_core::streaming::StreamResponse;
use async_stream::stream;
use axum::response::sse::{Event, KeepAlive, Sse};
use futures::Stream;
use serde_json::to_string;

use crate::agent::trace::AgentTraceEvent;

use super::driver::{AgUiDriverConfig, AgUiStreamDriver, SummaryHandle};
use super::events::AgentIoEvent;

pub fn agent_io_sse_stream(
    events: impl Stream<Item = AgentIoEvent> + Send + 'static,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let stream = stream! {
        use futures_util::StreamExt;
        let mut events = Box::pin(events);
        while let Some(event) = events.next().await {
            let data = to_string(&event).unwrap_or_else(|_| "{}".to_string());
            yield Ok::<Event, Infallible>(Event::default().data(data));
        }
    };

    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text(":keepalive"),
    )
}

pub fn a2a_sse_stream(
    events: impl Stream<Item = StreamResponse> + Send + 'static,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let stream = stream! {
        use futures_util::StreamExt;
        let mut events = Box::pin(events);
        while let Some(event) = events.next().await {
            let data = to_string(&event.to_jsonrpc_data()).unwrap_or_else(|_| "{}".to_string());
            yield Ok::<Event, Infallible>(
                Event::default()
                    .event(event.event_name())
                    .data(data)
            );
        }
    };

    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text(":keepalive"),
    )
}

/// One-liner: trace stream + config → Axum SSE response.
///
/// Combines [`AgUiStreamDriver`] with [`agent_io_sse_stream`] so a standalone
/// native agent can serve AG UI events over SSE without any manual wiring.
pub fn ag_ui_sse_response<S>(
    config: AgUiDriverConfig,
    trace_stream: S,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>>
where
    S: Stream<Item = AgentTraceEvent> + Send + 'static,
{
    agent_io_sse_stream(AgUiStreamDriver::new(config).drive(trace_stream))
}

/// Same as [`ag_ui_sse_response`] but also returns a [`SummaryHandle`] that
/// resolves to the [`super::driver::RunSummary`] after the stream completes.
///
/// Use when you need post-run processing (metrics, persistence, etc.) alongside
/// live SSE streaming.
pub fn ag_ui_sse_response_with_summary<S>(
    config: AgUiDriverConfig,
    trace_stream: S,
) -> (
    Sse<impl Stream<Item = Result<Event, Infallible>>>,
    SummaryHandle,
)
where
    S: Stream<Item = AgentTraceEvent> + Send + 'static,
{
    let (event_stream, handle) = AgUiStreamDriver::new(config)
        .drive(trace_stream)
        .with_summary_handle();
    (agent_io_sse_stream(event_stream), handle)
}
