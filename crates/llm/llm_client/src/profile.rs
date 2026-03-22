use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ModelFamily {
    OpenAI,
    Gpt5,
    Qwen3,
    Claude,
    Gemini,
    DeepSeek,
    Llama,
    Mistral,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ModelProfile {
    Generic,
    Gpt5 {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reasoning_effort: Option<String>, // minimal | medium | high
        #[serde(default, skip_serializing_if = "Option::is_none")]
        responses_text_verbosity: Option<String>, // low | medium | high (Responses API: text.verbosity)
        #[serde(default, skip_serializing_if = "Option::is_none")]
        responses_reasoning_object: Option<bool>, // if true, use { reasoning: { effort } } for Responses API
    },
    Qwen3 {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        enable_thinking: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tool_call_parser: Option<String>, // e.g. "hermes"
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reasoning_parser: Option<String>, // e.g. "deepseek_r1"
        #[serde(default, skip_serializing_if = "Option::is_none")]
        auto_tool_choice: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        template_kwargs: Option<serde_json::Value>,
    },
}

/// Capability metadata for a specific model — context window, output limits,
/// feature support, and optional cost information.
///
/// Use [`ModelCapabilities::lookup`] to resolve capabilities from the built-in
/// registry, or construct manually for custom/self-hosted models.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelCapabilities {
    /// Maximum input tokens the model can accept (context window size).
    pub context_window: u32,
    /// Maximum tokens the model can generate in a single response.
    pub max_output_tokens: u32,
    /// Whether the model supports function/tool calling.
    pub supports_tools: bool,
    /// Whether the model supports vision (image) inputs.
    pub supports_vision: bool,
    /// Whether the model supports streaming responses.
    pub supports_streaming: bool,
    /// Cost per 1K input tokens (USD), if known. Used for budget tracking.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_per_1k_input: Option<f64>,
    /// Cost per 1K output tokens (USD), if known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_per_1k_output: Option<f64>,
}

impl ModelCapabilities {
    /// Safe fallback for unknown models — conservative 8K context window.
    pub const UNKNOWN_DEFAULT: Self = Self {
        context_window: 8_192,
        max_output_tokens: 4_096,
        supports_tools: true,
        supports_vision: false,
        supports_streaming: true,
        cost_per_1k_input: None,
        cost_per_1k_output: None,
    };

    /// Look up capabilities for a model by its ID string.
    ///
    /// Matches known model prefixes (e.g. "gpt-4o" matches "gpt-4o-2024-08-06").
    /// Returns [`UNKNOWN_DEFAULT`](Self::UNKNOWN_DEFAULT) if no match is found.
    pub fn lookup(model_id: &str) -> Self {
        let id = model_id.to_lowercase();
        for (prefix, caps) in KNOWN_MODELS {
            if id.starts_with(prefix) {
                return caps.clone();
            }
        }
        Self::UNKNOWN_DEFAULT
    }

    /// Compute available budget for conversation history after reserving
    /// space for output, system prompt, and tool schemas.
    pub fn available_for_history(
        &self,
        reserved_output: Option<u32>,
        system_tokens: u32,
        tools_tokens: u32,
    ) -> u32 {
        let output_reserve = reserved_output.unwrap_or(self.max_output_tokens);
        self.context_window
            .saturating_sub(output_reserve)
            .saturating_sub(system_tokens)
            .saturating_sub(tools_tokens)
    }
}

