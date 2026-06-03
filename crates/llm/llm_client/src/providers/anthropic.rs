use crate::{
    auth::AuthProvider,
    error::LlmError,
    model_client::{ClientCapabilities, HttpModelClient},
    provider::LlmProvider,
    types::{
        ChatContentPart, ChatMessage, LlmChoice, LlmRequest, LlmResponse, ToolCall,
        ToolCallRequest, Usage,
    },
};
use protocol_transport_core::StreamingPolicy;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone)]
pub(crate) struct AnthropicClient {
    inner: HttpModelClient,
}

impl AnthropicClient {
    pub(crate) fn new(
        base_url: String,
        default_headers: HashMap<String, String>,
        streaming: Option<StreamingPolicy>,
        auth: Arc<dyn AuthProvider>,
    ) -> Self {
        log::debug!("AnthropicClient::new");
        Self {
            inner: HttpModelClient::new(base_url, default_headers, streaming, auth),
        }
    }
}

// ---------------------------------------------------------------------------
// Payload & response mapping
// ---------------------------------------------------------------------------

impl AnthropicClient {
    fn content_blocks(msg: &ChatMessage) -> Option<serde_json::Value> {
        if let Some(parts) = &msg.content_parts {
            let blocks = parts
                .iter()
                .filter(|part| !part.is_empty_text())
                .map(|part| match part {
                    ChatContentPart::Text { text } => serde_json::json!({
                        "type": "text",
                        "text": text,
                    }),
                    ChatContentPart::ImageUrl { url, .. } => serde_json::json!({
                        "type": "image",
                        "source": {
                            "type": "url",
                            "url": url,
                        },
                    }),
                    ChatContentPart::ImageBase64 {
                        media_type, data, ..
                    } => serde_json::json!({
                        "type": "image",
                        "source": {
                            "type": "base64",
                            "media_type": media_type,
                            "data": data,
                        },
                    }),
                })
                .collect::<Vec<_>>();
            return Some(serde_json::Value::Array(blocks));
        }
        msg.content
            .as_ref()
            .map(|content| serde_json::Value::String(content.clone()))
    }

