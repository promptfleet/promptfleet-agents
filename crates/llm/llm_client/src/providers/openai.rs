use crate::{
    auth::AuthProvider,
    error::LlmError,
    model_client::{ApiMode, ClientCapabilities, HttpModelClient},
    multimodal::{FileDetailPolicy, validate_multimodal_inputs},
    provider::LlmProvider,
    types::{ChatContentPart, ChatMessage, LlmChoice, LlmRequest, LlmResponse, Usage},
};
use protocol_transport_core::StreamingPolicy;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone)]
pub(crate) struct OpenAIClient {
    inner: HttpModelClient,
    api_mode: Option<ApiMode>,
    chat_path: String,
    responses_path: String,
}

impl OpenAIClient {
    pub(crate) fn new(
        base_url: String,
        default_headers: HashMap<String, String>,
        streaming: Option<StreamingPolicy>,
        auth: Arc<dyn AuthProvider>,
        api_mode: Option<ApiMode>,
        chat_path: String,
        responses_path: String,
    ) -> Self {
        log::debug!("OpenAIClient::new api_mode={:?}", api_mode);
        Self {
            inner: HttpModelClient::new(base_url, default_headers, streaming, auth),
            api_mode,
            chat_path,
            responses_path,
        }
    }

    fn validate_multimodal_request(req: &LlmRequest, mode: ApiMode) -> Result<(), LlmError> {
        let file_detail_policy = if matches!(mode, ApiMode::Chat) {
            FileDetailPolicy::Unsupported
        } else {
            FileDetailPolicy::PdfOnly
        };
        validate_multimodal_inputs(req, file_detail_policy)
    }

    fn text_or_parts_content_chat(m: &ChatMessage) -> Option<serde_json::Value> {
        if let Some(parts) = &m.content_parts {
            let mapped: Vec<serde_json::Value> = parts
                .iter()
                .filter(|part| !part.is_empty_text())
                .map(|part| match part {
                    ChatContentPart::Text { text } => serde_json::json!({
                        "type": "text",
                        "text": text,
                    }),
                    ChatContentPart::ImageUrl { url, detail } => {
                        let mut image_url = serde_json::Map::new();
                        image_url.insert("url".to_string(), serde_json::Value::String(url.clone()));
                        if let Some(detail) = detail {
                            image_url.insert(
                                "detail".to_string(),
                                serde_json::Value::String(detail.clone()),
                            );
                        }
                        serde_json::json!({
                            "type": "image_url",
                            "image_url": serde_json::Value::Object(image_url),
                        })
                    }
                    ChatContentPart::ImageBase64 {
                        media_type,
                        data,
                        detail,
                    } => {
                        let mut image_url = serde_json::Map::new();
                        image_url.insert(
                            "url".to_string(),
                            serde_json::Value::String(format!("data:{media_type};base64,{data}")),
                        );
                        if let Some(detail) = detail {
                            image_url.insert(
                                "detail".to_string(),
                                serde_json::Value::String(detail.clone()),
                            );
                        }
                        serde_json::json!({
                            "type": "image_url",
                            "image_url": serde_json::Value::Object(image_url),
                        })
                    }
                    ChatContentPart::FileBase64 {
                        filename,
                        media_type,
                        data,
                        ..
                    } => serde_json::json!({
                        "type": "file",
                        "file": {
                            "filename": filename,
                            "file_data": format!("data:{media_type};base64,{data}"),
                        },
                    }),
                })
                .collect();
            return Some(serde_json::Value::Array(mapped));
        }
        m.content
            .as_ref()
            .map(|content| serde_json::Value::String(content.clone()))
    }

