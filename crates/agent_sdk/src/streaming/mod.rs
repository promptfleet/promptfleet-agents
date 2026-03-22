pub mod broadcast;
pub mod driver;
pub mod enrichers;
pub mod events;
pub mod mapper;
pub mod sse;
#[cfg(test)]
mod tests;

pub use a2a_protocol_core::streaming::{
    StreamResponse, TaskArtifactUpdateEvent, TaskStatusUpdateEvent,
};
pub use broadcast::StreamBroadcast;
pub use driver::{
    AgUiDriverConfig, AgUiStream, AgUiStreamDriver, RunStatus, RunSummary, StreamEnricher,
    SummaryHandle,
};
pub use enrichers::CitationEnricher;
pub use events::{AgentIoEvent, IoEventContext};
pub use mapper::{map_trace_to_stream_response, map_trace_to_agent_io, A2aSseContext};
pub use sse::{
    a2a_sse_stream, ag_ui_sse_response, ag_ui_sse_response_with_summary, agent_io_sse_stream,
};