/// Static registry of known model capabilities.
///
/// Sorted by specificity (longer prefixes first) so "gpt-4o-mini" matches
/// before "gpt-4o". Updated periodically — use [`ModelConfig::capabilities`]
/// override for custom deployments.
const KNOWN_MODELS: &[(&str, ModelCapabilities)] = &[
    // OpenAI
    ("gpt-4o-mini", ModelCapabilities {
        context_window: 128_000, max_output_tokens: 16_384,
        supports_tools: true, supports_vision: true, supports_streaming: true,
        cost_per_1k_input: Some(0.00015), cost_per_1k_output: Some(0.0006),
    }),
    ("gpt-4o", ModelCapabilities {
        context_window: 128_000, max_output_tokens: 16_384,
        supports_tools: true, supports_vision: true, supports_streaming: true,
        cost_per_1k_input: Some(0.0025), cost_per_1k_output: Some(0.01),
    }),
    ("gpt-4-turbo", ModelCapabilities {
        context_window: 128_000, max_output_tokens: 4_096,
        supports_tools: true, supports_vision: true, supports_streaming: true,
        cost_per_1k_input: Some(0.01), cost_per_1k_output: Some(0.03),
    }),
    ("gpt-4", ModelCapabilities {
        context_window: 8_192, max_output_tokens: 4_096,
        supports_tools: true, supports_vision: false, supports_streaming: true,
        cost_per_1k_input: Some(0.03), cost_per_1k_output: Some(0.06),
    }),
    ("gpt-5", ModelCapabilities {
        context_window: 256_000, max_output_tokens: 32_768,
        supports_tools: true, supports_vision: true, supports_streaming: true,
        cost_per_1k_input: Some(0.005), cost_per_1k_output: Some(0.02),
    }),
    ("o3-mini", ModelCapabilities {
        context_window: 200_000, max_output_tokens: 100_000,
        supports_tools: true, supports_vision: false, supports_streaming: true,
        cost_per_1k_input: Some(0.0011), cost_per_1k_output: Some(0.0044),
    }),
    ("o3", ModelCapabilities {
        context_window: 200_000, max_output_tokens: 100_000,
        supports_tools: true, supports_vision: true, supports_streaming: true,
        cost_per_1k_input: Some(0.01), cost_per_1k_output: Some(0.04),
    }),
    ("o1-mini", ModelCapabilities {
        context_window: 128_000, max_output_tokens: 65_536,
        supports_tools: false, supports_vision: false, supports_streaming: false,
        cost_per_1k_input: Some(0.003), cost_per_1k_output: Some(0.012),
    }),
    ("o1", ModelCapabilities {
        context_window: 200_000, max_output_tokens: 100_000,
        supports_tools: true, supports_vision: true, supports_streaming: true,
        cost_per_1k_input: Some(0.015), cost_per_1k_output: Some(0.06),
    }),
    // Anthropic Claude
    ("claude-4-opus", ModelCapabilities {
        context_window: 200_000, max_output_tokens: 32_768,
        supports_tools: true, supports_vision: true, supports_streaming: true,
        cost_per_1k_input: Some(0.015), cost_per_1k_output: Some(0.075),
    }),
    ("claude-4-sonnet", ModelCapabilities {
        context_window: 200_000, max_output_tokens: 64_000,
        supports_tools: true, supports_vision: true, supports_streaming: true,
        cost_per_1k_input: Some(0.003), cost_per_1k_output: Some(0.015),
    }),
    ("claude-3.5-sonnet", ModelCapabilities {
        context_window: 200_000, max_output_tokens: 8_192,
        supports_tools: true, supports_vision: true, supports_streaming: true,
        cost_per_1k_input: Some(0.003), cost_per_1k_output: Some(0.015),
    }),
    ("claude-3-haiku", ModelCapabilities {
        context_window: 200_000, max_output_tokens: 4_096,
        supports_tools: true, supports_vision: true, supports_streaming: true,
        cost_per_1k_input: Some(0.00025), cost_per_1k_output: Some(0.00125),
    }),
    // Google Gemini
    ("gemini-2.0-flash", ModelCapabilities {
        context_window: 1_048_576, max_output_tokens: 8_192,
        supports_tools: true, supports_vision: true, supports_streaming: true,
        cost_per_1k_input: Some(0.0001), cost_per_1k_output: Some(0.0004),
    }),
    ("gemini-2.5-pro", ModelCapabilities {
        context_window: 1_048_576, max_output_tokens: 65_536,
        supports_tools: true, supports_vision: true, supports_streaming: true,
        cost_per_1k_input: Some(0.00125), cost_per_1k_output: Some(0.01),
    }),
    ("gemini-1.5-pro", ModelCapabilities {
        context_window: 2_097_152, max_output_tokens: 8_192,
        supports_tools: true, supports_vision: true, supports_streaming: true,
        cost_per_1k_input: Some(0.00125), cost_per_1k_output: Some(0.005),
    }),
    // Qwen3
    ("qwen3-235b", ModelCapabilities {
        context_window: 131_072, max_output_tokens: 8_192,
        supports_tools: true, supports_vision: false, supports_streaming: true,
        cost_per_1k_input: None, cost_per_1k_output: None,
    }),
    ("qwen3-30b", ModelCapabilities {
        context_window: 131_072, max_output_tokens: 8_192,
        supports_tools: true, supports_vision: false, supports_streaming: true,
        cost_per_1k_input: None, cost_per_1k_output: None,
    }),
    ("qwen3-8b", ModelCapabilities {
        context_window: 131_072, max_output_tokens: 8_192,
        supports_tools: true, supports_vision: false, supports_streaming: true,
        cost_per_1k_input: None, cost_per_1k_output: None,
    }),
    // DeepSeek
    ("deepseek-r1", ModelCapabilities {
        context_window: 128_000, max_output_tokens: 8_192,
        supports_tools: false, supports_vision: false, supports_streaming: true,
        cost_per_1k_input: Some(0.00055), cost_per_1k_output: Some(0.0022),
    }),
    ("deepseek-v3", ModelCapabilities {
        context_window: 128_000, max_output_tokens: 8_192,
        supports_tools: true, supports_vision: false, supports_streaming: true,
        cost_per_1k_input: Some(0.00027), cost_per_1k_output: Some(0.0011),
    }),
    // Meta Llama
    ("llama-3.3-70b", ModelCapabilities {
        context_window: 128_000, max_output_tokens: 4_096,
        supports_tools: true, supports_vision: false, supports_streaming: true,
        cost_per_1k_input: None, cost_per_1k_output: None,
    }),
    ("llama-4-scout", ModelCapabilities {
        context_window: 512_000, max_output_tokens: 8_192,
        supports_tools: true, supports_vision: true, supports_streaming: true,
        cost_per_1k_input: None, cost_per_1k_output: None,
    }),
    ("llama-4-maverick", ModelCapabilities {
        context_window: 1_048_576, max_output_tokens: 8_192,
        supports_tools: true, supports_vision: true, supports_streaming: true,
        cost_per_1k_input: None, cost_per_1k_output: None,
    }),
    // Mistral
    ("mistral-large", ModelCapabilities {
        context_window: 128_000, max_output_tokens: 8_192,
        supports_tools: true, supports_vision: false, supports_streaming: true,
        cost_per_1k_input: Some(0.002), cost_per_1k_output: Some(0.006),
    }),
    ("mistral-small", ModelCapabilities {
        context_window: 128_000, max_output_tokens: 8_192,
        supports_tools: true, supports_vision: false, supports_streaming: true,
        cost_per_1k_input: Some(0.0002), cost_per_1k_output: Some(0.0006),
    }),
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    pub model_id: String,
    pub family: ModelFamily,
    pub profile: ModelProfile,
    /// Optional explicit capabilities override. When `None`, capabilities
    /// are resolved via [`ModelCapabilities::lookup`] using `model_id`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<ModelCapabilities>,
    #[serde(default)]
    pub extensions: BTreeMap<String, serde_json::Value>,
}