    fn text_or_parts_content_responses(m: &ChatMessage) -> Option<serde_json::Value> {
        if let Some(parts) = &m.content_parts {
            let mapped: Vec<serde_json::Value> = parts
                .iter()
                .filter(|part| !part.is_empty_text())
                .map(|part| match part {
                    ChatContentPart::Text { text } => serde_json::json!({
                        "type": "input_text",
                        "text": text,
                    }),
                    ChatContentPart::ImageUrl { url, detail } => {
                        let mut image = serde_json::Map::new();
                        image.insert(
                            "type".to_string(),
                            serde_json::Value::String("input_image".to_string()),
                        );
                        image.insert("image_url".to_string(), serde_json::Value::String(url.clone()));
                        if let Some(detail) = detail {
                            image.insert(
                                "detail".to_string(),
                                serde_json::Value::String(detail.clone()),
                            );
                        }
                        serde_json::Value::Object(image)
                    }
                    ChatContentPart::ImageBase64 {
                        media_type,
                        data,
                        detail,
                    } => {
                        let mut image = serde_json::Map::new();
                        image.insert(
                            "type".to_string(),
                            serde_json::Value::String("input_image".to_string()),
                        );
                        image.insert(
                            "image_url".to_string(),
                            serde_json::Value::String(format!("data:{media_type};base64,{data}")),
                        );
                        if let Some(detail) = detail {
                            image.insert(
                                "detail".to_string(),
                                serde_json::Value::String(detail.clone()),
                            );
                        }
                        serde_json::Value::Object(image)
                    }
                    ChatContentPart::FileBase64 {
                        filename,
                        media_type,
                        data,
                        detail,
                    } => {
                        let mut file = serde_json::Map::new();
                        file.insert(
                            "type".to_string(),
                            serde_json::Value::String("input_file".to_string()),
                        );
                        file.insert(
                            "filename".to_string(),
                            serde_json::Value::String(filename.clone()),
                        );
                        file.insert(
                            "file_data".to_string(),
                            serde_json::Value::String(format!(
                                "data:{media_type};base64,{data}"
                            )),
                        );
                        if let Some(detail) = detail {
                            file.insert(
                                "detail".to_string(),
                                serde_json::Value::String(detail.clone()),
                            );
                        }
                        serde_json::Value::Object(file)
                    }
                })
                .collect();
            return Some(serde_json::Value::Array(mapped));
        }
        m.content
            .as_ref()
            .map(|content| serde_json::Value::String(content.clone()))
    }

    fn map_openai_chat_message(m: &ChatMessage) -> serde_json::Value {
        if m.role == "tool" {
            return serde_json::json!({
                "role": "tool",
                "tool_call_id": m.tool_call_id,
                "content": m.content,
            });
        }
        if m.role == "assistant"
            && m.tool_calls
                .as_ref()
                .map(|t| !t.is_empty())
                .unwrap_or(false)
        {
            let tool_calls: Vec<serde_json::Value> = m
                .tool_calls
                .as_ref()
                .unwrap()
                .iter()
                .map(|tc| {
                    let args_str = match &tc.arguments {
                        serde_json::Value::String(s) => s.clone(),
                        other => other.to_string(),
                    };
                    serde_json::json!({
                        "id": tc.id,
                        "type": "function",
                        "function": {
                            "name": tc.name,
                            "arguments": args_str
                        }
                    })
                })
                .collect();
            let mut obj = serde_json::Map::new();
            obj.insert(
                "role".to_string(),
                serde_json::Value::String("assistant".into()),
            );
            if let Some(content) = Self::text_or_parts_content_chat(m) {
                if !content.as_str().is_some_and(str::is_empty) {
                    obj.insert("content".to_string(), content);
                }
            }
            obj.insert(
                "tool_calls".to_string(),
                serde_json::Value::Array(tool_calls),
            );
            return serde_json::Value::Object(obj);
        }
        let mut obj = serde_json::Map::new();
        obj.insert(
            "role".to_string(),
            serde_json::Value::String(m.role.clone()),
        );
        if let Some(content) = Self::text_or_parts_content_chat(m) {
            obj.insert("content".to_string(), content);
        }
        serde_json::Value::Object(obj)
    }

    fn map_messages_openai_chat(messages: &[ChatMessage]) -> Vec<serde_json::Value> {
        messages.iter().map(Self::map_openai_chat_message).collect()
    }

    fn map_openai_responses_message(m: &ChatMessage) -> Vec<serde_json::Value> {
        if m.role == "tool" {
            return vec![serde_json::json!({
                "type": "function_call_output",
                "call_id": m.tool_call_id.as_deref().unwrap_or_default(),
                "output": m.content.as_deref().unwrap_or_default(),
            })];
        }

        let tool_calls = if m.role == "assistant" {
            m.tool_calls.as_deref().unwrap_or_default()
        } else {
            &[]
        };
        let content = Self::text_or_parts_content_responses(m);
        let has_content = content.as_ref().is_some_and(|value| match value {
            serde_json::Value::String(text) => !text.is_empty(),
            serde_json::Value::Array(parts) => !parts.is_empty(),
            _ => true,
        });
        let mut items = Vec::with_capacity(tool_calls.len() + usize::from(has_content));

        if has_content || tool_calls.is_empty() {
            items.push(serde_json::json!({
                "role": m.role,
                "content": content.unwrap_or_else(|| serde_json::Value::String(String::new())),
            }));
        }

        items.extend(tool_calls.iter().map(|tool_call| {
            let arguments = match &tool_call.arguments {
                serde_json::Value::String(arguments) => arguments.clone(),
                other => other.to_string(),
            };
            serde_json::json!({
                "type": "function_call",
                "call_id": tool_call.id,
                "name": tool_call.name,
                "arguments": arguments,
            })
        }));
        items
    }

