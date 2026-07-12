use crate::{
    auth::AuthProvider,
    error::LlmError,
    model_client::{ClientCapabilities, HttpModelClient},
    multimodal::{FileDetailPolicy, validate_multimodal_inputs},
    provider::LlmProvider,
    stream::{LlmEventStream, StreamEvent},
    types::{
        ChatContentPart, ChatMessage, LlmChoice, LlmRequest, LlmResponse, ToolCall,
        ToolCallRequest, ToolChoice, Usage,
    },
};
use protocol_transport_core::StreamingPolicy;
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone)]
pub(crate) struct GoogleGenerateContentClient {
    inner: HttpModelClient,
}

impl GoogleGenerateContentClient {
    pub(crate) fn new(
        base_url: String,
        default_headers: HashMap<String, String>,
        streaming: Option<StreamingPolicy>,
        auth: Arc<dyn AuthProvider>,
    ) -> Self {
        log::debug!("GoogleGenerateContentClient::new");
        Self {
            inner: HttpModelClient::new(base_url, default_headers, streaming, auth),
        }
    }
}

impl GoogleGenerateContentClient {
    fn validate_multimodal_request(req: &LlmRequest) -> Result<(), LlmError> {
        validate_multimodal_inputs(req, FileDetailPolicy::PdfOnly)
    }

    #[cfg(test)]
    fn to_generate_content_payload(req: &LlmRequest) -> Value {
        let name_map = ToolNameMap::from_request(req);
        Self::to_generate_content_payload_with_names(req, &name_map)
    }

    fn to_generate_content_payload_with_names(req: &LlmRequest, name_map: &ToolNameMap) -> Value {
        let mut obj = Map::new();
        let mut contents = Vec::new();
        let mut system_parts = Vec::new();

        for msg in &req.messages {
            match msg.role.as_str() {
                "system" => {
                    if let Some(content) = nonempty_content(msg) {
                        system_parts.push(json!({ "text": content }));
                    }
                }
                "assistant" => contents.push(map_assistant_message(msg, name_map)),
                "tool" => contents.push(map_tool_result_message(msg, name_map)),
                _ => contents.push(map_user_message(msg)),
            }
        }

        obj.insert("contents".to_string(), Value::Array(contents));

        if !system_parts.is_empty() {
            obj.insert(
                "systemInstruction".to_string(),
                json!({
                    "role": "system",
                    "parts": system_parts,
                }),
            );
        }

        let mut generation_config = Map::new();
        if let Some(temp) = req.temperature {
            generation_config.insert("temperature".to_string(), json!(temp));
        }
        if let Some(max_tokens) = req.max_tokens {
            generation_config.insert("maxOutputTokens".to_string(), json!(max_tokens));
        }
        if !generation_config.is_empty() {
            obj.insert(
                "generationConfig".to_string(),
                Value::Object(generation_config),
            );
        }

        if let Some(tools) = &req.tools {
            if !tools.is_empty() {
                let declarations = tools
                    .iter()
                    .map(|tool| {
                        let mut declaration = Map::new();
                        declaration.insert(
                            "name".to_string(),
                            Value::String(name_map.to_wire(&tool.name)),
                        );
                        if let Some(description) = &tool.description {
                            declaration.insert(
                                "description".to_string(),
                                Value::String(description.clone()),
                            );
                        }
                        declaration.insert("parameters".to_string(), to_gemini_schema(&tool.parameters));
                        Value::Object(declaration)
                    })
                    .collect::<Vec<_>>();
                obj.insert(
                    "tools".to_string(),
                    json!([{ "functionDeclarations": declarations }]),
                );
            }
        }

        if let Some(choice) = &req.tool_choice {
            obj.insert("toolConfig".to_string(), map_tool_choice(choice));
        }

        if let Some(ext) = &req.extensions {
            for (key, value) in ext {
                if is_google_generate_content_extension(key) {
                    obj.insert(key.clone(), value.clone());
                }
            }
        }

        Value::Object(obj)
    }

