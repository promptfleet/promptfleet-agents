use protocol_transport_core::{
    ProtocolError, StreamingPolicy, Transport, TransportError, TransportFactory, UniversalRequest,
    UniversalResponse,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ApiMode {
    #[serde(rename = "chat")]
    Chat,
    #[serde(rename = "responses")]
    Responses,
    #[serde(rename = "auto")]
    Auto,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ClientConfig {
    pub base_url: String,
    pub api_key: Option<String>,
    pub default_headers: HashMap<String, String>,
    #[serde(default)]
    pub api_mode: Option<ApiMode>,
    /// Streaming timeout policy. `None` uses built-in defaults.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub streaming: Option<StreamingPolicy>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ClientCapabilities {
    pub streaming: bool,
    pub tool_calling: bool,
    pub structured_output: bool,
}

#[derive(thiserror::Error, Debug)]
pub enum ClientError {
    #[error("transport error: {0}")]
    Transport(#[from] TransportError),
    #[error("protocol error: {0}")]
    Protocol(#[from] ProtocolError),
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("invalid configuration: {0}")]
    Config(String),
}

pub type ClientResult<T> = Result<T, ClientError>;

/// Minimal provider-agnostic client API
pub trait ModelClient: Send + Sync {
    fn capabilities(&self) -> ClientCapabilities;

    /// Generic non-streaming LLM request (chat, tools, structured output)
    fn llm_request(&self, request: serde_json::Value) -> impl std::future::Future<Output = ClientResult<serde_json::Value>> + Send;
}

/// Default HTTP-backed client (provider-agnostic JSON)
#[derive(Clone)]
pub struct HttpModelClient {
    config: ClientConfig,
}

impl HttpModelClient {
    pub fn new(config: ClientConfig) -> Self {
        log::debug!("HttpModelClient::new base_url={}", config.base_url);
        Self { config }
    }

    pub fn config(&self) -> &ClientConfig {
        &self.config
    }

    fn make_headers(&self) -> HashMap<String, String> {
        let mut headers = self.config.default_headers.clone();
        headers
            .entry("content-type".to_string())
            .or_insert("application/json".to_string());
        if let Some(key) = &self.config.api_key {
            headers
                .entry("authorization".to_string())
                .or_insert(format!("Bearer {}", key));
        }
        log::debug!("HttpModelClient::make_headers count={}", headers.len());
        headers
    }

    fn build_universal_request(&self, path: &str, body: Vec<u8>) -> UniversalRequest {
        let url = if path.starts_with("http") {
            path.to_string()
        } else {
            format!("{}{}", self.config.base_url, path)
        };
        log::debug!(
            "HttpModelClient::build_universal_request path={} url={} body_len={}",
            path,
            url,
            body.len()
        );
        UniversalRequest {
            method: "POST".to_string(),
            uri: url,
            headers: self.make_headers(),
            body,
            protocol: "REST".to_string(),
            correlation_id: uuid::Uuid::new_v4().to_string(),
        }
    }

    pub async fn post_json(
        &self,
        path: &str,
        payload: serde_json::Value,
    ) -> ClientResult<serde_json::Value> {
        log::debug!(
            "HttpModelClient::post_json path={} payload_keys={}",
            path,
            payload.as_object().map(|o| o.len()).unwrap_or(0)
        );
        let body = serde_json::to_vec(&payload)?;
        let uni_req = self.build_universal_request(path, body);
        let transport = TransportFactory::rest_http();
        let resp_res = transport.send(uni_req).await;
        match resp_res {
            Ok(resp) => {
                log::debug!(
                    "HttpModelClient::post_json status={} resp_body_len={}",
                    resp.status,
                    resp.body.len()
                );
                if resp.status >= 400 {
                    let preview = String::from_utf8_lossy(&resp.body);
                    log::warn!(
                        "HttpModelClient::post_json error status={} body={} headers={:?}",
                        resp.status,
                        preview,
                        resp.headers
                    );
                    return Err(ClientError::Transport(TransportError::Http {
                        status: resp.status,
                        message: format!("HTTP {} error", resp.status),
                        body: Some(resp.body),
                        headers: Some(resp.headers),
                    }));
                }
                let json: serde_json::Value = serde_json::from_slice(&resp.body)?;
                Ok(json)
            }
            Err(e) => {
                log::warn!("HttpModelClient::post_json transport error: {}", e);
                Err(ClientError::Transport(e))
            }
        }
    }
}

impl ModelClient for HttpModelClient {
    fn capabilities(&self) -> ClientCapabilities {
        ClientCapabilities {
            // Streaming is only available on native targets (reqwest + tokio)
            streaming: cfg!(not(target_arch = "wasm32")),
            tool_calling: true,
            structured_output: true,
        }
    }

    async fn llm_request(&self, request: serde_json::Value) -> ClientResult<serde_json::Value> {
        log::debug!("HttpModelClient::llm_request dispatching to /v1/chat/completions");
        let body = serde_json::to_vec(&request)?;
        let uni_req = self.build_universal_request("/v1/chat/completions", body);

        // Use dual-target HTTP from protocol_transport_core
        let transport = TransportFactory::rest_http();
        let resp: UniversalResponse = transport.send(uni_req).await?;
        log::debug!(
            "HttpModelClient::llm_request status={} resp_body_len={}",
            resp.status,
            resp.body.len()
        );

        if resp.status >= 400 {
            let preview = String::from_utf8_lossy(&resp.body);
            log::warn!(
                "HttpModelClient::llm_request error status={} body={} headers={:?}",
                resp.status,
                preview,
                resp.headers
            );
            return Err(ClientError::Transport(TransportError::Http {
                status: resp.status,
                message: "HTTP error".to_string(),
                body: Some(resp.body),
                headers: Some(resp.headers),
            }));
        }
        let json: serde_json::Value = serde_json::from_slice(&resp.body)?;
        Ok(json)
    }
}

// ---------------------------------------------------------------------------
// Native-only: SSE streaming transport
// ---------------------------------------------------------------------------

#[cfg(not(target_arch = "wasm32"))]
impl HttpModelClient {
    /// POST a JSON payload and return the raw `reqwest::Response` for SSE
    /// streaming.
    ///
    /// Uses streaming-first client construction: `connect_timeout` only,
    /// no total `.timeout()`. The caller should use `IdleTimeoutStream`
    /// on the response body stream for per-chunk idle enforcement.
    pub async fn post_sse(
        &self,
        path: &str,
        payload: serde_json::Value,
    ) -> ClientResult<reqwest::Response> {
        let url = if path.starts_with("http") {
            path.to_string()
        } else {
            format!("{}{}", self.config.base_url, path)
        };

        let policy = self
            .config
            .streaming
            .clone()
            .unwrap_or_default();

        log::debug!(
            "HttpModelClient::post_sse url={} payload_keys={} connect_ms={}",
            url,
            payload.as_object().map(|o| o.len()).unwrap_or(0),
            policy.connect_ms,
        );

        let client = reqwest::Client::builder()
            .connect_timeout(policy.connect_timeout())
            .build()
            .map_err(|e| {
                ClientError::Transport(TransportError::Network(format!(
                    "failed to build reqwest client: {}",
                    e
                )))
            })?;

        let mut builder = client.post(&url);

        for (k, v) in self.make_headers() {
            builder = builder.header(&k, &v);
        }

        builder = builder.header("accept", "text/event-stream");

        let response = builder.json(&payload).send().await.map_err(|e| {
            log::warn!("HttpModelClient::post_sse connection error: {}", e);
            ClientError::Transport(TransportError::Network(e.to_string()))
        })?;

        let status = response.status().as_u16();
        log::debug!("HttpModelClient::post_sse status={}", status);

        if status >= 400 {
            let body = response.bytes().await.ok().map(|b| b.to_vec());
            let preview = body
                .as_ref()
                .map(|b| String::from_utf8_lossy(b).to_string())
                .unwrap_or_default();
            log::warn!(
                "HttpModelClient::post_sse error status={} body={}",
                status,
                preview
            );
            return Err(ClientError::Transport(TransportError::Http {
                status,
                message: format!("HTTP {} error", status),
                body,
                headers: None,
            }));
        }

        Ok(response)
    }

    /// Get the effective streaming policy for this client.
    pub fn streaming_policy(&self) -> StreamingPolicy {
        self.config.streaming.clone().unwrap_or_default()
    }
}