    fn map_messages_openai_responses(messages: &[ChatMessage]) -> Vec<serde_json::Value> {
        messages
            .iter()
            .flat_map(Self::map_openai_responses_message)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::ApiKeyAuth;
    use crate::types::{ChatContentPart, LlmRequest, ToolSchema};

    fn mk_client(api_mode: ApiMode) -> OpenAIClient {
        OpenAIClient::new(
            "http://localhost:1234".to_string(),
            Default::default(),
            None,
            Arc::new(ApiKeyAuth::new("")),
            Some(api_mode),
            "/v1/chat/completions".to_string(),
            "/v1/responses".to_string(),
        )
    }

    #[test]
    fn test_decide_mode_auto_routes_gpt5_to_responses() {
        let client = mk_client(ApiMode::Auto);
        assert!(matches!(
            client.decide_mode("gpt-5.4-mini"),
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
                    content: Some("be concise".to_string()),
                    ..Default::default()
                },
                ChatMessage {
                    role: "user".to_string(),
                    content: Some("hello".to_string()),
                    ..Default::default()
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

    #[test]
    fn test_to_chat_payload_basic() {
        let request = LlmRequest {
            model: "gpt-4o".to_string(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: Some("hello".to_string()),
                ..Default::default()
            }],
            temperature: Some(0.7),
            max_tokens: Some(100),
            ..Default::default()
        };
        let payload = OpenAIClient::to_chat_payload(&request);
        assert_eq!(payload["model"], "gpt-4o");
        assert_eq!(payload["messages"][0]["role"], "user");
        assert_eq!(payload["messages"][0]["content"], "hello");
        let temp = payload["temperature"].as_f64().expect("temperature");
        assert!((temp - 0.7_f64).abs() < 1e-5);
        assert_eq!(payload["max_tokens"], 100);
    }

    #[test]
    fn test_to_chat_payload_maps_multimodal_content_parts() {
        let request = LlmRequest {
            model: "gpt-4.1-mini".to_string(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content_parts: Some(vec![
                    ChatContentPart::text("what is visible?"),
                    ChatContentPart::image_base64(
                        "image/png",
                        "aW1hZ2U=",
                        Some("low".to_string()),
                    ),
                ]),
                ..Default::default()
            }],
            ..Default::default()
        };
        OpenAIClient::validate_multimodal_request(&request, ApiMode::Chat).unwrap();
        let payload = OpenAIClient::to_chat_payload(&request);
        let content = payload["messages"][0]["content"].as_array().unwrap();

        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[1]["type"], "image_url");
        assert_eq!(
            content[1]["image_url"]["url"],
            "data:image/png;base64,aW1hZ2U="
        );
        assert_eq!(content[1]["image_url"]["detail"], "low");
    }

    #[test]
    fn test_to_responses_payload_maps_multimodal_content_parts() {
        let request = LlmRequest {
            model: "gpt-5-mini".to_string(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content_parts: Some(vec![
                    ChatContentPart::text("what is visible?"),
                    ChatContentPart::image_url("https://example.test/screen.png", None),
                ]),
                ..Default::default()
            }],
            ..Default::default()
        };
        OpenAIClient::validate_multimodal_request(&request, ApiMode::Responses).unwrap();
        let payload = OpenAIClient::to_responses_payload(&request);
        let content = payload["input"][0]["content"].as_array().unwrap();

        assert_eq!(content[0]["type"], "input_text");
        assert_eq!(content[1]["type"], "input_image");
        assert_eq!(content[1]["image_url"], "https://example.test/screen.png");
    }

    #[test]
    fn test_to_responses_payload_maps_base64_image_input() {
        let request = LlmRequest {
            model: "gpt-5.4-mini".to_string(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content_parts: Some(vec![ChatContentPart::image_base64(
                    "image/png",
                    "aW1hZ2U=",
                    Some("high".to_string()),
                )]),
                ..Default::default()
            }],
            ..Default::default()
        };
        OpenAIClient::validate_multimodal_request(&request, ApiMode::Responses).unwrap();
        let payload = OpenAIClient::to_responses_payload(&request);
        let image = &payload["input"][0]["content"][0];

