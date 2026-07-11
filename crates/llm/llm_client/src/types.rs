use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// How the model should choose tools (OpenAI / Anthropic wire formats mapped internally).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolChoice {
    Auto,
    None,
    Required,
    Function(String),
}

impl ToolChoice {
    /// OpenAI Chat Completions / Responses `tool_choice` JSON.
    pub fn to_openai_value(&self) -> serde_json::Value {
        match self {
            ToolChoice::Auto => serde_json::Value::String("auto".to_string()),
            ToolChoice::None => serde_json::Value::String("none".to_string()),
            ToolChoice::Required => serde_json::Value::String("required".to_string()),
            ToolChoice::Function(name) => serde_json::json!({
                "type": "function",
                "function": { "name": name }
            }),
        }
    }

    /// Anthropic Messages `tool_choice` JSON.
    pub fn to_anthropic_value(&self) -> serde_json::Value {
        match self {
            ToolChoice::Auto => serde_json::json!({ "type": "auto" }),
            ToolChoice::None => serde_json::json!({ "type": "none" }),
            ToolChoice::Required => serde_json::json!({ "type": "any" }),
            ToolChoice::Function(name) => serde_json::json!({
                "type": "tool",
                "name": name
            }),
        }
    }
}

impl Serialize for ToolChoice {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.to_openai_value().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ToolChoice {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let v = serde_json::Value::deserialize(deserializer)?;
        parse_tool_choice_value(&v).map_err(serde::de::Error::custom)
    }
}

fn parse_tool_choice_value(v: &serde_json::Value) -> Result<ToolChoice, String> {
    if let Some(s) = v.as_str() {
        return match s {
            "auto" => Ok(ToolChoice::Auto),
            "none" => Ok(ToolChoice::None),
            "required" => Ok(ToolChoice::Required),
            other => Err(format!("unknown tool_choice string: {other}")),
        };
    }
    if let Some(obj) = v.as_object() {
        if obj.get("type").and_then(|t| t.as_str()) == Some("function") {
            let name = obj
                .get("function")
                .and_then(|f| f.get("name"))
                .and_then(|n| n.as_str())
                .ok_or_else(|| "tool_choice function missing name".to_string())?;
            return Ok(ToolChoice::Function(name.to_string()));
        }
    }
    Err(format!("unsupported tool_choice value: {v}"))
}

/// Provider-neutral multimodal message content.
///
/// Text-only callers can keep using [`ChatMessage::content`]. Multimodal
/// callers should use [`ChatMessage::content_parts`] so each provider can map
/// text and image inputs to its own wire format.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChatContentPart {
    Text {
        text: String,
    },
    ImageUrl {
        url: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
    },
    ImageBase64 {
        media_type: String,
        data: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
    },
}

impl ChatContentPart {
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text { text: text.into() }
    }

    pub fn image_url(url: impl Into<String>, detail: Option<String>) -> Self {
        Self::ImageUrl {
            url: url.into(),
            detail,
        }
    }

    pub fn image_base64(
        media_type: impl Into<String>,
        data: impl Into<String>,
        detail: Option<String>,
    ) -> Self {
        Self::ImageBase64 {
            media_type: media_type.into(),
            data: data.into(),
            detail,
        }
    }

    pub fn is_empty_text(&self) -> bool {
        matches!(self, Self::Text { text } if text.trim().is_empty())
    }
}

/// Provider-neutral chat message supporting text, image inputs, tool calls, and tool results.
///
/// Each provider maps this internal representation to its own wire format:
/// - OpenAI Chat Completions: `tool_calls` in assistant messages, `role: "tool"` for results
/// - OpenAI Responses: `function_call` and `function_call_output` input items
/// - Anthropic: `tool_use` content blocks, `tool_result` content blocks
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_parts: Option<Vec<ChatContentPart>>,
    /// Tool calls requested by the assistant (present in assistant messages).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCallRequest>>,
    /// ID of the tool call this message is responding to (present in tool-result messages).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// Function name for tool-result messages.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// A tool call as requested by the LLM in an assistant message.
///
/// `arguments` is normalized to `serde_json::Value` internally — OpenAI sends
/// arguments as a JSON string, Anthropic sends `input` as a JSON object. Each
/// provider normalizes on ingest and serializes back to its wire format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallRequest {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSchema {
    pub name: String,
    pub description: Option<String>,
    pub parameters: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strict: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LlmRequest {
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub messages: Vec<ChatMessage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ToolSchema>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extensions: Option<serde_json::Map<String, serde_json::Value>>, // additional top-level provider keys
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: Option<u32>,
    pub completion_tokens: Option<u32>,
    pub total_tokens: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmChoice {
    pub index: u32,
    pub message: ChatMessage,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call_id: Option<String>,
    pub name: String,
    pub arguments: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmResponse {
    pub id: Option<String>,
    pub created: Option<u64>,
    pub model: Option<String>,
    pub choices: Vec<LlmChoice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
}