    /// Convert an [`LlmRequest`] into the Anthropic Messages API wire format.
    ///
    /// Key differences from OpenAI:
    /// - System messages extracted to a top-level `system` parameter
    /// - `max_tokens` is required (defaults to 4096)
    /// - Tool schemas use `input_schema` instead of `parameters`
    /// - Tool results mapped from `role: "tool"` to Anthropic `tool_result` blocks
    /// - Assistant tool calls mapped to `tool_use` content blocks
    pub fn to_messages_payload(req: &LlmRequest) -> serde_json::Value {
        let mut obj = serde_json::Map::new();
        obj.insert(
            "model".to_string(),
            serde_json::Value::String(req.model.clone()),
        );

        // Extract system messages to top-level param
        let mut system_parts: Vec<String> = Vec::new();
        let mut non_system: Vec<&ChatMessage> = Vec::new();
        for msg in &req.messages {
            if msg.role == "system" {
                if let Some(content) = &msg.content {
                    system_parts.push(content.clone());
                }
            } else {
                non_system.push(msg);
            }
        }
        if !system_parts.is_empty() {
            obj.insert(
                "system".to_string(),
                serde_json::Value::String(system_parts.join("\n")),
            );
        }

        // Map messages, batching consecutive tool-result messages into a single
        // "user" message (Anthropic requires alternating user/assistant turns).
        let mut mapped: Vec<serde_json::Value> = Vec::new();
        let mut pending_tool_results: Vec<serde_json::Value> = Vec::new();

        for msg in &non_system {
            if msg.role == "tool" {
                let mut block = serde_json::Map::new();
                block.insert(
                    "type".to_string(),
                    serde_json::Value::String("tool_result".to_string()),
                );
                if let Some(id) = &msg.tool_call_id {
                    block.insert(
                        "tool_use_id".to_string(),
                        serde_json::Value::String(id.clone()),
                    );
                }
                if let Some(content) = &msg.content {
                    block.insert(
                        "content".to_string(),
                        serde_json::Value::String(content.clone()),
                    );
                }
                pending_tool_results.push(serde_json::Value::Object(block));
                continue;
            }

            // Flush pending tool results before any non-tool message
            if !pending_tool_results.is_empty() {
                mapped.push(serde_json::json!({
                    "role": "user",
                    "content": serde_json::Value::Array(pending_tool_results.drain(..).collect())
                }));
            }

            if msg.role == "assistant" && msg.tool_calls.is_some() {
                let mut content_blocks: Vec<serde_json::Value> = Vec::new();

                if let Some(text) = &msg.content {
                    if !text.is_empty() {
                        content_blocks.push(serde_json::json!({
                            "type": "text",
                            "text": text
                        }));
                    }
                }

                if let Some(tool_calls) = &msg.tool_calls {
                    for tc in tool_calls {
                        content_blocks.push(serde_json::json!({
                            "type": "tool_use",
                            "id": tc.id,
                            "name": tc.name,
                            "input": tc.arguments
                        }));
                    }
                }

                mapped.push(serde_json::json!({
                    "role": "assistant",
                    "content": content_blocks
                }));
            } else {
                let mut msg_obj = serde_json::Map::new();
                msg_obj.insert(
                    "role".to_string(),
                    serde_json::Value::String(msg.role.clone()),
                );
                if let Some(content) = Self::content_blocks(msg) {
                    msg_obj.insert("content".to_string(), content);
                }
                mapped.push(serde_json::Value::Object(msg_obj));
            }
        }

        // Flush trailing tool results
        if !pending_tool_results.is_empty() {
            mapped.push(serde_json::json!({
                "role": "user",
                "content": serde_json::Value::Array(pending_tool_results)
            }));
        }

        obj.insert("messages".to_string(), serde_json::Value::Array(mapped));

        // max_tokens is required for Anthropic — default to 4096
        let max_tokens = req.max_tokens.unwrap_or(4096);
        obj.insert(
            "max_tokens".to_string(),
            serde_json::Value::from(max_tokens),
        );

        if let Some(temp) = req.temperature {
            obj.insert("temperature".to_string(), serde_json::Value::from(temp));
        }

        // Tools: use input_schema instead of parameters
        if let Some(tools) = &req.tools {
            let mapped_tools: Vec<serde_json::Value> = tools
                .iter()
                .map(|t| {
                    let mut tool_obj = serde_json::Map::new();
                    tool_obj.insert(
                        "name".to_string(),
                        serde_json::Value::String(t.name.clone()),
                    );
                    if let Some(desc) = &t.description {
                        tool_obj.insert(
                            "description".to_string(),
                            serde_json::Value::String(desc.clone()),
                        );
                    }
                    tool_obj.insert("input_schema".to_string(), t.parameters.clone());
                    serde_json::Value::Object(tool_obj)
                })
                .collect();
            obj.insert("tools".to_string(), serde_json::Value::Array(mapped_tools));
        }

        if let Some(choice) = &req.tool_choice {
            obj.insert("tool_choice".to_string(), choice.to_anthropic_value());
        }

        if let Some(ext) = &req.extensions {
            for (k, v) in ext.iter() {
                obj.insert(k.clone(), v.clone());
            }
        }

        let payload = serde_json::Value::Object(obj);
        log::debug!(
            "AnthropicClient::to_messages_payload keys={}",
            payload.as_object().map(|o| o.len()).unwrap_or(0)
        );
        payload
    }