        assert_eq!(image["type"], "input_image");
        assert_eq!(image["image_url"], "data:image/png;base64,aW1hZ2U=");
        assert_eq!(image["detail"], "high");
    }

    #[test]
    fn test_to_responses_payload_maps_pdf_with_page_detail() {
        let request = LlmRequest {
            model: "gpt-5.4-mini".to_string(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content_parts: Some(vec![ChatContentPart::file_base64(
                    "incident.pdf",
                    "application/pdf",
                    "JVBERi0xLjQ=",
                    Some("high".to_string()),
                )]),
                ..Default::default()
            }],
            ..Default::default()
        };
        OpenAIClient::validate_multimodal_request(&request, ApiMode::Responses).unwrap();
        let payload = OpenAIClient::to_responses_payload(&request);
        let file = &payload["input"][0]["content"][0];

        assert_eq!(file["type"], "input_file");
        assert_eq!(file["filename"], "incident.pdf");
        assert_eq!(
            file["file_data"],
            "data:application/pdf;base64,JVBERi0xLjQ="
        );
        assert_eq!(file["detail"], "high");
    }

    #[test]
    fn test_to_responses_payload_maps_docx_without_pdf_detail() {
        let request = LlmRequest {
            model: "gpt-5.4-mini".to_string(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content_parts: Some(vec![ChatContentPart::file_base64(
                    "notes.docx",
                    "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
                    "UEsDBA==",
                    None,
                )]),
                ..Default::default()
            }],
            ..Default::default()
        };
        OpenAIClient::validate_multimodal_request(&request, ApiMode::Responses).unwrap();
        let payload = OpenAIClient::to_responses_payload(&request);
        let file = &payload["input"][0]["content"][0];

        assert_eq!(file["type"], "input_file");
        assert_eq!(file["filename"], "notes.docx");
        assert_eq!(
            file["file_data"],
            "data:application/vnd.openxmlformats-officedocument.wordprocessingml.document;base64,UEsDBA=="
        );
        assert!(file.get("detail").is_none());
    }

    #[test]
    fn test_validate_multimodal_request_rejects_invalid_base64_before_request() {
        let request = LlmRequest {
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content_parts: Some(vec![ChatContentPart::file_base64(
                    "notes.txt",
                    "text/plain",
                    "not base64",
                    None,
                )]),
                ..Default::default()
            }],
            ..Default::default()
        };

        let error = OpenAIClient::validate_multimodal_request(&request, ApiMode::Responses)
            .expect_err("invalid Base64 must fail validation");
        assert!(error.to_string().contains("Base64"));
    }

    #[test]
    fn test_validate_multimodal_request_rejects_detail_for_non_pdf() {
        let request = LlmRequest {
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content_parts: Some(vec![ChatContentPart::file_base64(
                    "notes.docx",
                    "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
                    "UEsDBA==",
                    Some("high".to_string()),
                )]),
                ..Default::default()
            }],
            ..Default::default()
        };

        let error = OpenAIClient::validate_multimodal_request(&request, ApiMode::Responses)
            .expect_err("non-PDF detail must fail validation");
        assert!(error.to_string().contains("only for PDF"));
    }

    #[test]
    fn test_to_chat_payload_with_tools() {
        let request = LlmRequest {
            model: "gpt-4o".to_string(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: Some("hi".to_string()),
                ..Default::default()
            }],
            tools: Some(vec![ToolSchema {
                name: "get_weather".to_string(),
                description: Some("Get weather".to_string()),
                parameters: serde_json::json!({"type": "object", "properties": {}}),
                strict: None,
            }]),
            ..Default::default()
        };
        let payload = OpenAIClient::to_chat_payload(&request);
        let tools = payload["tools"].as_array().expect("tools array");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["type"], "function");
        assert_eq!(tools[0]["function"]["name"], "get_weather");
        assert_eq!(tools[0]["function"]["description"], "Get weather");
    }

    #[test]
    fn test_to_chat_payload_with_extensions() {
        let mut ext = serde_json::Map::new();
        ext.insert("metadata".to_string(), serde_json::json!({"k": "v"}));
        ext.insert("top_bool".to_string(), serde_json::json!(true));
        let request = LlmRequest {
            model: "m".to_string(),
            messages: vec![],
            extensions: Some(ext),
            ..Default::default()
        };
        let payload = OpenAIClient::to_chat_payload(&request);
        assert_eq!(payload["metadata"], serde_json::json!({"k": "v"}));
        assert_eq!(payload["top_bool"], true);
    }

    #[test]
    fn test_to_responses_payload_with_tools() {
        let request = LlmRequest {
            model: "gpt-5".to_string(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: Some("x".to_string()),
                ..Default::default()
            }],
            tools: Some(vec![ToolSchema {
                name: "fn1".to_string(),
                description: Some("d".to_string()),
                parameters: serde_json::json!({}),
                strict: Some(true),
            }]),
            ..Default::default()
        };
        let payload = OpenAIClient::to_responses_payload(&request);
        let tools = payload["tools"].as_array().expect("tools");
        assert_eq!(tools[0]["type"], "function");
        assert_eq!(tools[0]["name"], "fn1");
        assert_eq!(tools[0]["strict"], true);
    }

    #[test]
    fn test_to_responses_payload_maps_function_call_history_items() {
        let request = LlmRequest {
            model: "gpt-5.4-mini".to_string(),
            messages: vec![
                ChatMessage {
                    role: "user".to_string(),
                    content: Some("How many agents are running?".to_string()),
                    ..Default::default()
                },
                ChatMessage {
                    role: "assistant".to_string(),
                    tool_calls: Some(vec![crate::types::ToolCallRequest {
                        id: "call_agents".to_string(),
                        name: "list_agents".to_string(),
                        arguments: serde_json::json!({"status": "running"}),
                    }]),
                    ..Default::default()
                },
                ChatMessage {
                    role: "tool".to_string(),
                    content: Some("{\"total\":3}".to_string()),
                    tool_call_id: Some("call_agents".to_string()),
                    name: Some("list_agents".to_string()),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };

        let payload = OpenAIClient::to_responses_payload(&request);
        let input = payload["input"].as_array().expect("responses input");

        assert_eq!(input.len(), 3);
        assert_eq!(input[0]["role"], "user");
        assert_eq!(input[0]["content"], "How many agents are running?");
        assert_eq!(
            input[1],
            serde_json::json!({
                "type": "function_call",
                "call_id": "call_agents",
                "name": "list_agents",
                "arguments": "{\"status\":\"running\"}",
            })
        );
        assert_eq!(
            input[2],
            serde_json::json!({
                "type": "function_call_output",
                "call_id": "call_agents",
                "output": "{\"total\":3}",
            })
        );
        assert!(input[1].get("content").is_none());
        assert!(input[1].get("tool_calls").is_none());
        assert!(input[2].get("role").is_none());
    }

    #[test]
    fn test_to_responses_payload_max_tokens_to_max_output_tokens() {
        let request = LlmRequest {
            model: "gpt-5".to_string(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: Some("u".to_string()),
                ..Default::default()
            }],
            max_tokens: Some(512),
            ..Default::default()
        };
        let payload = OpenAIClient::to_responses_payload(&request);
        assert_eq!(payload["max_output_tokens"], 512);
        assert!(payload.get("max_tokens").is_none());
    }

    #[test]
    fn test_to_responses_payload_no_system() {
        let request = LlmRequest {
            model: "gpt-5".to_string(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: Some("only user".to_string()),
                ..Default::default()
            }],
            ..Default::default()
        };
        let payload = OpenAIClient::to_responses_payload(&request);
        assert!(payload.get("instructions").is_none());
        assert_eq!(payload["input"].as_array().map(|a| a.len()), Some(1));
    }

    #[test]
    fn test_split_no_system() {
        let msgs = vec![ChatMessage {
            role: "user".to_string(),
            content: Some("a".to_string()),
            ..Default::default()
        }];
        let (instr, rest) = OpenAIClient::split_instructions_and_messages(&msgs);
        assert!(instr.is_none());
        assert_eq!(rest.len(), 1);
        assert_eq!(rest[0].role, "user");
    }

    #[test]
    fn test_split_multiple_system() {
        let msgs = vec![
            ChatMessage {
                role: "system".to_string(),
                content: Some("first".to_string()),
                ..Default::default()
            },
            ChatMessage {
                role: "system".to_string(),
                content: Some("second".to_string()),
                ..Default::default()
            },
            ChatMessage {
                role: "user".to_string(),
                content: Some("u".to_string()),
                ..Default::default()
            },
        ];
        let (instr, rest) = OpenAIClient::split_instructions_and_messages(&msgs);
        assert_eq!(instr.as_deref(), Some("first\nsecond"));
        assert_eq!(rest.len(), 1);
    }

    #[test]
    fn test_extract_tool_calls_single() {
        let raw = serde_json::json!({
            "choices": [{
                "message": {
                    "tool_calls": [{
                        "id": "call_1",
                        "function": {"name": "foo", "arguments": "{\"a\":1}"}
                    }]
                }
            }]
        });
        let tcs = OpenAIClient::extract_tool_calls_from_chat(&raw).expect("tool calls");
        assert_eq!(tcs.len(), 1);
        assert_eq!(tcs[0].id.as_deref(), Some("call_1"));
        assert_eq!(tcs[0].name, "foo");
        assert_eq!(tcs[0].arguments, serde_json::json!({"a": 1}));
    }

    #[test]
    fn test_extract_tool_calls_multiple() {
        let raw = serde_json::json!({
            "choices": [{
                "message": {
                    "tool_calls": [
                        {"id": "1", "function": {"name": "a", "arguments": "{}"}},
                        {"id": "2", "function": {"name": "b", "arguments": "{\"x\":true}"}}
                    ]
                }
            }]
        });
        let tcs = OpenAIClient::extract_tool_calls_from_chat(&raw).expect("tool calls");
        assert_eq!(tcs.len(), 2);
        assert_eq!(tcs[0].name, "a");
        assert_eq!(tcs[1].name, "b");
    }

    #[test]
    fn test_extract_tool_calls_empty_name_filtered() {
        let raw = serde_json::json!({
            "choices": [{
                "message": {
                    "tool_calls": [
                        {"id": "1", "function": {"name": "", "arguments": "{}"}},
                        {"id": "2", "function": {"name": "ok", "arguments": "{}"}}
                    ]
                }
            }]
        });
        let tcs = OpenAIClient::extract_tool_calls_from_chat(&raw).expect("tool calls");
        assert_eq!(tcs.len(), 1);
        assert_eq!(tcs[0].name, "ok");
    }

    #[test]
    fn test_extract_tool_calls_malformed_arguments() {
        let raw = serde_json::json!({
            "choices": [{
                "message": {
                    "tool_calls": [{
                        "id": "x",
                        "function": {"name": "f", "arguments": "not-json"}
                    }]
                }
            }]
        });
        let tcs = OpenAIClient::extract_tool_calls_from_chat(&raw).expect("tool calls");
        assert_eq!(tcs[0].arguments, serde_json::json!({"_raw": "not-json"}));
    }

    #[test]
    fn test_normalize_chat_json_normal() {
        let raw = serde_json::json!({
            "id": "chatcmpl-1",
            "model": "gpt-4",
            "choices": [{
                "message": {"role": "assistant", "content": "Hello!"}
            }]
        });
        let resp = OpenAIClient::normalize_chat_json(raw).expect("ok");
        assert_eq!(resp.choices[0].message.content.as_deref(), Some("Hello!"));
        assert_eq!(resp.choices[0].message.role, "assistant");
        assert_eq!(resp.id.as_deref(), Some("chatcmpl-1"));
    }

    #[test]
    fn test_normalize_chat_json_null_content_with_tool_calls() {
        let raw = serde_json::json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "tc_1",
                        "function": {"name": "w", "arguments": "{}"}
                    }]
                }
            }]
        });
        let resp = OpenAIClient::normalize_chat_json(raw).expect("ok");
        assert!(resp.choices[0].message.content.is_none());
        let reqs = resp.choices[0].message.tool_calls.as_ref().expect("reqs");
        assert_eq!(reqs.len(), 1);
        assert_eq!(reqs[0].id, "tc_1");
        assert_eq!(reqs[0].name, "w");
    }

    #[test]
    fn test_normalize_chat_json_missing_fields() {
        let raw = serde_json::json!({});
        let resp = OpenAIClient::normalize_chat_json(raw).expect("ok");
        assert_eq!(resp.choices[0].message.role, "assistant");
        assert!(resp.choices[0].message.content.is_none());
        assert!(resp.choices[0].message.tool_calls.is_none());
    }

    #[test]
    fn test_normalize_responses_json_output_text() {
        let raw = serde_json::json!({
            "id": "resp-1",
            "output_text": "shortcut text",
            "model": "gpt-5"
        });
        let resp = OpenAIClient::normalize_responses_json(raw).expect("ok");
        assert_eq!(
            resp.choices[0].message.content.as_deref(),
            Some("shortcut text")
        );
    }

    #[test]
    fn test_normalize_responses_json_output_items() {
        let raw = serde_json::json!({
            "output": [
                {"type": "output_text", "text": "part1"},
                {"type": "output_text", "text": "part2"}
            ]
        });
        let resp = OpenAIClient::normalize_responses_json(raw).expect("ok");
        assert_eq!(
            resp.choices[0].message.content.as_deref(),
            Some("part1part2")
        );
    }

    #[test]
    fn test_normalize_responses_json_with_tool_calls() {
        let raw = serde_json::json!({
            "output": [
                {
                    "type": "function_call",
                    "name": "todo",
                    "arguments": "{\"q\":\"x\"}",
                    "call_id": "fc_1",
                    "id": "item-1"
                }
            ]
        });
        let resp = OpenAIClient::normalize_responses_json(raw).expect("ok");
        let tcs = resp.tool_calls.as_ref().expect("tool_calls");
        assert_eq!(tcs.len(), 1);
        assert_eq!(tcs[0].name, "todo");
        assert_eq!(tcs[0].arguments, serde_json::json!({"q": "x"}));
        let reqs = resp.choices[0].message.tool_calls.as_ref().expect("reqs");
        assert_eq!(reqs[0].name, "todo");
        assert_eq!(reqs[0].id, "fc_1");
    }

    #[test]
    fn test_decide_mode_explicit_chat() {
        let client = mk_client(ApiMode::Chat);
        assert!(matches!(client.decide_mode("gpt-5-mini"), ApiMode::Chat));
        assert!(matches!(client.decide_mode("gpt-4o"), ApiMode::Chat));
    }

    #[test]
    fn test_decide_mode_explicit_responses() {
        let client = mk_client(ApiMode::Responses);
        assert!(matches!(client.decide_mode("gpt-5"), ApiMode::Responses));
        assert!(matches!(
            client.decide_mode("gpt-4o-mini"),
            ApiMode::Responses
        ));
    }
}

