//! Per-request authentication hooks for LLM HTTP clients.
//!
//! [`AuthProvider`] runs before each request to set headers (and optionally
//! influence URL query parameters). Use [`ApiKeyAuth`] for standard
//! `Authorization: Bearer` endpoints (OpenAI, OpenRouter, many others).
//! Use [`AnthropicApiKeyAuth`] for Anthropic's `x-api-key` header.
//! Use [`AzureOpenAiAuth`] for Azure OpenAI (`api-key` header or Entra bearer)
//! plus required `api-version` query parameter.

use crate::error::LlmError;
use std::collections::HashMap;
use std::sync::Arc;

/// Called before each HTTP request to inject or refresh authorization.
pub trait AuthProvider: Send + Sync {
    /// Mutate `headers` in place (add or replace auth-related entries).
    fn authorize(&self, headers: &mut HashMap<String, String>) -> Result<(), LlmError>;

    /// Optional query parameters appended to every request URL after the path.
    ///
    /// Default: none. Azure OpenAI uses this for `api-version`.
    fn query_params(&self) -> Vec<(String, String)> {
        vec![]
    }
}

/// Standard bearer token: `Authorization: Bearer <key>`.
///
/// Covers OpenAI, OpenRouter, Together, Fireworks, vLLM gateways, etc.
#[derive(Debug, Clone)]
pub struct ApiKeyAuth {
    key: String,
}

impl ApiKeyAuth {
    pub fn new(key: impl Into<String>) -> Self {
        Self { key: key.into() }
    }

    pub fn into_arc(self) -> Arc<dyn AuthProvider> {
        Arc::new(self)
    }
}

impl AuthProvider for ApiKeyAuth {
    fn authorize(&self, headers: &mut HashMap<String, String>) -> Result<(), LlmError> {
        headers.insert(
            "authorization".to_string(),
            format!("Bearer {}", self.key),
        );
        Ok(())
    }
}

/// Anthropic Messages API authentication (`x-api-key` + version header).
#[derive(Debug, Clone)]
pub struct AnthropicApiKeyAuth {
    key: String,
    version: String,
}

impl AnthropicApiKeyAuth {
    pub fn new(key: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            version: "2023-06-01".to_string(),
        }
    }

    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.version = version.into();
        self
    }

    pub fn into_arc(self) -> Arc<dyn AuthProvider> {
        Arc::new(self)
    }
}

impl AuthProvider for AnthropicApiKeyAuth {
    fn authorize(&self, headers: &mut HashMap<String, String>) -> Result<(), LlmError> {
        headers.insert("x-api-key".to_string(), self.key.clone());
        headers.insert("anthropic-version".to_string(), self.version.clone());
        Ok(())
    }
}

/// Azure OpenAI credential: static `api-key` header or Entra ID bearer token.
#[derive(Debug, Clone)]
pub enum AzureCredential {
    /// Sent as `api-key: {key}`.
    ApiKey(String),
    /// Sent as `Authorization: Bearer {token}`. Refresh externally before expiry.
    BearerToken(String),
}

/// Azure OpenAI: correct auth header plus `api-version` on every request.
#[derive(Debug, Clone)]
pub struct AzureOpenAiAuth {
    api_version: String,
    credential: AzureCredential,
}

impl AzureOpenAiAuth {
    pub fn new(api_version: impl Into<String>, credential: AzureCredential) -> Self {
        Self {
            api_version: api_version.into(),
            credential,
        }
    }
}

impl AuthProvider for AzureOpenAiAuth {
    fn authorize(&self, headers: &mut HashMap<String, String>) -> Result<(), LlmError> {
        match &self.credential {
            AzureCredential::ApiKey(k) => {
                headers.insert("api-key".to_string(), k.clone());
            }
            AzureCredential::BearerToken(t) => {
                headers.insert("authorization".to_string(), format!("Bearer {}", t));
            }
        }
        Ok(())
    }

    fn query_params(&self) -> Vec<(String, String)> {
        vec![("api-version".to_string(), self.api_version.clone())]
    }
}
