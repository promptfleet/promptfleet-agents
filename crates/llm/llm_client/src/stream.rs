//! Streaming event types, SSE parser, and event-stream construction.
//!
//! This module defines the [`StreamEvent`] vocabulary emitted during an LLM
//! streaming response and provides the machinery to parse raw SSE byte streams
//! from OpenAI-compatible providers into typed events.
//!
//! # Event sequence
//!
//! **Text response:**
//! `StreamStart -> ContentDelta* -> Done`
//!
//! **Tool calls:**
//! `StreamStart -> ToolCallStart -> ToolCallDelta* -> Done`
//!
//! **Reasoning models (Qwen3, DeepSeek R1):**
//! `StreamStart -> ReasoningDelta* -> ContentDelta* -> Done`

use crate::error::LlmError;
use crate::types::Usage;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// StreamEvent
// ---------------------------------------------------------------------------

/// A single event from an LLM streaming response.
///
/// Events are emitted in real-time as the provider generates tokens.
/// Variants cover content deltas, native reasoning/CoT deltas (never
/// fabricated), incremental tool-call fragments, and lifecycle signals.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamEvent {
    /// Stream started. Emitted once from the first chunk that contains a role.
    StreamStart {
        /// Provider-assigned response ID (e.g. `"chatcmpl-xxx"`)
        id: Option<String>,
        /// Model generating the response
        model: Option<String>,
    },

    /// A text content delta from the assistant.
    ContentDelta {
        /// The text fragment
        delta: String,
    },

    /// A reasoning/thinking delta from reasoning models.
    ///
    /// Only emitted when the upstream provider natively streams reasoning
    /// tokens (e.g. Qwen3 with `enable_thinking=true`, DeepSeek R1 via
    /// `reasoning_content`). **Never fabricated.**
    ReasoningDelta {
        /// The reasoning text fragment
        delta: String,
    },

    /// A new tool call started in the stream.
    ToolCallStart {
        /// Tool call index within this response (for parallel tool calls)
        index: u32,
        /// Unique ID for this tool call (e.g. `"call_xxx"`)
        id: String,
        /// Function name being called
        name: String,
    },

    /// An arguments JSON fragment for an in-progress tool call.
    ToolCallDelta {
        /// Tool call index (matches [`ToolCallStart::index`])
        index: u32,
        /// Fragment of the JSON arguments string
        arguments_delta: String,
    },

    /// The stream completed.
    Done {
        /// Why generation stopped: `"stop"`, `"tool_calls"`, `"length"`, etc.
        finish_reason: Option<String>,
        /// Token usage stats (only present when `stream_options.include_usage`
        /// was set in the request)
        usage: Option<Usage>,
    },

    /// An error occurred during streaming.
    Error {
        /// Human-readable error description
        message: String,
    },
}

/// A boxed, pinned, `Send` stream of [`StreamEvent`] results.
///
/// This is the canonical return type for all streaming LLM methods.
pub type LlmEventStream =
    std::pin::Pin<Box<dyn futures::Stream<Item = Result<StreamEvent, LlmError>> + Send>>;

// ---------------------------------------------------------------------------
// SSE parser
// ---------------------------------------------------------------------------

/// Stateful SSE line parser.
///
/// Feeds raw bytes from the HTTP response body and emits complete
/// `data:` payloads as strings. Handles line buffering across chunk
/// boundaries and supports both data-only and typed event consumption.
///
/// Use [`next_event`] for data-only payloads (OpenAI-compatible) or
/// [`next_typed_event`] for `(event_type, data)` pairs (Anthropic-compatible).
pub struct SseParser {
    /// Accumulated bytes not yet terminated by `\n`
    buffer: String,
    /// Complete `data:` payloads ready for consumption (data-only)
    events: std::collections::VecDeque<String>,
    /// Current `event:` field value, applied to the next `data:` line
    current_event_type: Option<String>,
    /// Complete `(event_type, data)` pairs for providers that use `event:` lines
    typed_events: std::collections::VecDeque<(Option<String>, String)>,
}