impl ModelConfig {
    /// Resolve capabilities — explicit override takes priority, then static
    /// registry lookup, then conservative defaults for unknown models.
    pub fn resolve_capabilities(&self) -> ModelCapabilities {
        self.capabilities
            .clone()
            .unwrap_or_else(|| ModelCapabilities::lookup(&self.model_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_known_models() {
        let caps = ModelCapabilities::lookup("gpt-4o-mini-2024-07-18");
        assert_eq!(caps.context_window, 128_000);
        assert_eq!(caps.max_output_tokens, 16_384);
        assert!(caps.supports_tools);
        assert!(caps.supports_vision);

        let caps = ModelCapabilities::lookup("gpt-4o");
        assert_eq!(caps.context_window, 128_000);

        let caps = ModelCapabilities::lookup("claude-4-sonnet-20250514");
        assert_eq!(caps.context_window, 200_000);
        assert_eq!(caps.max_output_tokens, 64_000);

        let caps = ModelCapabilities::lookup("qwen3-235b-a22b");
        assert_eq!(caps.context_window, 131_072);
    }

    #[test]
    fn lookup_unknown_returns_default() {
        let caps = ModelCapabilities::lookup("totally-unknown-model-v9");
        assert_eq!(caps.context_window, 8_192);
        assert_eq!(caps.max_output_tokens, 4_096);
    }

    #[test]
    fn available_for_history_math() {
        let caps = ModelCapabilities {
            context_window: 128_000,
            max_output_tokens: 16_384,
            supports_tools: true,
            supports_vision: false,
            supports_streaming: true,
            cost_per_1k_input: None,
            cost_per_1k_output: None,
        };
        // Default: subtract max_output_tokens + system + tools
        let avail = caps.available_for_history(None, 500, 2000);
        assert_eq!(avail, 128_000 - 16_384 - 500 - 2000);

        // Explicit output reserve
        let avail = caps.available_for_history(Some(4096), 500, 2000);
        assert_eq!(avail, 128_000 - 4096 - 500 - 2000);
    }

    #[test]
    fn model_config_resolve_capabilities() {
        // Without override — uses static registry
        let config = ModelConfig {
            model_id: "gpt-4o".to_string(),
            family: ModelFamily::OpenAI,
            profile: ModelProfile::Generic,
            capabilities: None,
            extensions: BTreeMap::new(),
        };
        let caps = config.resolve_capabilities();
        assert_eq!(caps.context_window, 128_000);

        // With explicit override
        let config = ModelConfig {
            model_id: "my-custom-model".to_string(),
            family: ModelFamily::OpenAI,
            profile: ModelProfile::Generic,
            capabilities: Some(ModelCapabilities {
                context_window: 32_000,
                max_output_tokens: 8_000,
                supports_tools: true,
                supports_vision: false,
                supports_streaming: true,
                cost_per_1k_input: None,
                cost_per_1k_output: None,
            }),
            extensions: BTreeMap::new(),
        };
        let caps = config.resolve_capabilities();
        assert_eq!(caps.context_window, 32_000);
    }
}