    #[cfg(test)]
    fn normalize_generate_content_json(raw: Value, model: Option<String>) -> Result<LlmResponse, LlmError> {
        Self::normalize_generate_content_json_with_names(raw, model, &ToolNameMap::default())
    }

    fn normalize_generate_content_json_with_names(
        raw: Value,
        model: Option<String>,
        name_map: &ToolNameMap,
    ) -> Result<LlmResponse, LlmError> {
        let response_id = raw
            .get("responseId")
            .or_else(|| raw.get("id"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        let model = raw
            .get("modelVersion")
            .or_else(|| raw.get("model"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .or(model);

        let candidate = raw
            .get("candidates")
            .and_then(Value::as_array)
            .and_then(|candidates| candidates.first());
        let parts = candidate
            .and_then(|candidate| candidate.get("content"))
            .and_then(|content| content.get("parts"))
            .and_then(Value::as_array);

        let mut text_parts = Vec::new();
        let mut tool_call_requests = Vec::new();
        let mut tool_calls = Vec::new();

        if let Some(parts) = parts {
            for (index, part) in parts.iter().enumerate() {
                if let Some(text) = part.get("text").and_then(Value::as_str) {
                    text_parts.push(text.to_string());
                }
                if let Some(function_call) = part.get("functionCall") {
                    let name = function_call
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    if name.is_empty() {
                        continue;
                    }
                    let name = name_map.to_original(&name);
                    let id = function_call
                        .get("id")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned)
                        .unwrap_or_else(|| format!("gemini_call_{index}"));
                    let arguments = function_call
                        .get("args")
                        .cloned()
                        .unwrap_or_else(|| json!({}));
                    tool_call_requests.push(ToolCallRequest {
                        id: id.clone(),
                        name: name.clone(),
                        arguments: arguments.clone(),
                    });
                    tool_calls.push(ToolCall {
                        id: Some(id),
                        call_id: None,
                        name,
                        arguments,
                    });
                }
            }
        }

        let finish_reason = candidate
            .and_then(|candidate| candidate.get("finishReason"))
            .and_then(Value::as_str)
            .map(map_finish_reason);

        let content = if text_parts.is_empty() {
            None
        } else {
            Some(text_parts.join(""))
        };

        let usage = raw.get("usageMetadata").map(|usage| Usage {
            prompt_tokens: usage
                .get("promptTokenCount")
                .and_then(Value::as_u64)
                .map(|value| value as u32),
            completion_tokens: usage
                .get("candidatesTokenCount")
                .and_then(Value::as_u64)
                .map(|value| value as u32),
            total_tokens: usage
                .get("totalTokenCount")
                .and_then(Value::as_u64)
                .map(|value| value as u32),
        });

        Ok(LlmResponse {
            id: response_id,
            created: None,
            model,
            choices: vec![LlmChoice {
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
            }],
            usage,
            tool_calls: if tool_calls.is_empty() {
                None
            } else {
                Some(tool_calls)
            },
        })
    }

    pub async fn llm(&self, req: LlmRequest) -> Result<LlmResponse, LlmError> {
        Self::validate_multimodal_request(&req)?;
        let name_map = ToolNameMap::from_request(&req);
        let path = generate_content_path(&req.model);
        let payload = Self::to_generate_content_payload_with_names(&req, &name_map);
        log::info!("GoogleGenerateContentClient::llm endpoint={}", path);
        let raw = self.inner.post_json(&path, payload).await?;
        Self::normalize_generate_content_json_with_names(raw, Some(req.model), &name_map)
    }

    pub async fn llm_stream(&self, req: LlmRequest) -> Result<LlmEventStream, LlmError> {
        let response = self.llm(req).await?;
        Ok(response_to_buffered_stream(response))
    }
}

fn map_user_message(msg: &ChatMessage) -> Value {
    json!({
        "role": "user",
        "parts": text_parts(msg),
    })
}

fn map_assistant_message(msg: &ChatMessage, name_map: &ToolNameMap) -> Value {
    let mut parts = Vec::new();
    if let Some(content) = nonempty_content(msg) {
        parts.push(json!({ "text": content }));
    }
    if let Some(tool_calls) = &msg.tool_calls {
        for call in tool_calls {
            let mut function_call = Map::new();
            function_call.insert("name".to_string(), Value::String(name_map.to_wire(&call.name)));
            function_call.insert("args".to_string(), call.arguments.clone());
            parts.push(json!({ "functionCall": function_call }));
        }
    }
    json!({
        "role": "model",
        "parts": parts,
    })
}

fn map_tool_result_message(msg: &ChatMessage, name_map: &ToolNameMap) -> Value {
    let name = msg
        .name
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("tool");
    let name = name_map.to_wire(name);
    let response = msg
        .content
        .as_deref()
        .and_then(|content| serde_json::from_str::<Value>(content).ok())
        .unwrap_or_else(|| json!({ "content": msg.content.clone().unwrap_or_default() }));
    json!({
        "parts": [{
            "functionResponse": {
                "name": name,
                "response": ensure_object_response(response),
            }
        }]
    })
}

fn text_parts(msg: &ChatMessage) -> Vec<Value> {
    if let Some(parts) = &msg.content_parts {
        return parts
            .iter()
            .filter_map(|part| match part {
                ChatContentPart::Text { text } if text.trim().is_empty() => None,
                ChatContentPart::Text { text } => Some(json!({ "text": text })),
                ChatContentPart::ImageBase64 {
                    media_type, data, ..
                } => Some(json!({
                    "inlineData": {
                        "mimeType": media_type,
                        "data": data,
                    }
                })),
                ChatContentPart::ImageUrl { url, .. } => Some(json!({
                    "fileData": {
                        "fileUri": url,
                    }
                })),
                ChatContentPart::FileBase64 {
                    media_type, data, ..
                } => Some(json!({
                    "inlineData": {
                        "mimeType": media_type,
                        "data": data,
                    }
                })),
            })
            .collect();
    }
    vec![json!({ "text": msg.content.clone().unwrap_or_default() })]
}

fn nonempty_content(msg: &ChatMessage) -> Option<&str> {
    msg.content
        .as_deref()
        .map(str::trim)
        .filter(|content| !content.is_empty())
}

fn ensure_object_response(value: Value) -> Value {
    if value.is_object() {
        value
    } else {
        json!({ "content": value })
    }
}

#[derive(Debug, Clone, Default)]
struct ToolNameMap {
    original_to_wire: HashMap<String, String>,
    wire_to_original: HashMap<String, String>,
}

impl ToolNameMap {
    fn from_request(req: &LlmRequest) -> Self {
        let mut map = Self::default();
        if let Some(tools) = &req.tools {
            for tool in tools {
                map.insert(&tool.name);
            }
        }
        map
    }

    fn insert(&mut self, original: &str) {
        let original = original.trim();
        if original.is_empty() || self.original_to_wire.contains_key(original) {
            return;
        }
        let wire = gemini_function_name(original);
        self.original_to_wire
            .insert(original.to_string(), wire.clone());
        self.wire_to_original.insert(wire, original.to_string());
    }

    fn to_wire(&self, original: &str) -> String {
        self.original_to_wire
            .get(original)
            .cloned()
            .unwrap_or_else(|| gemini_function_name(original))
    }

    fn to_original(&self, wire: &str) -> String {
        self.wire_to_original
            .get(wire)
            .cloned()
            .unwrap_or_else(|| wire.to_string())
    }
}

fn gemini_function_name(original: &str) -> String {
    let trimmed = original.trim();
    let mut safe = String::with_capacity(trimmed.len());
    for ch in trimmed.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            safe.push(ch);
        } else {
            safe.push('_');
        }
    }
    if safe
        .chars()
        .next()
        .is_none_or(|ch| !(ch.is_ascii_alphabetic() || ch == '_'))
    {
        safe = format!("fn_{safe}");
    }
    let changed = safe != trimmed || safe.len() > 64;
    if changed {
        let hash = stable_name_hash(trimmed);
        let suffix = format!("_{hash:08x}");
        let max_prefix = 64usize.saturating_sub(suffix.len());
        safe.truncate(max_prefix);
        safe.push_str(&suffix);
    }
    if safe.is_empty() {
        "fn_empty".to_string()
    } else {
        safe
    }
}

fn stable_name_hash(value: &str) -> u32 {
    let mut hash = 0x811c9dc5u32;
    for byte in value.as_bytes() {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(0x01000193);
    }
    hash
}

fn map_tool_choice(choice: &ToolChoice) -> Value {
    match choice {
        ToolChoice::Auto => json!({
            "functionCallingConfig": { "mode": "AUTO" }
        }),
        ToolChoice::None => json!({
            "functionCallingConfig": { "mode": "NONE" }
        }),
        ToolChoice::Required => json!({
            "functionCallingConfig": { "mode": "ANY" }
        }),
        ToolChoice::Function(name) => json!({
            "functionCallingConfig": {
                "mode": "ANY",
                "allowedFunctionNames": [name],
            }
        }),
    }
}

fn is_google_generate_content_extension(key: &str) -> bool {
    matches!(
        key,
        "cachedContent" | "labels" | "safetySettings" | "toolConfig"
    )
}

fn to_gemini_schema(schema: &Value) -> Value {
    match schema {
        Value::Object(object) => {
            let mut mapped = Map::new();
            for (key, value) in object {
                if key == "$schema" {
                    continue;
                }
                let mapped_value = if key == "type" {
                    value
                        .as_str()
                        .map(|kind| Value::String(kind.to_ascii_uppercase()))
                        .unwrap_or_else(|| to_gemini_schema(value))
                } else {
                    to_gemini_schema(value)
                };
                mapped.insert(key.clone(), mapped_value);
            }
            Value::Object(mapped)
        }
        Value::Array(values) => Value::Array(values.iter().map(to_gemini_schema).collect()),
        _ => schema.clone(),
    }
}

fn generate_content_path(model: &str) -> String {
    let model = model.trim().trim_start_matches('/');
    let model = model.strip_suffix(":generateContent").unwrap_or(model);
    if model.contains('/') {
        format!("/{model}:generateContent")
    } else {
        format!("/models/{model}:generateContent")
    }
}

fn map_finish_reason(reason: &str) -> String {
    match reason {
        "STOP" => "stop".to_string(),
        "MAX_TOKENS" => "length".to_string(),
        "MALFORMED_FUNCTION_CALL" => "tool_calls".to_string(),
        other => other.to_ascii_lowercase(),
    }
}

fn response_to_buffered_stream(response: LlmResponse) -> LlmEventStream {
    let mut events = Vec::new();
    events.push(Ok(StreamEvent::StreamStart {
        id: response.id.clone(),
        model: response.model.clone(),
    }));

    if let Some(choice) = response.choices.first() {
        if let Some(content) = &choice.message.content {
            if !content.is_empty() {
                events.push(Ok(StreamEvent::ContentDelta {
                    delta: content.clone(),
                }));
            }
        }
        if let Some(tool_calls) = &choice.message.tool_calls {
            for (index, call) in tool_calls.iter().enumerate() {
                events.push(Ok(StreamEvent::ToolCallStart {
                    index: index as u32,
                    id: call.id.clone(),
                    name: call.name.clone(),
                }));
                events.push(Ok(StreamEvent::ToolCallDelta {
                    index: index as u32,
                    arguments_delta: call.arguments.to_string(),
                }));
            }
        }
        events.push(Ok(StreamEvent::Done {
            finish_reason: choice.finish_reason.clone(),
            usage: response.usage.clone(),
        }));
    } else {
        events.push(Ok(StreamEvent::Done {
            finish_reason: None,
            usage: response.usage.clone(),
        }));
    }

    Box::pin(futures::stream::iter(events))
}

impl LlmProvider for GoogleGenerateContentClient {
    fn capabilities(&self) -> ClientCapabilities {
        ClientCapabilities {
            streaming: true,
            tool_calling: true,
            structured_output: true,
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ChatContentPart, ToolChoice, ToolSchema};

    #[test]
    fn generate_content_payload_maps_system_tools_and_tool_results() {
        let payload = GoogleGenerateContentClient::to_generate_content_payload(&LlmRequest {
            model: "gemini-2.5-flash".to_string(),
            messages: vec![
                ChatMessage {
                    role: "system".to_string(),
                    content: Some("Be direct".to_string()),
                    ..Default::default()
                },
                ChatMessage {
                    role: "user".to_string(),
                    content: Some("List repos".to_string()),
                    ..Default::default()
                },
                ChatMessage {
                    role: "assistant".to_string(),
                    tool_calls: Some(vec![ToolCallRequest {
                        id: "call_1".to_string(),
                        name: "list_repositories".to_string(),
                        arguments: json!({"owner": "promptfleet"}),
                    }]),
                    ..Default::default()
                },
                ChatMessage {
                    role: "tool".to_string(),
                    name: Some("list_repositories".to_string()),
                    content: Some(json!({"repositories": ["pf_swe_factory"]}).to_string()),
                    ..Default::default()
                },
            ],
            tools: Some(vec![ToolSchema {
                name: "list_repositories".to_string(),
                description: Some("List repositories".to_string()),
                parameters: json!({"type": "object", "properties": {}}),
                strict: None,
            }]),
            tool_choice: Some(ToolChoice::Auto),
            max_tokens: Some(64),
            ..Default::default()
        });

        assert_eq!(payload["systemInstruction"]["parts"][0]["text"], "Be direct");
        assert_eq!(payload["contents"][1]["role"], "model");
        assert_eq!(
            payload["contents"][1]["parts"][0]["functionCall"]["name"],
            "list_repositories"
        );
        assert!(
            payload["contents"][1]["parts"][0]["functionCall"]
                .get("id")
                .is_none()
        );
        assert_eq!(
            payload["contents"][2]["parts"][0]["functionResponse"]["response"]["repositories"][0],
            "pf_swe_factory"
        );
        assert_eq!(
            payload["tools"][0]["functionDeclarations"][0]["name"],
            "list_repositories"
        );
        assert_eq!(
            payload["tools"][0]["functionDeclarations"][0]["parameters"]["type"],
            "OBJECT"
        );
        assert_eq!(payload["toolConfig"]["functionCallingConfig"]["mode"], "AUTO");
        assert_eq!(payload["generationConfig"]["maxOutputTokens"], 64);
    }

    #[test]
    fn generate_content_payload_maps_multimodal_content_parts() {
        let request = LlmRequest {
            model: "gemini-2.5-flash".to_string(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content_parts: Some(vec![
                    ChatContentPart::text("what is visible?"),
                    ChatContentPart::image_base64("image/png", "aW1hZ2U=", None),
                    ChatContentPart::file_base64(
                        "context.pdf",
                        "application/pdf",
                        "JVBERi0xLjQ=",
                        None,
                    ),
                ]),
                ..Default::default()
            }],
            ..Default::default()
        };
        GoogleGenerateContentClient::validate_multimodal_request(&request).unwrap();
        let payload = GoogleGenerateContentClient::to_generate_content_payload(&request);

        let parts = payload["contents"][0]["parts"].as_array().unwrap();
        assert_eq!(parts[0]["text"], "what is visible?");
        assert_eq!(parts[1]["inlineData"]["mimeType"], "image/png");
        assert_eq!(parts[1]["inlineData"]["data"], "aW1hZ2U=");
        assert_eq!(parts[2]["inlineData"]["mimeType"], "application/pdf");
        assert_eq!(parts[2]["inlineData"]["data"], "JVBERi0xLjQ=");
    }