impl SseParser {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
            events: std::collections::VecDeque::new(),
            current_event_type: None,
            typed_events: std::collections::VecDeque::new(),
        }
    }

    /// Feed a chunk of bytes from the response body.
    pub fn feed(&mut self, bytes: &[u8]) {
        let text = String::from_utf8_lossy(bytes);
        self.buffer.push_str(&text);
        self.drain_lines();
    }

    /// Drain all complete lines from the internal buffer.
    fn drain_lines(&mut self) {
        while let Some(newline_pos) = self.buffer.find('\n') {
            let line = self.buffer[..newline_pos]
                .trim_end_matches('\r')
                .to_string();
            self.buffer = self.buffer[newline_pos + 1..].to_string();
            self.process_line(&line);
        }
    }

    /// Process a single SSE line.
    fn process_line(&mut self, line: &str) {
        if let Some(data) = line.strip_prefix("data: ") {
            self.events.push_back(data.to_string());
            self.typed_events
                .push_back((self.current_event_type.take(), data.to_string()));
        } else if let Some(data) = line.strip_prefix("data:") {
            self.events.push_back(data.to_string());
            self.typed_events
                .push_back((self.current_event_type.take(), data.to_string()));
        } else if let Some(evt) = line.strip_prefix("event: ") {
            self.current_event_type = Some(evt.to_string());
        } else if let Some(evt) = line.strip_prefix("event:") {
            self.current_event_type = Some(evt.to_string());
        }
    }

    /// Get the next complete SSE data payload, if available.
    ///
    /// Returns data-only strings, ignoring `event:` fields.
    /// Use this for OpenAI-compatible streams.
    pub fn next_event(&mut self) -> Option<String> {
        self.events.pop_front()
    }

    /// Get the next `(event_type, data)` pair, if available.
    ///
    /// The `event_type` is `Some` when the data line was preceded by an
    /// `event:` SSE field, `None` otherwise.
    /// Use this for Anthropic-style streams that require the event type
    /// to dispatch parsing.
    pub fn next_typed_event(&mut self) -> Option<(Option<String>, String)> {
        self.typed_events.pop_front()
    }
}

// ---------------------------------------------------------------------------
// Chunk parser (OpenAI chat completions format)
// ---------------------------------------------------------------------------

