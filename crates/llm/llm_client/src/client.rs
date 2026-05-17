//! [`LlmClient`] facade and builder — primary public entry point.

use crate::auth::AuthProvider;
use crate::error::LlmError;
use crate::model_client::ApiMode;
use crate::provider::LlmProvider;
use crate::providers::{AnthropicClient, GoogleGenerateContentClient, OpenAIClient};
use crate::stream::LlmEventStream;
use crate::types::{LlmRequest, LlmResponse};
use protocol_transport_core::StreamingPolicy;
use std::collections::HashMap;
use std::sync::Arc;

/// JSON schema family for the remote endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WireFormat {
    /// OpenAI chat completions / responses shape (incl. Azure OpenAI, OpenRouter, vLLM, …).
    OpenAiCompat,
    /// Anthropic Messages API shape (incl. Bedrock / Vertex Claude when routed that way).
    AnthropicMessages,
    /// Google Gemini GenerateContent shape (incl. Vertex AI Gemini publisher models).
    GoogleGenerateContent,
}

/// Configures an [`LlmClient`].
pub struct LlmClientBuilder {
    wire_format: WireFormat,
    base_url: String,
    auth: Option<Arc<dyn AuthProvider>>,
    default_headers: HashMap<String, String>,
    api_mode: Option<ApiMode>,
    streaming: Option<StreamingPolicy>,
    /// OpenAI-compatible chat path (default `/v1/chat/completions`).
    openai_chat_path: String,
    /// OpenAI-compatible responses path (default `/v1/responses`).
    openai_responses_path: String,
}

impl LlmClientBuilder {
    pub fn new(wire_format: WireFormat) -> Self {
        Self {
            wire_format,
            base_url: String::new(),
            auth: None,
            default_headers: HashMap::new(),
            api_mode: None,
            streaming: None,
            openai_chat_path: "/v1/chat/completions".to_string(),
            openai_responses_path: "/v1/responses".to_string(),
        }
    }

    pub fn base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into();
        self
    }

    pub fn auth(mut self, auth: impl AuthProvider + 'static) -> Self {
        self.auth = Some(Arc::new(auth));
        self
    }

    /// For OpenAI-compatible endpoints only. Ignored for Anthropic.
    pub fn api_mode(mut self, mode: ApiMode) -> Self {
        self.api_mode = Some(mode);
        self
    }

    pub fn streaming_policy(mut self, policy: StreamingPolicy) -> Self {
        self.streaming = Some(policy);
        self
    }

    pub fn default_headers(mut self, headers: HashMap<String, String>) -> Self {
        self.default_headers = headers;
        self
    }

    /// Override OpenAI chat and responses URL paths (Azure uses `/chat/completions`, etc.).
    pub fn openai_paths(mut self, chat: String, responses: String) -> Self {
        self.openai_chat_path = chat;
        self.openai_responses_path = responses;
        self
    }

    pub fn build(self) -> Result<LlmClient, LlmError> {
        let auth = self
            .auth
            .ok_or_else(|| LlmError::Config("auth is required".to_string()))?;

        if self.base_url.trim().is_empty() {
            return Err(LlmError::Config("base_url is required".to_string()));
        }

        let inner: Arc<dyn LlmProvider> = match self.wire_format {
            WireFormat::OpenAiCompat => Arc::new(OpenAIClient::new(
                self.base_url,
                self.default_headers,
                self.streaming,
                auth,
                self.api_mode,
                self.openai_chat_path,
                self.openai_responses_path,
            )),
            WireFormat::AnthropicMessages => Arc::new(AnthropicClient::new(
                self.base_url,
                self.default_headers,
                self.streaming,
                auth,
            )),
            WireFormat::GoogleGenerateContent => Arc::new(GoogleGenerateContentClient::new(
                self.base_url,
                self.default_headers,
                self.streaming,
                auth,
            )),
        };

        Ok(LlmClient { inner })
    }
}

/// High-level LLM client (single entry point for apps).
#[derive(Clone)]
pub struct LlmClient {
    inner: Arc<dyn LlmProvider>,
}

impl LlmClient {
    pub fn builder(wire_format: WireFormat) -> LlmClientBuilder {
        LlmClientBuilder::new(wire_format)
    }

    /// Build a client from a custom provider implementation.
    ///
    /// This is the extension point for provider-specific adapters that need
    /// runtime credential resolution or a wire format not covered by
    /// [`WireFormat`].
    pub fn from_provider(provider: impl LlmProvider + 'static) -> Self {
        Self {
            inner: Arc::new(provider),
        }
    }

    /// Build a client from an already shared provider implementation.
    pub fn from_provider_arc(provider: Arc<dyn LlmProvider>) -> Self {
        Self { inner: provider }
    }

    /// Convenience for Azure OpenAI chat deployments.
    pub fn azure_openai_builder(
        resource_name: &str,
        deployment_id: &str,
        api_version: &str,
        credential: crate::auth::AzureCredential,
    ) -> LlmClientBuilder {
        let base_url = format!(
            "https://{}.openai.azure.com/openai/deployments/{}",
            resource_name, deployment_id
        );
        LlmClientBuilder::new(WireFormat::OpenAiCompat)
            .base_url(base_url)
            .auth(crate::auth::AzureOpenAiAuth::new(api_version, credential))
            .openai_paths(
                "/chat/completions".to_string(),
                "/openai/responses".to_string(),
            )
    }

    pub async fn chat(&self, req: LlmRequest) -> Result<LlmResponse, LlmError> {
        self.inner.chat(req).await
    }

    pub async fn chat_stream(&self, req: LlmRequest) -> Result<LlmEventStream, LlmError> {
        self.inner.chat_stream(req).await
    }

    pub fn capabilities(&self) -> crate::model_client::ClientCapabilities {
        self.inner.capabilities()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_client::ClientCapabilities;
    use crate::provider::{ChatFuture, ChatStreamFuture};
    use crate::{ChatMessage, LlmChoice};

    struct EchoProvider;

    impl LlmProvider for EchoProvider {
        fn capabilities(&self) -> ClientCapabilities {
            ClientCapabilities {
                streaming: false,
                tool_calling: false,
                structured_output: false,
            }
        }

        fn chat<'a>(&'a self, req: LlmRequest) -> ChatFuture<'a> {
            Box::pin(async move {
                Ok(LlmResponse {
                    id: Some("echo".to_string()),
                    created: None,
                    model: Some(req.model),
                    choices: vec![LlmChoice {
                        index: 0,
                        message: ChatMessage {
                            role: "assistant".to_string(),
                            content: Some("ok".to_string()),
                            ..Default::default()
                        },
                        finish_reason: Some("stop".to_string()),
                    }],
                    usage: None,
                    tool_calls: None,
                })
            })
        }

        fn chat_stream<'a>(&'a self, _req: LlmRequest) -> ChatStreamFuture<'a> {
            Box::pin(async move { Err(LlmError::Config("streaming disabled".to_string())) })
        }
    }

    #[tokio::test]
    async fn custom_provider_can_back_llm_client() {
        let client = LlmClient::from_provider(EchoProvider);
        let response = client
            .chat(LlmRequest {
                model: "custom-model".to_string(),
                ..Default::default()
            })
            .await
            .expect("custom provider response");

        assert_eq!(response.model.as_deref(), Some("custom-model"));
        assert_eq!(
            response.choices[0].message.content.as_deref(),
            Some("ok")
        );
        assert!(!client.capabilities().streaming);
    }
}
