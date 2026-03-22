use crate::{
    model_client::{
        ApiMode, ClientCapabilities, ClientConfig, ClientError, HttpModelClient, ModelClient,
    },
    types::{ChatMessage, LlmChoice, LlmRequest, LlmResponse, Usage},
};

#[derive(Clone)]
pub struct OpenAIClient {
    inner: HttpModelClient,
}

impl OpenAIClient {
    pub fn new(config: ClientConfig) -> Self {
        log::debug!("OpenAIClient::new api_mode={:?}", config.api_mode);
        Self {
            inner: HttpModelClient::new(config),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::LlmRequest;

    fn mk_client(api_mode: ApiMode) -> OpenAIClient {
        OpenAIClient::new(ClientConfig {
            base_url: "http://localhost:1234".to_string(),
            api_key: None,
            default_headers: Default::default(),
            api_mode: Some(api_mode),
            ..ClientConfig::default()
        })
    }

    #[test]
    fn test_decide_mode_auto_routes_gpt5_to_responses() {
        let client = mk_client(ApiMode::Auto);
        assert!(matches!(
            client.decide_mode("gpt-5-mini"),
            ApiMode::Responses
        ));
        assert!(matches!(client.decide_mode("gpt-4o-mini"), ApiMode::Chat));
    }

    #[test]
    fn test_to_responses_payload_moves_system_messages_to_instructions() {
        let request = LlmRequest {
            model: "gpt-5".to_string(),
            messages: vec![
                ChatMessage {
                    role: "system".to_string(),
                    content: "be concise".to_string(),
                },
                ChatMessage {
                    role: "user".to_string(),
                    content: "hello".to_string(),
                },
            ],
            ..Default::default()
        };

        let payload = OpenAIClient::to_responses_payload(&request);
        assert_eq!(payload["model"], "gpt-5");
        assert_eq!(payload["instructions"], "be concise");
        assert_eq!(payload["input"].as_array().map(|a| a.len()), Some(1));
        assert_eq!(payload["input"][0]["role"], "user");
        assert_eq!(payload["input"][0]["content"], "hello");
    }
}

impl ModelClient for OpenAIClient {
    fn capabilities(&self) -> ClientCapabilities {
        self.inner.capabilities()
    }

    async fn llm_request(
        &self,
        request: serde_json::Value,
    ) -> Result<serde_json::Value, ClientError> {
        self.inner.llm_request(request).await
    }
}

impl OpenAIClient {
    fn decide_mode(&self, model: &str) -> ApiMode {
        let decided = match self.inner.config().api_mode.unwrap_or(ApiMode::Chat) {
            ApiMode::Chat => ApiMode::Chat,
            ApiMode::Responses => ApiMode::Responses,
            ApiMode::Auto => {
                if model.starts_with("gpt-5") {
                    ApiMode::Responses
                } else {
                    ApiMode::Chat
                }
            }
        };
        log::debug!(
            "OpenAIClient::decide_mode model={} decided={:?}",
            model,
            decided
        );
        decided
    }

    fn map_tools_for_chat(
        tools: &Option<Vec<crate::types::ToolSchema>>,
    ) -> Option<serde_json::Value> {
        if let Some(list) = tools {
            let mapped: Vec<serde_json::Value> = list
                .iter()
                .map(|t| {
                    serde_json::json!({
                        "type": "function",
                        "function": {
                            "name": t.name,
                            "description": t.description,
                            "parameters": t.parameters
                        }
                    })
                })
                .collect();
            Some(serde_json::Value::Array(mapped))
        } else {
            None
        }
    }

    fn to_chat_payload(req: &LlmRequest) -> serde_json::Value {
        let mut obj = serde_json::Map::new();
        obj.insert(
            "model".to_string(),
            serde_json::Value::String(req.model.clone()),
        );
        obj.insert(
            "messages".to_string(),
            serde_json::to_value(&req.messages).unwrap_or(serde_json::Value::Null),
        );
        if let Some(tools) = Self::map_tools_for_chat(&req.tools) {
            obj.insert("tools".to_string(), tools);
        }
        if let Some(choice) = &req.tool_choice {
            obj.insert("tool_choice".to_string(), choice.clone());
        }
        if let Some(temp) = req.temperature {
            obj.insert("temperature".to_string(), serde_json::Value::from(temp));
        }
        if let Some(mt) = req.max_tokens {
            obj.insert("max_tokens".to_string(), serde_json::Value::from(mt));
        }
        if let Some(ext) = &req.extensions {
            for (k, v) in ext.iter() {
                obj.insert(k.clone(), v.clone());
            }
        }
        let payload = serde_json::Value::Object(obj);
        log::debug!(
            "OpenAIClient::to_chat_payload keys={}",
            payload.as_object().map(|o| o.len()).unwrap_or(0)
        );
        payload
    }

    fn split_instructions_and_messages(
        messages: &[ChatMessage],
    ) -> (Option<String>, Vec<ChatMessage>) {
        let mut instructions_segments: Vec<String> = Vec::new();
        let mut rest: Vec<ChatMessage> = Vec::new();
        for m in messages {
            if m.role == "system" {
                instructions_segments.push(m.content.clone());
            } else {
                rest.push(ChatMessage {
                    role: m.role.clone(),
                    content: m.content.clone(),
                });
            }
        }
        let instructions = if instructions_segments.is_empty() {
            None
        } else {
            Some(instructions_segments.join("\n"))
        };
        log::debug!(
            "OpenAIClient::split_instructions_and_messages instructions_len={} rest_count={}",
            instructions.as_ref().map(|s| s.len()).unwrap_or(0),
            rest.len()
        );
        (instructions, rest)
    }

    fn map_tools_for_responses(
        tools: &Option<Vec<crate::types::ToolSchema>>,
    ) -> Option<serde_json::Value> {
        if let Some(list) = tools {
            let mapped: Vec<serde_json::Value> = list
                .iter()
                .map(|t| {
                    let mut obj = serde_json::Map::new();
                    obj.insert(
                        "type".to_string(),
                        serde_json::Value::String("function".to_string()),
                    );
                    obj.insert(
                        "name".to_string(),
                        serde_json::Value::String(t.name.clone()),
                    );
                    if let Some(desc) = &t.description {
                        obj.insert(
                            "description".to_string(),
                            serde_json::Value::String(desc.clone()),
                        );
                    }
                    obj.insert("parameters".to_string(), t.parameters.clone());
                    if let Some(strict) = t.strict {
                        obj.insert("strict".to_string(), serde_json::Value::Bool(strict));
                    }
                    serde_json::Value::Object(obj)
                })
                .collect();
            log::debug!(
                "OpenAIClient::map_tools_for_responses count={}",
                mapped.len()
            );
            Some(serde_json::Value::Array(mapped))
        } else {
            None
        }
    }

    fn to_responses_payload(req: &LlmRequest) -> serde_json::Value {
        let (instructions, inputs) = Self::split_instructions_and_messages(&req.messages);
        let tools = Self::map_tools_for_responses(&req.tools);
        let mut obj = serde_json::Map::new();
        obj.insert(
            "model".to_string(),
            serde_json::Value::String(req.model.clone()),
        );
        if let Some(instr) = instructions {
            obj.insert("instructions".to_string(), serde_json::Value::String(instr));
        }
        obj.insert(
            "input".to_string(),
            serde_json::to_value(inputs).unwrap_or(serde_json::Value::Null),
        );
        if let Some(t) = tools {
            obj.insert("tools".to_string(), t);
        }
        if let Some(choice) = &req.tool_choice {
            obj.insert("tool_choice".to_string(), choice.clone());
        }
        if let Some(temp) = req.temperature {
            obj.insert("temperature".to_string(), serde_json::Value::from(temp));
        }
        if let Some(mt) = req.max_tokens {
            obj.insert("max_output_tokens".to_string(), serde_json::Value::from(mt));
        }
        if let Some(ext) = &req.extensions {
            for (k, v) in ext.iter() {
                obj.insert(k.clone(), v.clone());
            }
        }
        let payload = serde_json::Value::Object(obj);
        log::debug!(
            "OpenAIClient::to_responses_payload keys={}",
            payload.as_object().map(|o| o.len()).unwrap_or(0)
        );
        payload
    }

    fn extract_tool_calls_from_chat(
        raw: &serde_json::Value,
    ) -> Option<Vec<crate::types::ToolCall>> {
        let mut tool_calls: Vec<crate::types::ToolCall> = Vec::new();
        if let Some(choices) = raw.get("choices").and_then(|v| v.as_array()) {
            for ch in choices {
                if let Some(msg) = ch.get("message") {
                    if let Some(tc_arr) = msg.get("tool_calls").and_then(|v| v.as_array()) {
                        for tc in tc_arr {
                            let id = tc.get("id").and_then(|v| v.as_str()).map(|s| s.to_string());
                            let call_id = None; // Chat API uses id only
                            let name = tc
                                .get("function")
                                .and_then(|f| f.get("name"))
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            let args_str = tc
                                .get("function")
                                .and_then(|f| f.get("arguments"))
                                .and_then(|v| v.as_str())
                                .unwrap_or("{}");
                            let arguments = serde_json::from_str::<serde_json::Value>(args_str)
                                .unwrap_or(serde_json::json!({"_raw": args_str}));
                            if !name.is_empty() {
                                tool_calls.push(crate::types::ToolCall {
                                    id,
                                    call_id,
                                    name,
                                    arguments,
                                });
                            }
                        }
                    }
                }
            }
        }
        if tool_calls.is_empty() {
            None
        } else {
            Some(tool_calls)
        }
    }

    fn extract_tool_calls_from_responses(
        raw: &serde_json::Value,
    ) -> Option<Vec<crate::types::ToolCall>> {
        let mut tool_calls: Vec<crate::types::ToolCall> = Vec::new();
        if let Some(items) = raw.get("output").and_then(|v| v.as_array()) {
            for item in items {
                if item.get("type").and_then(|t| t.as_str()) == Some("function_call") {
                    let name = item
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let args_str = item
                        .get("arguments")
                        .and_then(|v| v.as_str())
                        .unwrap_or("{}");
                    let arguments = serde_json::from_str::<serde_json::Value>(args_str)
                        .unwrap_or(serde_json::json!({"_raw": args_str}));
                    let call_id = item
                        .get("call_id")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    let id = item
                        .get("id")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    if !name.is_empty() {
                        tool_calls.push(crate::types::ToolCall {
                            id,
                            call_id,
                            name,
                            arguments,
                        });
                    }
                }
            }
        }
        if tool_calls.is_empty() {
            None
        } else {
            Some(tool_calls)
        }
    }

    fn normalize_responses_json(raw: serde_json::Value) -> Result<LlmResponse, ClientError> {
        let output_text = raw
            .get("output_text")
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .or_else(|| {
                let mut buf = String::new();
                if let Some(items) = raw.get("output").and_then(|v| v.as_array()) {
                    for item in items {
                        if item.get("type").and_then(|t| t.as_str()) == Some("output_text") {
                            if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                                buf.push_str(text);
                            }
                        }
                    }
                }
                if buf.is_empty() {
                    None
                } else {
                    Some(buf)
                }
            });
        let content = output_text.unwrap_or_default();
        log::debug!(
            "OpenAIClient::normalize_responses_json content_len={}",
            content.len()
        );
        let role = "assistant".to_string();
        let choice = LlmChoice {
            index: 0,
            message: ChatMessage { role, content },
            finish_reason: None,
        };
        let id = raw
            .get("id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let model = raw
            .get("model")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let usage = raw
            .get("usage")
            .and_then(|u| serde_json::from_value::<Usage>(u.clone()).ok());
        let tool_calls = Self::extract_tool_calls_from_responses(&raw);
        Ok(LlmResponse {
            id,
            created: None,
            model,
            choices: vec![choice],
            usage,
            tool_calls,
        })
    }

    fn normalize_chat_json(raw: serde_json::Value) -> Result<LlmResponse, ClientError> {
        // Content may be null when tool_calls are present. Coerce to empty string.
        let content = raw
            .get("choices")
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.get(0))
            .and_then(|ch| ch.get("message"))
            .and_then(|msg| msg.get("content"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let role = raw
            .get("choices")
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.get(0))
            .and_then(|ch| ch.get("message"))
            .and_then(|msg| msg.get("role"))
            .and_then(|v| v.as_str())
            .unwrap_or("assistant")
            .to_string();
        let choice = LlmChoice {
            index: 0,
            message: ChatMessage { role, content },
            finish_reason: None,
        };
        let id = raw
            .get("id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let created = raw.get("created").and_then(|v| v.as_u64());
        let model = raw
            .get("model")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let usage = raw
            .get("usage")
            .and_then(|u| serde_json::from_value::<Usage>(u.clone()).ok());
        let tool_calls = Self::extract_tool_calls_from_chat(&raw);
        Ok(LlmResponse {
            id,
            created,
            model,
            choices: vec![choice],
            usage,
            tool_calls,
        })
    }
}

// ---------------------------------------------------------------------------
// Native-only: streaming LLM request
// ---------------------------------------------------------------------------

#[cfg(not(target_arch = "wasm32"))]
impl OpenAIClient {
    /// Send a streaming LLM request and return a typed event stream.
    ///
    /// This is the streaming counterpart of [`OpenAIClient::llm`]. It
    /// injects `"stream": true` and `"stream_options": { "include_usage": true }`
    /// into the request payload, opens an SSE connection via
    /// [`HttpModelClient::post_sse`], and returns a
    /// [`crate::stream::LlmEventStream`] that yields [`crate::stream::StreamEvent`]s
    /// in real-time.
    ///
    /// The returned stream completes when the provider sends `data: [DONE]`,
    /// the connection closes, or an error occurs.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use futures::StreamExt;
    /// use llm_client::providers::OpenAIClient;
    /// use llm_client::{LlmRequest, ChatMessage, StreamEvent};
    /// use llm_client::model_client::ClientConfig;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = OpenAIClient::new(ClientConfig {
    ///     base_url: "https://api.openai.com".into(),
    ///     api_key: Some("sk-...".into()),
    ///     ..Default::default()
    /// });
    ///
    /// let req = LlmRequest {
    ///     model: "gpt-4".into(),
    ///     messages: vec![ChatMessage { role: "user".into(), content: "Hello".into() }],
    ///     ..Default::default()
    /// };
    ///
    /// let mut stream = client.llm_stream(req).await?;
    /// while let Some(event) = stream.next().await {
    ///     match event? {
    ///         StreamEvent::ContentDelta { delta } => print!("{}", delta),
    ///         StreamEvent::Done { .. } => break,
    ///         _ => {}
    ///     }
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn llm_stream(
        &self,
        req: LlmRequest,
    ) -> Result<crate::stream::LlmEventStream, ClientError> {
        let mode = self.decide_mode(&req.model);
        let (path, mut payload) = match mode {
            ApiMode::Chat => ("/v1/chat/completions", Self::to_chat_payload(&req)),
            ApiMode::Responses | ApiMode::Auto => {
                ("/v1/responses", Self::to_responses_payload(&req))
            }
        };

        // Inject streaming flags
        if let Some(obj) = payload.as_object_mut() {
            obj.insert("stream".to_string(), serde_json::Value::Bool(true));
            // Request usage stats in the final chunk
            obj.insert(
                "stream_options".to_string(),
                serde_json::json!({ "include_usage": true }),
            );
        }

        log::info!("OpenAIClient::llm_stream mode={:?} endpoint={}", mode, path);

        let response = self.inner.post_sse(path, payload).await?;

        Ok(crate::stream::sse_event_stream(response))
    }

    /// Stream from a pre-built JSON payload, bypassing `LlmRequest` deserialization.
    ///
    /// Use this when the caller already has a fully-formed OpenAI-compatible
    /// request body (e.g. from the tool-calling orchestrator where messages
    /// contain `tool_calls`, `tool_call_id`, and nullable `content` fields
    /// that `ChatMessage` cannot represent).
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn llm_stream_raw(
        &self,
        mut payload: serde_json::Value,
    ) -> Result<crate::stream::LlmEventStream, ClientError> {
        // Inject streaming flags
        if let Some(obj) = payload.as_object_mut() {
            obj.insert("stream".to_string(), serde_json::Value::Bool(true));
            obj.insert(
                "stream_options".to_string(),
                serde_json::json!({ "include_usage": true }),
            );
        }

        let path = "/v1/chat/completions";
        log::info!("OpenAIClient::llm_stream_raw endpoint={}", path);

        let response = self.inner.post_sse(path, payload).await?;

        Ok(crate::stream::sse_event_stream(response))
    }
}

impl OpenAIClient {
    pub async fn llm(&self, req: LlmRequest) -> Result<LlmResponse, ClientError> {
        let mode = self.decide_mode(&req.model);
        match mode {
            ApiMode::Chat => {
                let value = Self::to_chat_payload(&req);
                log::info!("OpenAIClient::llm mode=chat endpoint=/v1/chat/completions");
                let raw = self.inner.post_json("/v1/chat/completions", value).await?;
                // Normalize chat response to handle null content when tool_calls are present
                let mut parsed = Self::normalize_chat_json(raw.clone())?;
                // Enrich with tool calls if present (already included by normalize, but keep to ensure)
                parsed.tool_calls = Self::extract_tool_calls_from_chat(&raw);
                Ok(parsed)
            }
            ApiMode::Responses | ApiMode::Auto => {
                let value = Self::to_responses_payload(&req);
                log::info!("OpenAIClient::llm mode=responses endpoint=/v1/responses");
                let raw = self.inner.post_json("/v1/responses", value).await?;
                let normalized = Self::normalize_responses_json(raw)?;
                Ok(normalized)
            }
        }
    }
}