/// Parse a single OpenAI chat completion chunk JSON string into
/// [`StreamEvent`]s.
///
/// Most chunks produce exactly one event. The first chunk of a tool call
/// may produce both a [`StreamEvent::ToolCallStart`] and a
/// [`StreamEvent::ToolCallDelta`] if arguments are included in the same
/// chunk. Returns an empty vec for chunks with no actionable delta.
pub(crate) fn parse_chat_chunk(data: &str) -> Result<Vec<StreamEvent>, LlmError> {
    let json: serde_json::Value = serde_json::from_str(data)?;

    let id = json
        .get("id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let model = json
        .get("model")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let mut events = Vec::new();

    if let Some(choices) = json.get("choices").and_then(|v| v.as_array()) {
        for choice in choices {
            let delta = match choice.get("delta") {
                Some(d) => d,
                None => continue,
            };

            let finish_reason = choice
                .get("finish_reason")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            // First chunk often carries `delta.role` — emit StreamStart
            if delta.get("role").is_some() && events.is_empty() {
                events.push(StreamEvent::StreamStart {
                    id: id.clone(),
                    model: model.clone(),
                });
            }

            // Reasoning content (thinking models: Qwen3, DeepSeek R1)
            if let Some(reasoning) = delta.get("reasoning_content").and_then(|v| v.as_str()) {
                if !reasoning.is_empty() {
                    events.push(StreamEvent::ReasoningDelta {
                        delta: reasoning.to_string(),
                    });
                }
            }

            // Text content
            if let Some(content) = delta.get("content").and_then(|v| v.as_str()) {
                if !content.is_empty() {
                    events.push(StreamEvent::ContentDelta {
                        delta: content.to_string(),
                    });
                }
            }

            // Tool calls (streamed incrementally)
            if let Some(tool_calls) = delta.get("tool_calls").and_then(|v| v.as_array()) {
                for tc in tool_calls {
                    let idx = tc.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as u32;

                    // First fragment of a tool call: has `id` + `function.name`
                    let tc_id = tc.get("id").and_then(|v| v.as_str());
                    let tc_name = tc
                        .get("function")
                        .and_then(|f| f.get("name"))
                        .and_then(|v| v.as_str());

                    if let (Some(call_id), Some(name)) = (tc_id, tc_name) {
                        events.push(StreamEvent::ToolCallStart {
                            index: idx,
                            id: call_id.to_string(),
                            name: name.to_string(),
                        });
                    }

                    // Arguments delta
                    if let Some(args) = tc
                        .get("function")
                        .and_then(|f| f.get("arguments"))
                        .and_then(|v| v.as_str())
                    {
                        if !args.is_empty() {
                            events.push(StreamEvent::ToolCallDelta {
                                index: idx,
                                arguments_delta: args.to_string(),
                            });
                        }
                    }
                }
            }

            // finish_reason signals the end of generation
            if let Some(reason) = finish_reason {
                let usage = json
                    .get("usage")
                    .and_then(|u| serde_json::from_value::<Usage>(u.clone()).ok());
                events.push(StreamEvent::Done {
                    finish_reason: Some(reason),
                    usage,
                });
            }
        }
    }

    // Handle usage-only final chunk (when stream_options.include_usage = true,
    // OpenAI may send a final chunk with usage data and empty choices)
    if events.is_empty() {
        if let Some(usage_val) = json.get("usage") {
            if let Ok(usage) = serde_json::from_value::<Usage>(usage_val.clone()) {
                events.push(StreamEvent::Done {
                    finish_reason: None,
                    usage: Some(usage),
                });
            }
        }
    }

    Ok(events)
}

/// Some OpenAI-compatible gateways repeat `delta.role` on every SSE chunk. We emit at most one
/// [`StreamEvent::StreamStart`] per HTTP response so the sequence matches the documented
/// `StreamStart → … → Done` shape.
fn dedupe_stream_starts(
    events: Vec<StreamEvent>,
    stream_start_sent: &mut bool,
) -> Vec<StreamEvent> {
    let mut out = Vec::with_capacity(events.len());
    for event in events {
        if matches!(&event, StreamEvent::StreamStart { .. }) {
            if *stream_start_sent {
                continue;
            }
            *stream_start_sent = true;
        }
        out.push(event);
    }
    out
}

// ---------------------------------------------------------------------------
// Native-only: SSE byte stream → StreamEvent stream
// ---------------------------------------------------------------------------

/// Convert a raw `reqwest::Response` (whose body is an SSE byte stream)
/// into a typed [`LlmEventStream`].
///
/// Spawns a background tokio task that reads chunks, feeds them through
/// [`SseParser`], parses each data line via [`parse_chat_chunk`], and
/// sends the resulting [`StreamEvent`]s through a bounded channel.
///
/// Repeated [`StreamEvent::StreamStart`] (e.g. when every chunk carries `delta.role`) is
/// collapsed to a single start event per response.
///
/// The returned stream completes when the upstream sends `data: [DONE]`,
/// the connection closes, or an error occurs.
#[cfg(not(target_arch = "wasm32"))]
pub fn sse_event_stream(response: reqwest::Response) -> LlmEventStream {
    use futures::StreamExt;

    let (tx, rx) = tokio::sync::mpsc::channel::<Result<StreamEvent, LlmError>>(64);

    tokio::spawn(async move {
        let mut parser = SseParser::new();
        let mut byte_stream = response.bytes_stream();
        let mut stream_start_sent = false;

        while let Some(result) = byte_stream.next().await {
            match result {
                Ok(chunk) => {
                    parser.feed(&chunk);

                    while let Some(data) = parser.next_event() {
                        if data == "[DONE]" {
                            log::debug!("sse_event_stream: received [DONE]");
                            return;
                        }

                        match parse_chat_chunk(&data) {
                            Ok(events) => {
                                for event in dedupe_stream_starts(events, &mut stream_start_sent) {
                                    if tx.send(Ok(event)).await.is_err() {
                                        log::debug!("sse_event_stream: receiver dropped, stopping");
                                        return;
                                    }
                                }
                            }
                            Err(e) => {
                                log::warn!("sse_event_stream: failed to parse chunk: {}", e);
                                let _ = tx.send(Err(e)).await;
                                return;
                            }
                        }
                    }
                }
                Err(e) => {
                    log::warn!("sse_event_stream: byte stream error: {}", e);
                    let err = LlmError::Transport(
                        protocol_transport_core::TransportError::Network(e.to_string()),
                    );
                    let _ = tx.send(Err(err)).await;
                    return;
                }
            }
        }

        log::debug!("sse_event_stream: byte stream ended");
    });

    // Convert the mpsc::Receiver into a Stream
    Box::pin(futures::stream::unfold(rx, |mut rx| async move {
        match rx.recv().await {
            Some(item) => Some((item, rx)),
            None => None,
        }
    }))
}

/// Convert a buffered SSE response body into a typed [`LlmEventStream`].
///
/// This is the WASM-compatible counterpart of [`sse_event_stream`]. Instead
/// of reading chunks incrementally from a network stream, it parses the
/// entire buffered body at once. All events are emitted immediately.
///
/// Limitations: no incremental token delivery — the caller receives all
/// events after the full response completes. True incremental WASM streaming
/// is deferred until WASI 0.3.
///
/// Like [`sse_event_stream`], repeated [`StreamEvent::StreamStart`] is deduped per response.
pub fn sse_event_stream_from_buffer(body: Vec<u8>) -> LlmEventStream {
    let mut parser = SseParser::new();
    parser.feed(&body);

    let mut all_events: Vec<Result<StreamEvent, LlmError>> = Vec::new();
    let mut stream_start_sent = false;
    while let Some(data) = parser.next_event() {
        if data == "[DONE]" {
            break;
        }
        match parse_chat_chunk(&data) {
            Ok(events) => {
                for event in dedupe_stream_starts(events, &mut stream_start_sent) {
                    all_events.push(Ok(event));
                }
            }
            Err(e) => {
                all_events.push(Err(e));
                break;
            }
        }
    }

    Box::pin(futures::stream::iter(all_events))
}

// ---------------------------------------------------------------------------
// Shared SSE transcripts for parser / buffer parity tests
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod test_fixtures {
    use super::{parse_chat_chunk, SseParser, StreamEvent};

    /// OpenAI-style SSE transcript: start + 2 content chunks + usage + `[DONE]`.
    pub fn openai_full_chat() -> &'static str {
        const S: &str = concat!(
            "data: {\"id\":\"chatcmpl-test\",\"model\":\"gpt-4\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\"},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"chatcmpl-test\",\"model\":\"gpt-4\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Hello\"},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"chatcmpl-test\",\"model\":\"gpt-4\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\" world\"},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"chatcmpl-test\",\"model\":\"gpt-4\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":2,\"total_tokens\":7}}\n\n",
            "data: [DONE]\n\n",
        );
        S
    }

    pub fn openai_tool_transcript() -> &'static str {
        const S: &str = concat!(
            "data: {\"id\":\"chatcmpl-tc\",\"model\":\"gpt-4\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"tool_calls\":[{\"index\":0,\"id\":\"call_123\",\"type\":\"function\",\"function\":{\"name\":\"get_weather\",\"arguments\":\"\"}}]},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"chatcmpl-tc\",\"model\":\"gpt-4\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"city\\\"\"}}]},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"chatcmpl-tc\",\"model\":\"gpt-4\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\":\\\"Paris\\\"}\"}}]},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"chatcmpl-tc\",\"model\":\"gpt-4\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n",
        );
        S
    }

    pub fn openai_reasoning_transcript() -> &'static str {
        const S: &str = concat!(
            "data: {\"id\":\"chatcmpl-r\",\"model\":\"qwen3-235b\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\"},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"chatcmpl-r\",\"model\":\"qwen3-235b\",\"choices\":[{\"index\":0,\"delta\":{\"reasoning_content\":\"Let me analyze this.\"},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"chatcmpl-r\",\"model\":\"qwen3-235b\",\"choices\":[{\"index\":0,\"delta\":{\"reasoning_content\":\" The answer is clear.\"},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"chatcmpl-r\",\"model\":\"qwen3-235b\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"42\"},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"chatcmpl-r\",\"model\":\"qwen3-235b\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            "data: [DONE]\n\n",
        );
        S
    }

    /// Two content chunks; each repeats `delta.role` (some OpenAI-compatible proxies do this).
    pub fn openai_repeat_role_each_chunk() -> &'static str {
        const S: &str = concat!(
            "data: {\"id\":\"chatcmpl-dup\",\"model\":\"gpt-4\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"Hi\"},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"chatcmpl-dup\",\"model\":\"gpt-4\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"!\"},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"chatcmpl-dup\",\"model\":\"gpt-4\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            "data: [DONE]\n\n",
        );
        S
    }

    pub fn collect_openai_transcript_events(transcript: &str) -> Vec<StreamEvent> {
        let mut parser = SseParser::new();
        parser.feed(transcript.as_bytes());
        let mut all = Vec::new();
        while let Some(data) = parser.next_event() {
            if data == "[DONE]" {
                break;
            }
            let events = parse_chat_chunk(&data).expect("fixture chunk must parse");
            all.extend(events);
        }
        all
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::test_fixtures;
    use super::*;

    // ── SseParser ──────────────────────────────────────────────────────

    #[test]
    fn parser_single_complete_line() {
        let mut p = SseParser::new();
        p.feed(b"data: {\"hello\":\"world\"}\n\n");
        assert_eq!(p.next_event(), Some("{\"hello\":\"world\"}".into()));
        assert_eq!(p.next_event(), None);
    }

    #[test]
    fn parser_split_across_chunks() {
        let mut p = SseParser::new();
        p.feed(b"data: {\"hel");
        assert_eq!(p.next_event(), None);
        p.feed(b"lo\":\"world\"}\n\n");
        assert_eq!(p.next_event(), Some("{\"hello\":\"world\"}".into()));
    }

    #[test]
    fn parser_multiple_events_in_one_chunk() {
        let mut p = SseParser::new();
        p.feed(b"data: first\ndata: second\n\n");
        assert_eq!(p.next_event(), Some("first".into()));
        assert_eq!(p.next_event(), Some("second".into()));
        assert_eq!(p.next_event(), None);
    }

    #[test]
    fn parser_ignores_non_data_fields() {
        let mut p = SseParser::new();
        p.feed(b"event: message\nid: 42\nretry: 1000\ndata: payload\n\n");
        assert_eq!(p.next_event(), Some("payload".into()));
        assert_eq!(p.next_event(), None);
    }

    #[test]
    fn parser_handles_crlf() {
        let mut p = SseParser::new();
        p.feed(b"data: crlf\r\n\r\n");
        assert_eq!(p.next_event(), Some("crlf".into()));
    }

    #[test]
    fn parser_data_without_space() {
        let mut p = SseParser::new();
        p.feed(b"data:nospace\n\n");
        assert_eq!(p.next_event(), Some("nospace".into()));
    }

    #[test]
    fn parser_done_signal() {
        let mut p = SseParser::new();
        p.feed(b"data: [DONE]\n\n");
        assert_eq!(p.next_event(), Some("[DONE]".into()));
    }

    #[test]
    fn parser_typed_event_basic() {
        let mut p = SseParser::new();
        p.feed(b"event: message_start\ndata: {\"type\":\"message_start\"}\n\n");

        let (evt, data) = p.next_typed_event().unwrap();
        assert_eq!(evt.as_deref(), Some("message_start"));
        assert_eq!(data, "{\"type\":\"message_start\"}");
        assert!(p.next_typed_event().is_none());

        // data-only queue still has the same event (independent)
        assert_eq!(p.next_event(), Some("{\"type\":\"message_start\"}".into()));
    }

    #[test]
    fn parser_typed_event_none_without_event_line() {
        let mut p = SseParser::new();
        p.feed(b"data: plain-payload\n\n");

        let (evt, data) = p.next_typed_event().unwrap();
        assert!(evt.is_none());
        assert_eq!(data, "plain-payload");
    }

    #[test]
    fn parser_typed_events_multiple() {
        let mut p = SseParser::new();
        p.feed(b"event: content_block_delta\ndata: delta1\n\nevent: message_delta\ndata: done1\n\n");

        let (e1, d1) = p.next_typed_event().unwrap();
        assert_eq!(e1.as_deref(), Some("content_block_delta"));
        assert_eq!(d1, "delta1");

        let (e2, d2) = p.next_typed_event().unwrap();
        assert_eq!(e2.as_deref(), Some("message_delta"));
        assert_eq!(d2, "done1");

        assert!(p.next_typed_event().is_none());
    }

    // ── parse_chat_chunk: content ──────────────────────────────────────

    #[test]
    fn parse_content_delta() {
        let chunk = r#"{"id":"chatcmpl-1","object":"chat.completion.chunk","model":"gpt-4","choices":[{"index":0,"delta":{"content":"Hello"},"finish_reason":null}]}"#;
        let events = parse_chat_chunk(chunk).unwrap();
        assert_eq!(events.len(), 1);
        match &events[0] {
            StreamEvent::ContentDelta { delta } => assert_eq!(delta, "Hello"),
            other => panic!("expected ContentDelta, got {:?}", other),
        }
    }

    #[test]
    fn parse_first_chunk_with_role() {
        let chunk = r#"{"id":"chatcmpl-1","object":"chat.completion.chunk","model":"gpt-4","choices":[{"index":0,"delta":{"role":"assistant"},"finish_reason":null}]}"#;
        let events = parse_chat_chunk(chunk).unwrap();
        assert_eq!(events.len(), 1);
        match &events[0] {
            StreamEvent::StreamStart { id, model } => {
                assert_eq!(id.as_deref(), Some("chatcmpl-1"));
                assert_eq!(model.as_deref(), Some("gpt-4"));
            }
            other => panic!("expected StreamStart, got {:?}", other),
        }
    }

    #[test]
    fn parse_first_chunk_role_and_content() {
        let chunk = r#"{"id":"chatcmpl-1","model":"gpt-4","choices":[{"index":0,"delta":{"role":"assistant","content":"Hi"},"finish_reason":null}]}"#;
        let events = parse_chat_chunk(chunk).unwrap();
        assert_eq!(events.len(), 2);
        assert!(matches!(&events[0], StreamEvent::StreamStart { .. }));
        assert!(matches!(&events[1], StreamEvent::ContentDelta { delta } if delta == "Hi"));
    }

    #[test]
    fn parse_done_with_finish_reason() {
        let chunk = r#"{"id":"chatcmpl-1","model":"gpt-4","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}"#;
        let events = parse_chat_chunk(chunk).unwrap();
        assert_eq!(events.len(), 1);
        match &events[0] {
            StreamEvent::Done {
                finish_reason,
                usage,
            } => {
                assert_eq!(finish_reason.as_deref(), Some("stop"));
                assert!(usage.is_none());
            }
            other => panic!("expected Done, got {:?}", other),
        }
    }

    #[test]
    fn parse_usage_only_final_chunk() {
        let chunk = r#"{"id":"chatcmpl-1","model":"gpt-4","choices":[],"usage":{"prompt_tokens":10,"completion_tokens":20,"total_tokens":30}}"#;
        let events = parse_chat_chunk(chunk).unwrap();
        assert_eq!(events.len(), 1);
        match &events[0] {
            StreamEvent::Done { usage, .. } => {
                let u = usage.as_ref().unwrap();
                assert_eq!(u.prompt_tokens, Some(10));
                assert_eq!(u.completion_tokens, Some(20));
                assert_eq!(u.total_tokens, Some(30));
            }
            other => panic!("expected Done with usage, got {:?}", other),
        }
    }

    // ── parse_chat_chunk: tool calls ──────────────────────────────────

    #[test]
    fn parse_tool_call_start() {
        let chunk = r#"{"id":"chatcmpl-1","model":"gpt-4","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_abc","type":"function","function":{"name":"get_weather","arguments":""}}]},"finish_reason":null}]}"#;
        let events = parse_chat_chunk(chunk).unwrap();
        assert_eq!(events.len(), 1);
        match &events[0] {
            StreamEvent::ToolCallStart { index, id, name } => {
                assert_eq!(*index, 0);
                assert_eq!(id, "call_abc");
                assert_eq!(name, "get_weather");
            }
            other => panic!("expected ToolCallStart, got {:?}", other),
        }
    }

    #[test]
    fn parse_tool_call_start_with_args() {
        let chunk = r#"{"id":"chatcmpl-1","model":"gpt-4","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_abc","type":"function","function":{"name":"get_weather","arguments":"{\"city"}}]},"finish_reason":null}]}"#;
        let events = parse_chat_chunk(chunk).unwrap();
        assert_eq!(events.len(), 2);
        assert!(matches!(&events[0], StreamEvent::ToolCallStart { .. }));
        match &events[1] {
            StreamEvent::ToolCallDelta {
                index,
                arguments_delta,
            } => {
                assert_eq!(*index, 0);
                assert_eq!(arguments_delta, "{\"city");
            }
            other => panic!("expected ToolCallDelta, got {:?}", other),
        }
    }

    #[test]
    fn parse_tool_call_args_delta() {
        let chunk = r#"{"id":"chatcmpl-1","model":"gpt-4","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\":\"Paris\"}"}}]},"finish_reason":null}]}"#;
        let events = parse_chat_chunk(chunk).unwrap();
        assert_eq!(events.len(), 1);
        match &events[0] {
            StreamEvent::ToolCallDelta {
                index,
                arguments_delta,
            } => {
                assert_eq!(*index, 0);
                assert_eq!(arguments_delta, "\":\"Paris\"}");
            }
            other => panic!("expected ToolCallDelta, got {:?}", other),
        }
    }

    #[test]
    fn parse_tool_calls_finish() {
        let chunk = r#"{"id":"chatcmpl-1","model":"gpt-4","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}"#;
        let events = parse_chat_chunk(chunk).unwrap();
        assert_eq!(events.len(), 1);
        match &events[0] {
            StreamEvent::Done { finish_reason, .. } => {
                assert_eq!(finish_reason.as_deref(), Some("tool_calls"));
            }
            other => panic!("expected Done, got {:?}", other),
        }
    }

    #[test]
    fn parse_parallel_tool_calls() {
        let chunk = r#"{"id":"chatcmpl-1","model":"gpt-4","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":"fn_a","arguments":""}},{"index":1,"id":"call_2","type":"function","function":{"name":"fn_b","arguments":""}}]},"finish_reason":null}]}"#;
        let events = parse_chat_chunk(chunk).unwrap();
        assert_eq!(events.len(), 2);
        assert!(
            matches!(&events[0], StreamEvent::ToolCallStart { index: 0, name, .. } if name == "fn_a")
        );
        assert!(
            matches!(&events[1], StreamEvent::ToolCallStart { index: 1, name, .. } if name == "fn_b")
        );
    }

    // ── parse_chat_chunk: reasoning ────────────────────────────────────

    #[test]
    fn parse_reasoning_delta() {
        let chunk = r#"{"id":"chatcmpl-1","model":"qwen3-235b","choices":[{"index":0,"delta":{"reasoning_content":"Let me think..."},"finish_reason":null}]}"#;
        let events = parse_chat_chunk(chunk).unwrap();
        assert_eq!(events.len(), 1);
        match &events[0] {
            StreamEvent::ReasoningDelta { delta } => {
                assert_eq!(delta, "Let me think...");
            }
            other => panic!("expected ReasoningDelta, got {:?}", other),
        }
    }

    #[test]
    fn parse_reasoning_then_content() {
        // Some models emit reasoning + content in the same chunk at transition
        let chunk = r#"{"id":"chatcmpl-1","model":"qwen3","choices":[{"index":0,"delta":{"reasoning_content":"done thinking","content":"The answer is 42"},"finish_reason":null}]}"#;
        let events = parse_chat_chunk(chunk).unwrap();
        assert_eq!(events.len(), 2);
        assert!(
            matches!(&events[0], StreamEvent::ReasoningDelta { delta } if delta == "done thinking")
        );
        assert!(
            matches!(&events[1], StreamEvent::ContentDelta { delta } if delta == "The answer is 42")
        );
    }

    // ── parse_chat_chunk: edge cases ───────────────────────────────────

    #[test]
    fn parse_empty_delta() {
        let chunk = r#"{"id":"chatcmpl-1","model":"gpt-4","choices":[{"index":0,"delta":{},"finish_reason":null}]}"#;
        let events = parse_chat_chunk(chunk).unwrap();
        assert!(events.is_empty());
    }

    #[test]
    fn parse_empty_content_ignored() {
        let chunk = r#"{"id":"chatcmpl-1","model":"gpt-4","choices":[{"index":0,"delta":{"content":""},"finish_reason":null}]}"#;
        let events = parse_chat_chunk(chunk).unwrap();
        assert!(events.is_empty());
    }

    #[test]
    fn parse_invalid_json_returns_error() {
        let result = parse_chat_chunk("not json at all");
        assert!(result.is_err());
    }

    // ── End-to-end SSE stream wiring ──────────────────────────────────

    /// Full SSE transcript → same events as reference collector.
    #[test]
    fn sse_event_stream_full_transcript() {
        let transcript = test_fixtures::openai_full_chat();
        let all_events = test_fixtures::collect_openai_transcript_events(transcript);

        // Verify the event sequence
        assert_eq!(
            all_events.len(),
            4,
            "expected 4 events: start + 2 deltas + done"
        );

        assert!(
            matches!(&all_events[0], StreamEvent::StreamStart { id, model }
                if id.as_deref() == Some("chatcmpl-test") && model.as_deref() == Some("gpt-4"))
        );
        assert!(matches!(&all_events[1], StreamEvent::ContentDelta { delta } if delta == "Hello"));
        assert!(matches!(&all_events[2], StreamEvent::ContentDelta { delta } if delta == " world"));
        match &all_events[3] {
            StreamEvent::Done {
                finish_reason,
                usage,
            } => {
                assert_eq!(finish_reason.as_deref(), Some("stop"));
                let u = usage.as_ref().expect("usage should be present");
                assert_eq!(u.prompt_tokens, Some(5));
                assert_eq!(u.completion_tokens, Some(2));
                assert_eq!(u.total_tokens, Some(7));
            }
            other => panic!("expected Done, got {:?}", other),
        }
    }

    #[test]
    fn sse_event_stream_tool_call_transcript() {
        let transcript = test_fixtures::openai_tool_transcript();
        let all_events = test_fixtures::collect_openai_transcript_events(transcript);

        // Expected: StreamStart, ToolCallStart, ToolCallDelta, ToolCallDelta, Done
        assert_eq!(all_events.len(), 5, "events: {:?}", all_events);

        assert!(matches!(&all_events[0], StreamEvent::StreamStart { .. }));

        match &all_events[1] {
            StreamEvent::ToolCallStart { index, id, name } => {
                assert_eq!(*index, 0);
                assert_eq!(id, "call_123");
                assert_eq!(name, "get_weather");
            }
            other => panic!("expected ToolCallStart, got {:?}", other),
        }

        assert!(matches!(
            &all_events[2],
            StreamEvent::ToolCallDelta { index: 0, arguments_delta } if arguments_delta == "{\"city\""
        ));

        assert!(matches!(
            &all_events[3],
            StreamEvent::ToolCallDelta { index: 0, arguments_delta } if arguments_delta == ":\"Paris\"}"
        ));

        match &all_events[4] {
            StreamEvent::Done { finish_reason, .. } => {
                assert_eq!(finish_reason.as_deref(), Some("tool_calls"));
            }
            other => panic!("expected Done, got {:?}", other),
        }
    }

    #[test]
    fn sse_event_stream_reasoning_transcript() {
        let transcript = test_fixtures::openai_reasoning_transcript();
        let all_events = test_fixtures::collect_openai_transcript_events(transcript);

        // Expected: StreamStart, ReasoningDelta x2, ContentDelta, Done
        assert_eq!(all_events.len(), 5, "events: {:?}", all_events);

        assert!(matches!(&all_events[0], StreamEvent::StreamStart { .. }));
        assert!(
            matches!(&all_events[1], StreamEvent::ReasoningDelta { delta } if delta == "Let me analyze this.")
        );
        assert!(
            matches!(&all_events[2], StreamEvent::ReasoningDelta { delta } if delta == " The answer is clear.")
        );
        assert!(matches!(&all_events[3], StreamEvent::ContentDelta { delta } if delta == "42"));
        assert!(
            matches!(&all_events[4], StreamEvent::Done { finish_reason, .. } if finish_reason.as_deref() == Some("stop"))
        );
    }

    /// Buffered stream emits the same `StreamEvent`s as incremental manual parsing.
    #[test]
    fn buffer_vs_manual_parser_parity() {
        let t = test_fixtures::openai_full_chat();
        let expected = test_fixtures::collect_openai_transcript_events(t);
        let stream = sse_event_stream_from_buffer(t.as_bytes().to_vec());
        let got: Vec<_> = futures::executor::block_on_stream(stream)
            .map(|r| r.expect("fixture should not error"))
            .collect();
        assert_eq!(got, expected);
    }

    /// Raw parse emits two `StreamStart`s; SSE drivers collapse to one.
    #[test]
    fn sse_dedupes_stream_start_when_role_repeated_per_chunk() {
        let transcript = test_fixtures::openai_repeat_role_each_chunk();
        let raw = test_fixtures::collect_openai_transcript_events(transcript);
        assert_eq!(
            raw.iter().filter(|e| matches!(e, StreamEvent::StreamStart { .. })).count(),
            2,
            "fixture must repeat role per chunk"
        );

        let stream = sse_event_stream_from_buffer(transcript.as_bytes().to_vec());
        let got: Vec<_> = futures::executor::block_on_stream(stream)
            .map(|r| r.expect("ok"))
            .collect();
        assert_eq!(got.len(), 4, "events: {:?}", got);
        assert!(matches!(&got[0], StreamEvent::StreamStart { .. }));
        assert!(matches!(&got[1], StreamEvent::ContentDelta { delta } if delta == "Hi"));
        assert!(matches!(&got[2], StreamEvent::ContentDelta { delta } if delta == "!"));
        assert!(matches!(&got[3], StreamEvent::Done { .. }));
    }

    #[test]
    fn sse_event_stream_from_buffer_matches_openai_transcript() {
        let transcript = test_fixtures::openai_full_chat();
        let stream = sse_event_stream_from_buffer(transcript.as_bytes().to_vec());
        let collected: Vec<_> = futures::executor::block_on_stream(stream).collect();

        assert_eq!(collected.len(), 4);
        assert!(matches!(
            &collected[0],
            Ok(StreamEvent::StreamStart { .. })
        ));
        assert!(matches!(
            &collected[1],
            Ok(StreamEvent::ContentDelta { delta }) if delta == "Hello"
        ));
        assert!(matches!(
            &collected[2],
            Ok(StreamEvent::ContentDelta { delta }) if delta == " world"
        ));
        match &collected[3] {
            Ok(StreamEvent::Done {
                finish_reason,
                usage,
            }) => {
                assert_eq!(finish_reason.as_deref(), Some("stop"));
                let u = usage.as_ref().expect("usage");
                assert_eq!(u.prompt_tokens, Some(5));
                assert_eq!(u.completion_tokens, Some(2));
                assert_eq!(u.total_tokens, Some(7));
            }
            other => panic!("expected Done, got {:?}", other),
        }
    }

    #[test]
    fn sse_invalid_chunk_mid_stream_returns_transport_error() {
        let transcript = concat!(
            "data: {\"id\":\"x\",\"model\":\"gpt-4\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hi\"},\"finish_reason\":null}]}\n\n",
            "data: {{{not-json\n\n",
        );
        let stream = sse_event_stream_from_buffer(transcript.as_bytes().to_vec());
        let collected: Vec<_> = futures::executor::block_on_stream(stream).collect();
        assert_eq!(collected.len(), 2);
        assert!(matches!(&collected[0], Ok(StreamEvent::ContentDelta { delta }) if delta == "hi"));
        assert!(collected[1].is_err());
    }

    /// Tests that SSE bytes split at arbitrary boundaries still parse correctly.
    #[test]
    fn parser_byte_level_splitting() {
        let full = b"data: {\"a\":1}\ndata: {\"b\":2}\ndata: [DONE]\n";

        // Feed byte by byte to stress test the line buffer
        let mut parser = SseParser::new();
        for &byte in full.iter() {
            parser.feed(&[byte]);
        }

        assert_eq!(parser.next_event(), Some("{\"a\":1}".into()));
        assert_eq!(parser.next_event(), Some("{\"b\":2}".into()));
        assert_eq!(parser.next_event(), Some("[DONE]".into()));
        assert_eq!(parser.next_event(), None);
    }
}