    #[test]
    fn test_validate_multimodal_request_rejects_unknown_image_detail() {
        let request = LlmRequest {
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content_parts: Some(vec![ChatContentPart::image_base64(
                    "image/png",
                    "aW1hZ2U=",
                    Some("full".to_string()),
                )]),
                ..Default::default()
            }],
            ..Default::default()
        };

        let error = GoogleGenerateContentClient::validate_multimodal_request(&request)
            .expect_err("unknown image detail must fail before serialization");
        assert!(error.to_string().contains("auto, low, or high"));
    }

    #[test]
    fn generate_content_payload_drops_openai_only_extensions() {
        let payload = GoogleGenerateContentClient::to_generate_content_payload(&LlmRequest {
            model: "gemini-2.5-flash".to_string(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: Some("hello".to_string()),
                ..Default::default()
            }],
            extensions: Some(
                [
                    ("parallel_tool_calls".to_string(), json!(true)),
                    ("safetySettings".to_string(), json!([])),
                ]
                .into_iter()
                .collect(),
            ),
            ..Default::default()
        });

        assert!(payload.get("parallel_tool_calls").is_none());
        assert!(payload.get("safetySettings").is_some());
    }

    #[test]
    fn generate_content_payload_sanitizes_tool_names_and_response_restores_original() {
        let req = LlmRequest {
            model: "gemini-2.5-flash".to_string(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: Some("List repos".to_string()),
                ..Default::default()
            }],
            tools: Some(vec![ToolSchema {
                name: "github-readonly-smoke__github_list_repositories".to_string(),
                description: None,
                parameters: json!({"type": "object"}),
                strict: None,
            }]),
            ..Default::default()
        };
        let name_map = ToolNameMap::from_request(&req);
        let payload = GoogleGenerateContentClient::to_generate_content_payload_with_names(
            &req,
            &name_map,
        );
        let wire_name = payload["tools"][0]["functionDeclarations"][0]["name"]
            .as_str()
            .expect("wire name");

        assert!(!wire_name.contains('-'));
        assert_ne!(wire_name, "github-readonly-smoke__github_list_repositories");

        let response = GoogleGenerateContentClient::normalize_generate_content_json_with_names(
            json!({
                "candidates": [{
                    "content": {
                        "parts": [{
                            "functionCall": {
                                "name": wire_name,
                                "args": {}
                            }
                        }]
                    }
                }]
            }),
            None,
            &name_map,
        )
        .expect("normalized");

        assert_eq!(
            response.choices[0].message.tool_calls.as_ref().unwrap()[0].name,
            "github-readonly-smoke__github_list_repositories"
        );
    }

    #[test]
    fn generate_content_response_normalizes_text_and_function_calls() {
        let response = GoogleGenerateContentClient::normalize_generate_content_json(
            json!({
                "responseId": "resp-1",
                "modelVersion": "gemini-2.5-flash",
                "candidates": [{
                    "finishReason": "MALFORMED_FUNCTION_CALL",
                    "content": {
                        "role": "model",
                        "parts": [
                            { "text": "Checking." },
                            { "functionCall": {
                                "name": "list_repositories",
                                "args": { "owner": "promptfleet" }
                            }}
                        ]
                    }
                }],
                "usageMetadata": {
                    "promptTokenCount": 10,
                    "candidatesTokenCount": 4,
                    "totalTokenCount": 14
                }
            }),
            None,
        )
        .expect("normalized");

        assert_eq!(response.id.as_deref(), Some("resp-1"));
        assert_eq!(response.choices[0].message.content.as_deref(), Some("Checking."));
        assert_eq!(
            response.choices[0].message.tool_calls.as_ref().unwrap()[0].name,
            "list_repositories"
        );
        assert_eq!(response.usage.as_ref().unwrap().total_tokens, Some(14));
    }

    #[test]
    fn generate_content_path_uses_full_vertex_resource_when_supplied() {
        assert_eq!(
            generate_content_path("publishers/google/models/gemini-2.5-flash"),
            "/publishers/google/models/gemini-2.5-flash:generateContent"
        );
        assert_eq!(
            generate_content_path("gemini-2.5-flash"),
            "/models/gemini-2.5-flash:generateContent"
        );
    }
}