    /// Normalize an Anthropic Messages API response into [`LlmResponse`].
    ///
    /// Extracts text from `content[].type == "text"` blocks, tool calls from
    /// `content[].type == "tool_use"` blocks, and maps `stop_reason` and
    /// `usage` fields to the provider-neutral representation.
    pub fn normalize_messages_json(raw: serde_json::Value) -> Result<LlmResponse, LlmError> {
        let id = raw
            .get("id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let model = raw
            .get("model")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let content_blocks = raw.get("content").and_then(|v| v.as_array());

        let mut text_parts: Vec<String> = Vec::new();
        let mut tool_call_requests: Vec<ToolCallRequest> = Vec::new();
        let mut tool_calls: Vec<ToolCall> = Vec::new();

        if let Some(blocks) = content_blocks {
            for block in blocks {
                let block_type = block.get("type").and_then(|v| v.as_str()).unwrap_or("");
                match block_type {
                    "text" => {
                        if let Some(text) = block.get("text").and_then(|v| v.as_str()) {
                            text_parts.push(text.to_string());
                        }
                    }
                    "tool_use" => {
                        let tc_id = block
                            .get("id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let tc_name = block
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let tc_input = block.get("input").cloned().unwrap_or(serde_json::json!({}));

                        if !tc_name.is_empty() && !tc_id.is_empty() {
                            tool_call_requests.push(ToolCallRequest {
                                id: tc_id.clone(),
                                name: tc_name.clone(),
                                arguments: tc_input.clone(),
                            });
                            tool_calls.push(ToolCall {
                                id: Some(tc_id),
                                call_id: None,
                                name: tc_name,
                                arguments: tc_input,
                            });
                        }
                    }
                    _ => {}
                }
            }
        }

        let content = if text_parts.is_empty() {
            None
        } else {
            Some(text_parts.join(""))
        };

        let finish_reason = raw
            .get("stop_reason")
            .and_then(|v| v.as_str())
            .map(map_stop_reason);

        let usage = raw.get("usage").map(|u| {
            let input = u
                .get("input_tokens")
                .and_then(|v| v.as_u64())
                .map(|v| v as u32);
            let output = u
                .get("output_tokens")
                .and_then(|v| v.as_u64())
                .map(|v| v as u32);
            let total = match (input, output) {
                (Some(i), Some(o)) => Some(i + o),
                _ => None,
            };
            Usage {
                prompt_tokens: input,
                completion_tokens: output,
                total_tokens: total,
            }
        });

        log::debug!(
            "AnthropicClient::normalize_messages_json content_len={} tool_calls={}",
            content.as_ref().map(|s| s.len()).unwrap_or(0),
            tool_calls.len()
        );

        let choice = LlmChoice {
            index: 0,
            message: ChatMessage {
                role: "assistant".to_string(),
                content,
                tool_calls: if tool_call_requests.is_empty() {
                    None
                } else {
                    Some(tool_call_requests)
                },
                ..Default::default()
            },
            finish_reason,
        };

        Ok(LlmResponse {
            id,
            created: None,
            model,
            choices: vec![choice],
            usage,
            tool_calls: if tool_calls.is_empty() {
                None
            } else {
                Some(tool_calls)
            },
        })
    }

    pub async fn llm(&self, req: LlmRequest) -> Result<LlmResponse, LlmError> {
        let payload = Self::to_messages_payload(&req);
        log::info!("AnthropicClient::llm endpoint=/v1/messages");
        let raw = self.inner.post_json("/v1/messages", payload).await?;
        Self::normalize_messages_json(raw)
    }
}

// ---------------------------------------------------------------------------
// Native-only: streaming LLM request
// ---------------------------------------------------------------------------

#[cfg(not(target_arch = "wasm32"))]
impl AnthropicClient {
    /// Send a streaming request to the Anthropic Messages API.
    ///
    /// Injects `"stream": true` and opens an SSE connection. Anthropic uses
    /// typed SSE events (`event:` lines) instead of OpenAI's `data: [DONE]`
    /// termination.
    pub async fn llm_stream(
        &self,
        req: LlmRequest,
    ) -> Result<crate::stream::LlmEventStream, LlmError> {
        let mut payload = Self::to_messages_payload(&req);
        if let Some(obj) = payload.as_object_mut() {
            obj.insert("stream".to_string(), serde_json::Value::Bool(true));
        }

        log::info!("AnthropicClient::llm_stream endpoint=/v1/messages");
        let response = self.inner.post_sse("/v1/messages", payload).await?;
        Ok(sse_event_stream_anthropic(response))
    }
}

#[cfg(target_arch = "wasm32")]
impl AnthropicClient {
    pub async fn llm_stream(
        &self,
        req: LlmRequest,
    ) -> Result<crate::stream::LlmEventStream, LlmError> {
        let mut payload = Self::to_messages_payload(&req);
        if let Some(obj) = payload.as_object_mut() {
            obj.insert("stream".to_string(), serde_json::Value::Bool(true));
        }

        log::info!("AnthropicClient::llm_stream (wasm) endpoint=/v1/messages");

        let body = self
            .inner
            .post_sse_buffered("/v1/messages", payload)
            .await?;
        Ok(Self::stream_from_buffer(body))
    }

    fn stream_from_buffer(body: Vec<u8>) -> crate::stream::LlmEventStream {
        let mut parser = crate::stream::SseParser::new();
        parser.feed(&body);

        let mut all_events: Vec<Result<crate::stream::StreamEvent, LlmError>> = Vec::new();
        while let Some((event_type, data)) = parser.next_typed_event() {
            let ev_type = event_type.as_deref().unwrap_or("");
            if ev_type == "message_stop" {
                break;
            }
            if ev_type == "ping" {
                continue;
            }
            match parse_anthropic_chunk(ev_type, &data) {
                Ok(events) => {
                    for event in events {
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
}

// ---------------------------------------------------------------------------
// Stop reason mapping
// ---------------------------------------------------------------------------

fn map_stop_reason(reason: &str) -> String {
    match reason {
        "end_turn" => "stop".to_string(),
        "tool_use" => "tool_calls".to_string(),
        "max_tokens" => "length".to_string(),
        other => other.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Anthropic SSE chunk parser
// ---------------------------------------------------------------------------

/// Parse a single Anthropic SSE event into [`StreamEvent`]s.
///
/// Anthropic SSE uses typed events (`event:` line + `data:` line):
/// - `message_start` → [`StreamEvent::StreamStart`]
/// - `content_block_start` with `tool_use` → [`StreamEvent::ToolCallStart`]
/// - `content_block_delta` with `text_delta` → [`StreamEvent::ContentDelta`]
/// - `content_block_delta` with `input_json_delta` → [`StreamEvent::ToolCallDelta`]
/// - `message_delta` → [`StreamEvent::Done`]
/// - Other events (`content_block_start` text, `content_block_stop`,
///   `message_stop`, `ping`) are ignored.
pub(crate) fn parse_anthropic_chunk(
    event_type: &str,
    data: &str,
) -> Result<Vec<crate::stream::StreamEvent>, LlmError> {
    use crate::stream::StreamEvent;

    let json: serde_json::Value = serde_json::from_str(data)?;
    let mut events = Vec::new();

    match event_type {
        "message_start" => {
            let msg = json.get("message");
            let id = msg
                .and_then(|m| m.get("id"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let model = msg
                .and_then(|m| m.get("model"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            events.push(StreamEvent::StreamStart { id, model });
        }
        "content_block_start" => {
            let block = json.get("content_block");
            let index = json.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            let block_type = block.and_then(|b| b.get("type")).and_then(|v| v.as_str());

            if block_type == Some("tool_use") {
                let id = block
                    .and_then(|b| b.get("id"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let name = block
                    .and_then(|b| b.get("name"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                events.push(StreamEvent::ToolCallStart { index, id, name });
            }
            // text block starts are ignored — we wait for content_block_delta
        }
        "content_block_delta" => {
            let index = json.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            let delta = json.get("delta");
            let delta_type = delta.and_then(|d| d.get("type")).and_then(|v| v.as_str());

            match delta_type {
                Some("text_delta") => {
                    if let Some(text) = delta.and_then(|d| d.get("text")).and_then(|v| v.as_str()) {
                        if !text.is_empty() {
                            events.push(StreamEvent::ContentDelta {
                                delta: text.to_string(),
                            });
                        }
                    }
                }
                Some("input_json_delta") => {
                    if let Some(partial) = delta
                        .and_then(|d| d.get("partial_json"))
                        .and_then(|v| v.as_str())
                    {
                        if !partial.is_empty() {
                            events.push(StreamEvent::ToolCallDelta {
                                index,
                                arguments_delta: partial.to_string(),
                            });
                        }
                    }
                }
                _ => {}
            }
        }
        "message_delta" => {
            let stop_reason = json
                .get("delta")
                .and_then(|d| d.get("stop_reason"))
                .and_then(|v| v.as_str())
                .map(map_stop_reason);

            let usage = json.get("usage").map(|u| {
                let output = u
                    .get("output_tokens")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32);
                Usage {
                    prompt_tokens: None,
                    completion_tokens: output,
                    total_tokens: None,
                }
            });

            events.push(StreamEvent::Done {
                finish_reason: stop_reason,
                usage,
            });
        }
        // content_block_stop, message_stop, ping — no-op
        _ => {}
    }

    Ok(events)
}

// ---------------------------------------------------------------------------
// Native-only: Anthropic SSE byte stream → StreamEvent stream
// ---------------------------------------------------------------------------

/// Convert a raw `reqwest::Response` (Anthropic SSE) into a typed
/// [`LlmEventStream`].
///
/// Unlike [`crate::stream::sse_event_stream`] (OpenAI), this uses
/// [`SseParser::next_typed_event`] to capture `event:` lines and
/// dispatches via [`parse_anthropic_chunk`].
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn sse_event_stream_anthropic(
    response: reqwest::Response,
) -> crate::stream::LlmEventStream {
    use crate::stream::SseParser;
    use futures::StreamExt;

    let (tx, rx) = tokio::sync::mpsc::channel::<Result<crate::stream::StreamEvent, LlmError>>(64);

    tokio::spawn(async move {
        let mut parser = SseParser::new();
        let mut byte_stream = response.bytes_stream();

        while let Some(result) = byte_stream.next().await {
            match result {
                Ok(chunk) => {
                    parser.feed(&chunk);

                    while let Some((event_type, data)) = parser.next_typed_event() {
                        let evt = event_type.as_deref().unwrap_or("");

                        if evt == "message_stop" {
                            log::debug!("sse_event_stream_anthropic: message_stop");
                            return;
                        }
                        if evt == "ping" {
                            continue;
                        }

                        match parse_anthropic_chunk(evt, &data) {
                            Ok(events) => {
                                for event in events {
                                    if tx.send(Ok(event)).await.is_err() {
                                        log::debug!("sse_event_stream_anthropic: receiver dropped");
                                        return;
                                    }
                                }
                            }
                            Err(e) => {
                                log::warn!("sse_event_stream_anthropic: parse error: {}", e);
                                let _ = tx.send(Err(e)).await;
                                return;
                            }
                        }
                    }
                }
                Err(e) => {
                    log::warn!("sse_event_stream_anthropic: byte stream error: {}", e);
                    let err = LlmError::Transport(
                        protocol_transport_core::TransportError::Network(e.to_string()),
                    );
                    let _ = tx.send(Err(err)).await;
                    return;
                }
            }
        }

        log::debug!("sse_event_stream_anthropic: byte stream ended");
    });

    Box::pin(futures::stream::unfold(rx, |mut rx| async move {
        match rx.recv().await {
            Some(item) => Some((item, rx)),
            None => None,
        }
    }))
}

impl LlmProvider for AnthropicClient {
    fn capabilities(&self) -> ClientCapabilities {
        self.inner.capabilities()
    }

    fn chat<'a>(&'a self, req: LlmRequest) -> crate::provider::ChatFuture<'a> {
        let this = self.clone();
        Box::pin(async move { this.llm(req).await })
    }

    fn chat_stream<'a>(&'a self, req: LlmRequest) -> crate::provider::ChatStreamFuture<'a> {
        let this = self.clone();
        Box::pin(async move { this.llm_stream(req).await })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stream::{SseParser, StreamEvent};
    use crate::types::{ChatContentPart, ChatMessage, LlmRequest, ToolCallRequest, ToolSchema};

    // ── to_messages_payload ──────────────────────────────────────────────

    #[test]
    fn test_to_messages_payload_system_extraction() {
        let req = LlmRequest {
            model: "claude-3-5-sonnet-20241022".to_string(),
            messages: vec![
                ChatMessage {
                    role: "system".to_string(),
                    content: Some("Be concise.".to_string()),
                    ..Default::default()
                },
                ChatMessage {
                    role: "user".to_string(),
                    content: Some("Hello".to_string()),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let payload = AnthropicClient::to_messages_payload(&req);

        assert_eq!(payload["system"], "Be concise.");
        let messages = payload["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(messages[0]["content"], "Hello");
    }

    #[test]
    fn test_to_messages_payload_maps_multimodal_content_parts() {
        let req = LlmRequest {
            model: "claude-sonnet-4-20250514".to_string(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content_parts: Some(vec![
                    ChatContentPart::text("what is visible?"),
                    ChatContentPart::image_base64("image/png", "abc123", None),
                ]),
                ..Default::default()
            }],
            ..Default::default()
        };
        let payload = AnthropicClient::to_messages_payload(&req);
        let content = payload["messages"][0]["content"].as_array().unwrap();

        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[1]["type"], "image");
        assert_eq!(content[1]["source"]["type"], "base64");
        assert_eq!(content[1]["source"]["media_type"], "image/png");
        assert_eq!(content[1]["source"]["data"], "abc123");
    }

    #[test]
    fn test_to_messages_payload_tool_schema() {
        let req = LlmRequest {
            model: "claude-3-5-sonnet-20241022".to_string(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: Some("weather?".to_string()),
                ..Default::default()
            }],
            tools: Some(vec![ToolSchema {
                name: "get_weather".to_string(),
                description: Some("Get weather for a city".to_string()),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "city": {"type": "string"}
                    },
                    "required": ["city"]
                }),
                strict: None,
            }]),
            ..Default::default()
        };
        let payload = AnthropicClient::to_messages_payload(&req);

        let tools = payload["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "get_weather");
        assert_eq!(tools[0]["description"], "Get weather for a city");
        assert_eq!(
            tools[0]["input_schema"]["properties"]["city"]["type"],
            "string"
        );
        // Must NOT have "parameters" key
        assert!(tools[0].get("parameters").is_none());
    }

    #[test]
    fn test_to_messages_payload_max_tokens_default() {
        let req = LlmRequest {
            model: "claude-3-haiku".to_string(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: Some("hi".to_string()),
                ..Default::default()
            }],
            max_tokens: None,
            ..Default::default()
        };
        let payload = AnthropicClient::to_messages_payload(&req);
        assert_eq!(payload["max_tokens"], 4096);
    }

    #[test]
    fn test_to_messages_payload_max_tokens_explicit() {
        let req = LlmRequest {
            model: "claude-3-haiku".to_string(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: Some("hi".to_string()),
                ..Default::default()
            }],
            max_tokens: Some(1024),
            ..Default::default()
        };
        let payload = AnthropicClient::to_messages_payload(&req);
        assert_eq!(payload["max_tokens"], 1024);
    }

    #[test]
    fn test_to_messages_payload_tool_result_mapping() {
        let req = LlmRequest {
            model: "claude-3-5-sonnet-20241022".to_string(),
            messages: vec![
                ChatMessage {
                    role: "user".to_string(),
                    content: Some("weather?".to_string()),
                    ..Default::default()
                },
                ChatMessage {
                    role: "assistant".to_string(),
                    content: None,
                    tool_calls: Some(vec![ToolCallRequest {
                        id: "toolu_1".to_string(),
                        name: "get_weather".to_string(),
                        arguments: serde_json::json!({"city": "Paris"}),
                    }]),
                    ..Default::default()
                },
                ChatMessage {
                    role: "tool".to_string(),
                    content: Some("Sunny, 22°C".to_string()),
                    tool_call_id: Some("toolu_1".to_string()),
                    ..Default::default()
                },
                ChatMessage {
                    role: "tool".to_string(),
                    content: Some("Light wind".to_string()),
                    tool_call_id: Some("toolu_2".to_string()),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let payload = AnthropicClient::to_messages_payload(&req);
        let messages = payload["messages"].as_array().unwrap();

        // msg 0: user
        assert_eq!(messages[0]["role"], "user");

        // msg 1: assistant with tool_use content blocks
        assert_eq!(messages[1]["role"], "assistant");
        let asst_content = messages[1]["content"].as_array().unwrap();
        assert_eq!(asst_content[0]["type"], "tool_use");
        assert_eq!(asst_content[0]["id"], "toolu_1");
        assert_eq!(asst_content[0]["name"], "get_weather");

        // msg 2: both tool results batched into one user message
        assert_eq!(messages[2]["role"], "user");
        let tool_content = messages[2]["content"].as_array().unwrap();
        assert_eq!(tool_content.len(), 2);
        assert_eq!(tool_content[0]["type"], "tool_result");
        assert_eq!(tool_content[0]["tool_use_id"], "toolu_1");
        assert_eq!(tool_content[0]["content"], "Sunny, 22°C");
        assert_eq!(tool_content[1]["type"], "tool_result");
        assert_eq!(tool_content[1]["tool_use_id"], "toolu_2");
        assert_eq!(tool_content[1]["content"], "Light wind");
    }

    #[test]
    fn test_to_messages_payload_assistant_tool_calls() {
        let req = LlmRequest {
            model: "claude-3-5-sonnet-20241022".to_string(),
            messages: vec![
                ChatMessage {
                    role: "user".to_string(),
                    content: Some("check weather and time".to_string()),
                    ..Default::default()
                },
                ChatMessage {
                    role: "assistant".to_string(),
                    content: Some("Let me check.".to_string()),
                    tool_calls: Some(vec![
                        ToolCallRequest {
                            id: "tc_1".to_string(),
                            name: "get_weather".to_string(),
                            arguments: serde_json::json!({"city": "Paris"}),
                        },
                        ToolCallRequest {
                            id: "tc_2".to_string(),
                            name: "get_time".to_string(),
                            arguments: serde_json::json!({"tz": "CET"}),
                        },
                    ]),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let payload = AnthropicClient::to_messages_payload(&req);
        let messages = payload["messages"].as_array().unwrap();

        let asst = &messages[1];
        assert_eq!(asst["role"], "assistant");
        let blocks = asst["content"].as_array().unwrap();
        assert_eq!(blocks.len(), 3); // text + 2 tool_use

        assert_eq!(blocks[0]["type"], "text");
        assert_eq!(blocks[0]["text"], "Let me check.");

        assert_eq!(blocks[1]["type"], "tool_use");
        assert_eq!(blocks[1]["id"], "tc_1");
        assert_eq!(blocks[1]["name"], "get_weather");
        assert_eq!(blocks[1]["input"], serde_json::json!({"city": "Paris"}));

        assert_eq!(blocks[2]["type"], "tool_use");
        assert_eq!(blocks[2]["id"], "tc_2");
        assert_eq!(blocks[2]["name"], "get_time");
    }

    // ── normalize_messages_json ──────────────────────────────────────────

    #[test]
    fn test_normalize_text_response() {
        let raw = serde_json::json!({
            "id": "msg_test",
            "type": "message",
            "role": "assistant",
            "model": "claude-3-5-sonnet-20241022",
            "content": [
                {"type": "text", "text": "Hello, world!"}
            ],
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 10, "output_tokens": 5}
        });
        let resp = AnthropicClient::normalize_messages_json(raw).unwrap();

        assert_eq!(resp.id.as_deref(), Some("msg_test"));
        assert_eq!(resp.model.as_deref(), Some("claude-3-5-sonnet-20241022"));
        assert_eq!(
            resp.choices[0].message.content.as_deref(),
            Some("Hello, world!")
        );
        assert_eq!(resp.choices[0].message.role, "assistant");
        assert!(resp.choices[0].message.tool_calls.is_none());
        assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("stop"));
    }

    #[test]
    fn test_normalize_tool_use_response() {
        let raw = serde_json::json!({
            "id": "msg_tc",
            "model": "claude-3-5-sonnet-20241022",
            "content": [
                {
                    "type": "tool_use",
                    "id": "toolu_abc",
                    "name": "get_weather",
                    "input": {"city": "Paris"}
                }
            ],
            "stop_reason": "tool_use",
            "usage": {"input_tokens": 20, "output_tokens": 30}
        });
        let resp = AnthropicClient::normalize_messages_json(raw).unwrap();

        assert!(resp.choices[0].message.content.is_none());
        assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("tool_calls"));

        let reqs = resp.choices[0].message.tool_calls.as_ref().unwrap();
        assert_eq!(reqs.len(), 1);
        assert_eq!(reqs[0].id, "toolu_abc");
        assert_eq!(reqs[0].name, "get_weather");
        assert_eq!(reqs[0].arguments, serde_json::json!({"city": "Paris"}));

        let tcs = resp.tool_calls.as_ref().unwrap();
        assert_eq!(tcs.len(), 1);
        assert_eq!(tcs[0].name, "get_weather");
    }

    #[test]
    fn test_normalize_mixed_response() {
        let raw = serde_json::json!({
            "id": "msg_mix",
            "model": "claude-3-5-sonnet-20241022",
            "content": [
                {"type": "text", "text": "Let me check. "},
                {
                    "type": "tool_use",
                    "id": "toolu_1",
                    "name": "search",
                    "input": {"q": "rust"}
                }
            ],
            "stop_reason": "tool_use",
            "usage": {"input_tokens": 15, "output_tokens": 25}
        });
        let resp = AnthropicClient::normalize_messages_json(raw).unwrap();

        assert_eq!(
            resp.choices[0].message.content.as_deref(),
            Some("Let me check. ")
        );
        let reqs = resp.choices[0].message.tool_calls.as_ref().unwrap();
        assert_eq!(reqs.len(), 1);
        assert_eq!(reqs[0].name, "search");
    }

    #[test]
    fn test_normalize_usage_mapping() {
        let raw = serde_json::json!({
            "id": "msg_u",
            "model": "claude-3-haiku",
            "content": [{"type": "text", "text": "ok"}],
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 100, "output_tokens": 50}
        });
        let resp = AnthropicClient::normalize_messages_json(raw).unwrap();
        let usage = resp.usage.unwrap();

        assert_eq!(usage.prompt_tokens, Some(100));
        assert_eq!(usage.completion_tokens, Some(50));
        assert_eq!(usage.total_tokens, Some(150));
    }

    #[test]
    fn test_normalize_stop_reason() {
        for (anthropic, expected) in [
            ("end_turn", "stop"),
            ("tool_use", "tool_calls"),
            ("max_tokens", "length"),
        ] {
            let raw = serde_json::json!({
                "id": "msg_sr",
                "model": "claude-3-haiku",
                "content": [],
                "stop_reason": anthropic,
                "usage": {"input_tokens": 1, "output_tokens": 1}
            });
            let resp = AnthropicClient::normalize_messages_json(raw).unwrap();
            assert_eq!(
                resp.choices[0].finish_reason.as_deref(),
                Some(expected),
                "stop_reason '{}' should map to '{}'",
                anthropic,
                expected
            );
        }
    }

    // ── parse_anthropic_chunk ────────────────────────────────────────────

    #[test]
    fn test_parse_anthropic_chunk_text_delta() {
        let data = r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}"#;
        let events = parse_anthropic_chunk("content_block_delta", data).unwrap();
        assert_eq!(events.len(), 1);
        match &events[0] {
            StreamEvent::ContentDelta { delta } => assert_eq!(delta, "Hello"),
            other => panic!("expected ContentDelta, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_anthropic_chunk_tool_use_start() {
        let data = r#"{"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_abc","name":"get_weather","input":{}}}"#;
        let events = parse_anthropic_chunk("content_block_start", data).unwrap();
        assert_eq!(events.len(), 1);
        match &events[0] {
            StreamEvent::ToolCallStart { index, id, name } => {
                assert_eq!(*index, 1);
                assert_eq!(id, "toolu_abc");
                assert_eq!(name, "get_weather");
            }
            other => panic!("expected ToolCallStart, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_anthropic_chunk_text_block_start_ignored() {
        let data =
            r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#;
        let events = parse_anthropic_chunk("content_block_start", data).unwrap();
        assert!(events.is_empty());
    }

    #[test]
    fn test_parse_anthropic_chunk_input_json_delta() {
        let data = r#"{"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"city\":"}}"#;
        let events = parse_anthropic_chunk("content_block_delta", data).unwrap();
        assert_eq!(events.len(), 1);
        match &events[0] {
            StreamEvent::ToolCallDelta {
                index,
                arguments_delta,
            } => {
                assert_eq!(*index, 1);
                assert_eq!(arguments_delta, "{\"city\":");
            }
            other => panic!("expected ToolCallDelta, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_anthropic_chunk_message_delta_done() {
        let data = r#"{"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":42}}"#;
        let events = parse_anthropic_chunk("message_delta", data).unwrap();
        assert_eq!(events.len(), 1);
        match &events[0] {
            StreamEvent::Done {
                finish_reason,
                usage,
            } => {
                assert_eq!(finish_reason.as_deref(), Some("stop"));
                let u = usage.as_ref().unwrap();
                assert_eq!(u.completion_tokens, Some(42));
                assert!(u.prompt_tokens.is_none());
            }
            other => panic!("expected Done, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_anthropic_chunk_message_start() {
        let data = r#"{"type":"message_start","message":{"id":"msg_stream","model":"claude-3-5-sonnet","role":"assistant","content":[],"usage":{"input_tokens":10,"output_tokens":1}}}"#;
        let events = parse_anthropic_chunk("message_start", data).unwrap();
        assert_eq!(events.len(), 1);
        match &events[0] {
            StreamEvent::StreamStart { id, model } => {
                assert_eq!(id.as_deref(), Some("msg_stream"));
                assert_eq!(model.as_deref(), Some("claude-3-5-sonnet"));
            }
            other => panic!("expected StreamStart, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_anthropic_chunk_content_block_stop_ignored() {
        let data = r#"{"type":"content_block_stop","index":0}"#;
        let events = parse_anthropic_chunk("content_block_stop", data).unwrap();
        assert!(events.is_empty());
    }

    // ── Full SSE transcript ──────────────────────────────────────────────

    #[test]
    fn test_anthropic_sse_full_transcript() {
        let transcript = [
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_t\",\"model\":\"claude-3-5-sonnet\",\"role\":\"assistant\",\"content\":[],\"usage\":{\"input_tokens\":10,\"output_tokens\":1}}}\n\n",
            "event: content_block_start\n",
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}\n\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\" world\"}}\n\n",
            "event: content_block_stop\n",
            "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
            "event: message_delta\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":15}}\n\n",
            "event: message_stop\n",
            "data: {\"type\":\"message_stop\"}\n\n",
        ]
        .join("");

        let mut parser = SseParser::new();
        parser.feed(transcript.as_bytes());

        let mut all_events = Vec::new();
        while let Some((event_type, data)) = parser.next_typed_event() {
            let evt = event_type.as_deref().unwrap_or("");
            if evt == "message_stop" {
                break;
            }
            if evt == "ping" {
                continue;
            }
            let events = parse_anthropic_chunk(evt, &data).unwrap();
            all_events.extend(events);
        }

        // StreamStart, (text block start ignored), ContentDelta x2,
        // (block stop ignored), Done
        assert_eq!(
            all_events.len(),
            4,
            "expected 4 events: start + 2 deltas + done, got {:?}",
            all_events
        );

        assert!(
            matches!(&all_events[0], StreamEvent::StreamStart { id, model }
                if id.as_deref() == Some("msg_t") && model.as_deref() == Some("claude-3-5-sonnet"))
        );
        assert!(matches!(&all_events[1], StreamEvent::ContentDelta { delta } if delta == "Hello"));
        assert!(matches!(&all_events[2], StreamEvent::ContentDelta { delta } if delta == " world"));
        match &all_events[3] {
            StreamEvent::Done {
                finish_reason,
                usage,
            } => {
                assert_eq!(finish_reason.as_deref(), Some("stop"));
                let u = usage.as_ref().expect("usage");
                assert_eq!(u.completion_tokens, Some(15));
            }
            other => panic!("expected Done, got {:?}", other),
        }
    }
}