/// [`sse_event_stream`] (incremental native) vs [`sse_event_stream_from_buffer`] on the same bytes.
#[cfg(all(test, not(target_arch = "wasm32")))]
mod native_sse_parity_tests {
    use super::sse_event_stream;
    use super::sse_event_stream_from_buffer;
    use super::test_fixtures;
    use super::{LlmError, StreamEvent};
    use bytes::Bytes;
    use futures::StreamExt;
    use std::convert::Infallible;

    fn openai_sse_response_chunked(body: &[u8], chunk: usize) -> reqwest::Response {
        let chunks: Vec<Bytes> = body
            .chunks(chunk.max(1))
            .map(Bytes::copy_from_slice)
            .collect();
        let st = futures::stream::iter(chunks.into_iter().map(Ok::<_, Infallible>));
        let wrapped = reqwest::Body::wrap_stream(st);
        let http = http::Response::builder()
            .status(200)
            .body(wrapped)
            .expect("fixture response");
        reqwest::Response::from(http)
    }

    #[tokio::test]
    async fn buffer_vs_native_sse_event_parity() {
        let transcript = test_fixtures::openai_full_chat();
        let expected = test_fixtures::collect_openai_transcript_events(transcript);

        let buffer_out: Vec<Result<StreamEvent, LlmError>> =
            sse_event_stream_from_buffer(transcript.as_bytes().to_vec())
                .collect()
                .await;
        let buffer_events: Vec<StreamEvent> = buffer_out.into_iter().map(|r| r.unwrap()).collect();
        assert_eq!(buffer_events, expected);

        for chunk_size in [1_usize, 7, 64, transcript.len()] {
            let resp = openai_sse_response_chunked(transcript.as_bytes(), chunk_size);
            let native_out: Vec<Result<StreamEvent, LlmError>> =
                sse_event_stream(resp).collect().await;
            let native_events: Vec<StreamEvent> =
                native_out.into_iter().map(|r| r.unwrap()).collect();
            assert_eq!(
                native_events, expected,
                "parity failed for chunk_size={chunk_size}"
            );
        }
    }

    #[tokio::test]
    async fn buffer_vs_native_sse_parity_repeat_role_chunks() {
        let transcript = test_fixtures::openai_repeat_role_each_chunk();
        let buffer_events: Vec<StreamEvent> = sse_event_stream_from_buffer(
            transcript.as_bytes().to_vec(),
        )
        .map(|r| r.unwrap())
        .collect()
        .await;

        for chunk_size in [1_usize, 5, 99, transcript.len()] {
            let resp = openai_sse_response_chunked(transcript.as_bytes(), chunk_size);
            let native_events: Vec<StreamEvent> =
                sse_event_stream(resp).map(|r| r.unwrap()).collect().await;
            assert_eq!(
                native_events, buffer_events,
                "parity failed for chunk_size={chunk_size}"
            );
        }
    }
}
