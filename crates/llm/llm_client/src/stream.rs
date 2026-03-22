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

use crate::model_client::ClientError;
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
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    std::pin::Pin<Box<dyn futures::Stream<Item = Result<StreamEvent, ClientError>> + Send>>;

// ---------------------------------------------------------------------------
// SSE parser
// ---------------------------------------------------------------------------

/// Stateful SSE line parser.
///
/// Feeds raw bytes from the HTTP response body and emits complete
/// `data:` payloads as strings. Handles line buffering across chunk
/// boundaries and ignores non-data SSE fields.
pub struct SseParser {
    /// Accumulated bytes not yet terminated by `\n`
    buffer: String,
    /// Complete `data:` payloads ready for consumption
    events: std::collections::VecDeque<String>,
}

impl SseParser {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
            events: std::collections::VecDeque::new(),
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
        } else if let Some(data) = line.strip_prefix("data:") {
            // Handle `data:` without trailing space (technically valid SSE)
            self.events.push_back(data.to_string());
        }
        // Other SSE fields (event:, id:, retry:) and blank lines are ignored
    }

    /// Get the next complete SSE data payload, if available.
    pub fn next_event(&mut self) -> Option<String> {
        self.events.pop_front()
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
pub fn parse_chat_chunk(data: &str) -> Result<Vec<StreamEvent>, ClientError> {
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
/// The returned stream completes when the upstream sends `data: [DONE]`,
/// the connection closes, or an error occurs.
#[cfg(not(target_arch = "wasm32"))]
pub fn sse_event_stream(response: reqwest::Response) -> LlmEventStream {
    use futures::StreamExt;

    let (tx, rx) = tokio::sync::mpsc::channel::<Result<StreamEvent, ClientError>>(64);

    tokio::spawn(async move {
        let mut parser = SseParser::new();
        let mut byte_stream = response.bytes_stream();

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
                                for event in events {
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
                    let err = ClientError::Transport(
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

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
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

    /// Simulates a full SSE transcript and verifies the sse_event_stream
    /// pipeline produces the correct sequence of StreamEvents.
    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn sse_event_stream_full_transcript() {
        // Build a realistic SSE transcript (3 chunks + [DONE])
        let transcript = [
            "data: {\"id\":\"chatcmpl-test\",\"model\":\"gpt-4\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\"},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"chatcmpl-test\",\"model\":\"gpt-4\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Hello\"},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"chatcmpl-test\",\"model\":\"gpt-4\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\" world\"},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"chatcmpl-test\",\"model\":\"gpt-4\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":2,\"total_tokens\":7}}\n\n",
            "data: [DONE]\n\n",
        ]
        .join("");

        // Feed the transcript through our parser machinery directly
        // (bypassing reqwest, since we test the parsing pipeline)
        let mut parser = SseParser::new();
        parser.feed(transcript.as_bytes());

        let mut all_events = Vec::new();
        while let Some(data) = parser.next_event() {
            if data == "[DONE]" {
                break;
            }
            let events = parse_chat_chunk(&data).unwrap();
            all_events.extend(events);
        }

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

    /// Simulates SSE transcript with tool calls and verifies the correct
    /// sequence of ToolCallStart, ToolCallDelta, and Done events.
    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn sse_event_stream_tool_call_transcript() {
        let transcript = [
            "data: {\"id\":\"chatcmpl-tc\",\"model\":\"gpt-4\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"tool_calls\":[{\"index\":0,\"id\":\"call_123\",\"type\":\"function\",\"function\":{\"name\":\"get_weather\",\"arguments\":\"\"}}]},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"chatcmpl-tc\",\"model\":\"gpt-4\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"city\\\"\"}}]},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"chatcmpl-tc\",\"model\":\"gpt-4\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\":\\\"Paris\\\"}\"}}]},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"chatcmpl-tc\",\"model\":\"gpt-4\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n",
        ]
        .join("");

        let mut parser = SseParser::new();
        parser.feed(transcript.as_bytes());

        let mut all_events = Vec::new();
        while let Some(data) = parser.next_event() {
            if data == "[DONE]" {
                break;
            }
            let events = parse_chat_chunk(&data).unwrap();
            all_events.extend(events);
        }

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

    /// Simulates SSE transcript with reasoning deltas (Qwen3-style).
    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn sse_event_stream_reasoning_transcript() {
        let transcript = [
            "data: {\"id\":\"chatcmpl-r\",\"model\":\"qwen3-235b\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\"},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"chatcmpl-r\",\"model\":\"qwen3-235b\",\"choices\":[{\"index\":0,\"delta\":{\"reasoning_content\":\"Let me analyze this.\"},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"chatcmpl-r\",\"model\":\"qwen3-235b\",\"choices\":[{\"index\":0,\"delta\":{\"reasoning_content\":\" The answer is clear.\"},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"chatcmpl-r\",\"model\":\"qwen3-235b\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"42\"},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"chatcmpl-r\",\"model\":\"qwen3-235b\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            "data: [DONE]\n\n",
        ]
        .join("");

        let mut parser = SseParser::new();
        parser.feed(transcript.as_bytes());

        let mut all_events = Vec::new();
        while let Some(data) = parser.next_event() {
            if data == "[DONE]" {
                break;
            }
            let events = parse_chat_chunk(&data).unwrap();
            all_events.extend(events);
        }

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
