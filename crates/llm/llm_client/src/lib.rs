//! Provider-neutral LLM HTTP client: typed requests, streaming SSE events,
//! model profiles, and a [`LlmClient`] facade.
//!
//! # Quick start
//!
//! ```no_run
//! use llm_client::auth::ApiKeyAuth;
//! use llm_client::client::{LlmClient, WireFormat};
//! use llm_client::{ChatMessage, LlmRequest};
//!
//! # async fn demo() -> Result<(), llm_client::LlmError> {
//! let client = LlmClient::builder(WireFormat::OpenAiCompat)
//!     .base_url("https://api.openai.com/v1")
//!     .auth(ApiKeyAuth::new(std::env::var("OPENAI_API_KEY").unwrap()))
//!     .build()?;
//!
//! let resp = client
//!     .chat(LlmRequest {
//!         model: "gpt-4o-mini".into(),
//!         messages: vec![ChatMessage {
//!             role: "user".into(),
//!             content: Some("Hello".into()),
//!             ..Default::default()
//!         }],
//!         ..Default::default()
//!     })
//!     .await?;
//! # let _ = resp;
//! # Ok(())
//! # }
//! ```
//!
//! # Wire formats
//!
//! [`WireFormat`] selects JSON shape (OpenAI-compatible vs Anthropic Messages), not a
//! single vendor — OpenRouter uses [`WireFormat::OpenAiCompat`]; Bedrock Claude
//! often uses [`WireFormat::AnthropicMessages`].
//!
//! On native targets, streaming is incremental over SSE. On WASM, responses are
//! buffered then parsed (see [`stream::sse_event_stream_from_buffer`]).

pub mod auth;
pub mod client;
pub mod error;
pub mod model_client;
pub mod prepare;
pub mod profile;
pub mod provider;
pub(crate) mod providers;
pub mod stream;
pub mod types;

pub use auth::{AnthropicApiKeyAuth, ApiKeyAuth, AuthProvider, AzureCredential, AzureOpenAiAuth};
pub use client::{LlmClient, LlmClientBuilder, WireFormat};
pub use error::{LlmError, LlmResult};
pub use model_client::{ApiMode, ClientCapabilities};
pub use profile::{ModelCapabilities, ModelConfig, ModelFamily, ModelProfile};
pub use provider::{ChatFuture, ChatStreamFuture, LlmProvider};
pub use protocol_transport_core::StreamingPolicy;
pub use stream::{LlmEventStream, SseParser, StreamEvent};
pub use types::*;