impl OpenAIClient {
    fn decide_mode(&self, model: &str) -> ApiMode {
        let decided = match self.api_mode.unwrap_or(ApiMode::Chat) {
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
            serde_json::Value::Array(Self::map_messages_openai_chat(&req.messages)),
        );
        if let Some(tools) = Self::map_tools_for_chat(&req.tools) {
            obj.insert("tools".to_string(), tools);
        }
        if let Some(choice) = &req.tool_choice {
            obj.insert("tool_choice".to_string(), choice.to_openai_value());
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
                instructions_segments.push(m.content.clone().unwrap_or_default());
            } else {
                rest.push(m.clone());
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
            serde_json::Value::Array(Self::map_messages_openai_responses(&inputs)),
        );
        if let Some(t) = tools {
            obj.insert("tools".to_string(), t);
        }
        if let Some(choice) = &req.tool_choice {
            obj.insert("tool_choice".to_string(), choice.to_openai_value());
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

    fn extract_tool_call_requests_from_chat(
        raw: &serde_json::Value,
    ) -> Option<Vec<crate::types::ToolCallRequest>> {
        let mut requests = Vec::new();
        if let Some(choices) = raw.get("choices").and_then(|v| v.as_array()) {
            for ch in choices {
                if let Some(msg) = ch.get("message") {
                    if let Some(tc_arr) = msg.get("tool_calls").and_then(|v| v.as_array()) {
                        for tc in tc_arr {
                            let id = tc
                                .get("id")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
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
                            if !name.is_empty() && !id.is_empty() {
                                requests.push(crate::types::ToolCallRequest {
                                    id,
                                    name,
                                    arguments,
                                });
                            }
                        }
                    }
                }
            }
        }
        if requests.is_empty() {
            None
        } else {
            Some(requests)
        }
    }

    fn extract_tool_call_requests_from_responses(
        raw: &serde_json::Value,
    ) -> Option<Vec<crate::types::ToolCallRequest>> {
        let mut requests = Vec::new();
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
                    let id = item
                        .get("call_id")
                        .or_else(|| item.get("id"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    if !name.is_empty() && !id.is_empty() {
                        requests.push(crate::types::ToolCallRequest {
                            id,
                            name,
                            arguments,
                        });
                    }
                }
            }
        }
        if requests.is_empty() {
            None
        } else {
            Some(requests)
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

    fn normalize_responses_json(raw: serde_json::Value) -> Result<LlmResponse, LlmError> {
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
                if buf.is_empty() { None } else { Some(buf) }
            });
        let content = output_text;
        log::debug!(
            "OpenAIClient::normalize_responses_json content_len={}",
            content.as_ref().map(|s| s.len()).unwrap_or(0)
        );
        let tool_calls = Self::extract_tool_calls_from_responses(&raw);
        let tool_call_requests = Self::extract_tool_call_requests_from_responses(&raw);
        let choice = LlmChoice {
            index: 0,
            message: ChatMessage {
                role: "assistant".to_string(),
                content,
                tool_calls: tool_call_requests,
                ..Default::default()
            },
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
        Ok(LlmResponse {
            id,
            created: None,
            model,
            choices: vec![choice],
            usage,
            tool_calls,
        })
    }

    fn normalize_chat_json(raw: serde_json::Value) -> Result<LlmResponse, LlmError> {
        let content = raw
            .get("choices")
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.get(0))
            .and_then(|ch| ch.get("message"))
            .and_then(|msg| msg.get("content"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let role = raw
            .get("choices")
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.get(0))
            .and_then(|ch| ch.get("message"))
            .and_then(|msg| msg.get("role"))
            .and_then(|v| v.as_str())
            .unwrap_or("assistant")
            .to_string();
        let tool_calls = Self::extract_tool_calls_from_chat(&raw);
        let tool_call_requests = Self::extract_tool_call_requests_from_chat(&raw);
        let choice = LlmChoice {
            index: 0,
            message: ChatMessage {
                role,
                content,
                tool_calls: tool_call_requests,
                ..Default::default()
            },
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
    /// use llm_client::auth::ApiKeyAuth;
    /// use llm_client::client::{LlmClient, WireFormat};
    /// use llm_client::{LlmRequest, ChatMessage, StreamEvent};
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = LlmClient::builder(WireFormat::OpenAiCompat)
    ///     .base_url("https://api.openai.com")
    ///     .auth(ApiKeyAuth::new("sk-..."))
    ///     .build()?;
    ///
    /// let req = LlmRequest {
    ///     model: "gpt-4".into(),
    ///     messages: vec![ChatMessage { role: "user".into(), content: Some("Hello".into()), ..Default::default() }],
    ///     ..Default::default()
    /// };
    ///
    /// let mut stream = client.chat_stream(req).await?;
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
    ) -> Result<crate::stream::LlmEventStream, LlmError> {
        let mode = self.decide_mode(&req.model);
        Self::validate_multimodal_request(&req, mode)?;
        let (path, mut payload) = match mode {
            ApiMode::Chat => (self.chat_path.as_str(), Self::to_chat_payload(&req)),
            ApiMode::Responses | ApiMode::Auto => (
                self.responses_path.as_str(),
                Self::to_responses_payload(&req),
            ),
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
        let wire_format = match mode {
            ApiMode::Chat => crate::stream::StreamWireFormat::ChatCompletions,
            ApiMode::Responses | ApiMode::Auto => crate::stream::StreamWireFormat::Responses,
        };

        Ok(crate::stream::sse_event_stream_with_format(
            response,
            wire_format,
        ))
    }
}

#[cfg(target_arch = "wasm32")]
impl OpenAIClient {
    /// Buffer-then-parse SSE streaming for WASM targets.
    ///
    /// Uses [`HttpModelClient::post_sse_buffered`] to get the complete response,
    /// then parses all SSE events at once. No incremental token delivery.
    pub async fn llm_stream(
        &self,
        req: LlmRequest,
    ) -> Result<crate::stream::LlmEventStream, LlmError> {
        let mode = self.decide_mode(&req.model);
        Self::validate_multimodal_request(&req, mode)?;
        let (path, mut payload) = match mode {
            ApiMode::Chat => (self.chat_path.as_str(), Self::to_chat_payload(&req)),
            ApiMode::Responses | ApiMode::Auto => (
                self.responses_path.as_str(),
                Self::to_responses_payload(&req),
            ),
        };

        if let Some(obj) = payload.as_object_mut() {
            obj.insert("stream".to_string(), serde_json::Value::Bool(true));
            obj.insert(
                "stream_options".to_string(),
                serde_json::json!({ "include_usage": true }),
            );
        }

        log::info!(
            "OpenAIClient::llm_stream (wasm) mode={:?} endpoint={}",
            mode,
            path
        );

        let body = self.inner.post_sse_buffered(path, payload).await?;
        let wire_format = match mode {
            ApiMode::Chat => crate::stream::StreamWireFormat::ChatCompletions,
            ApiMode::Responses | ApiMode::Auto => crate::stream::StreamWireFormat::Responses,
        };
        Ok(crate::stream::sse_event_stream_from_buffer_with_format(
            body,
            wire_format,
        ))
    }
}

impl OpenAIClient {
    pub async fn llm(&self, req: LlmRequest) -> Result<LlmResponse, LlmError> {
        let mode = self.decide_mode(&req.model);
        Self::validate_multimodal_request(&req, mode)?;
        match mode {
            ApiMode::Chat => {
                let path = self.chat_path.clone();
                let value = Self::to_chat_payload(&req);
                log::info!("OpenAIClient::llm mode=chat endpoint={}", path);
                let raw = self.inner.post_json(&path, value).await?;
                // Normalize chat response to handle null content when tool_calls are present
                let mut parsed = Self::normalize_chat_json(raw.clone())?;
                // Enrich with tool calls if present (already included by normalize, but keep to ensure)
                parsed.tool_calls = Self::extract_tool_calls_from_chat(&raw);
                Ok(parsed)
            }
            ApiMode::Responses | ApiMode::Auto => {
                let path = self.responses_path.clone();
                let value = Self::to_responses_payload(&req);
                log::info!("OpenAIClient::llm mode=responses endpoint={}", path);
                let raw = self.inner.post_json(&path, value).await?;
                let normalized = Self::normalize_responses_json(raw)?;
                Ok(normalized)
            }
        }
    }
}

impl LlmProvider for OpenAIClient {
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
