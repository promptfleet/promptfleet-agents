use crate::auth::AuthProvider;
use crate::error::LlmError;
use protocol_transport_core::{
    StreamingPolicy, Transport, TransportError, TransportFactory, UniversalRequest,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ApiMode {
    #[serde(rename = "chat")]
    Chat,
    #[serde(rename = "responses")]
    Responses,
    #[serde(rename = "auto")]
    Auto,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ClientCapabilities {
    pub streaming: bool,
    pub tool_calling: bool,
    pub structured_output: bool,
}

pub type ClientResult<T> = Result<T, LlmError>;

/// HTTP transport with per-request authorization (dual-target WASM / native).
#[derive(Clone)]
pub(crate) struct HttpModelClient {
    base_url: String,
    default_headers: HashMap<String, String>,
    /// Used by native `post_sse` for connect timeout; WASM uses `rest_http` only (no reqwest).
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    streaming: Option<StreamingPolicy>,
    auth: Arc<dyn AuthProvider>,
}

impl HttpModelClient {
    pub(crate) fn new(
        base_url: String,
        default_headers: HashMap<String, String>,
        streaming: Option<StreamingPolicy>,
        auth: Arc<dyn AuthProvider>,
    ) -> Self {
        log::debug!("HttpModelClient::new base_url={}", base_url);
        Self {
            base_url,
            default_headers,
            streaming,
            auth,
        }
    }

    fn make_headers(&self) -> Result<HashMap<String, String>, LlmError> {
        let mut headers = self.default_headers.clone();
        headers
            .entry("content-type".to_string())
            .or_insert("application/json".to_string());
        self.auth.authorize(&mut headers)?;
        Ok(headers)
    }

    fn build_full_url(&self, path: &str) -> String {
        let base = if path.starts_with("http") {
            path.to_string()
        } else {
            format!(
                "{}/{}",
                self.base_url.trim_end_matches('/'),
                path.trim_start_matches('/')
            )
        };

        let params = self.auth.query_params();
        if params.is_empty() {
            return base;
        }
        let sep = if base.contains('?') { '&' } else { '?' };
        let tail: String = params
            .iter()
            .enumerate()
            .map(|(i, (k, v))| {
                let prefix = if i == 0 { String::new() } else { "&".to_string() };
                format!("{prefix}{k}={v}")
            })
            .collect();
        format!("{base}{sep}{tail}")
    }

    fn build_universal_request(&self, path: &str, body: Vec<u8>) -> Result<UniversalRequest, LlmError> {
        let url = self.build_full_url(path);
        log::debug!(
            "HttpModelClient::build_universal_request path={} url={} body_len={}",
            path,
            url,
            body.len()
        );
        Ok(UniversalRequest {
            method: "POST".to_string(),
            uri: url,
            headers: self.make_headers()?,
            body,
            protocol: "REST".to_string(),
            correlation_id: uuid::Uuid::new_v4().to_string(),
        })
    }

    pub(crate) async fn post_json(
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
        let uni_req = self.build_universal_request(path, body)?;
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
                    return Err(LlmError::Transport(TransportError::Http {
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
                Err(LlmError::Transport(e))
            }
        }
    }

    pub(crate) fn capabilities(&self) -> ClientCapabilities {
        ClientCapabilities {
            streaming: true,
            tool_calling: true,
            structured_output: true,
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl HttpModelClient {
    pub(crate) async fn post_sse(
        &self,
        path: &str,
        payload: serde_json::Value,
    ) -> ClientResult<reqwest::Response> {
        let url = self.build_full_url(path);
        let policy = self.streaming.clone().unwrap_or_default();

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
                LlmError::Transport(TransportError::Network(format!(
                    "failed to build reqwest client: {}",
                    e
                )))
            })?;

        let mut builder = client.post(&url);
        for (k, v) in self.make_headers()? {
            builder = builder.header(&k, &v);
        }
        builder = builder.header("accept", "text/event-stream");

        let response = builder.json(&payload).send().await.map_err(|e| {
            log::warn!("HttpModelClient::post_sse connection error: {}", e);
            LlmError::Transport(TransportError::Network(e.to_string()))
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
            return Err(LlmError::Transport(TransportError::Http {
                status,
                message: format!("HTTP {} error", status),
                body,
                headers: None,
            }));
        }

        Ok(response)
    }
}

#[cfg(target_arch = "wasm32")]
impl HttpModelClient {
    pub(crate) async fn post_sse_buffered(
        &self,
        path: &str,
        payload: serde_json::Value,
    ) -> ClientResult<Vec<u8>> {
        log::debug!(
            "HttpModelClient::post_sse_buffered path={} payload_keys={}",
            path,
            payload.as_object().map(|o| o.len()).unwrap_or(0)
        );
        let body = serde_json::to_vec(&payload)?;
        let mut uni_req = self.build_universal_request(path, body)?;
        uni_req
            .headers
            .insert("accept".to_string(), "text/event-stream".to_string());
        let transport = TransportFactory::rest_http();
        let resp = transport.send(uni_req).await?;
        if resp.status >= 400 {
            let preview = String::from_utf8_lossy(&resp.body);
            log::warn!(
                "HttpModelClient::post_sse_buffered error status={} body={}",
                resp.status,
                preview
            );
            return Err(LlmError::Transport(TransportError::Http {
                status: resp.status,
                message: format!("HTTP {} error", resp.status),
                body: Some(resp.body),
                headers: Some(resp.headers),
            }));
        }
        Ok(resp.body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::ApiKeyAuth;

    fn test_auth() -> Arc<dyn AuthProvider> {
        Arc::new(ApiKeyAuth::new("sk-test"))
    }

    #[test]
    fn test_make_headers_default_content_type() {
        let client = HttpModelClient::new(
            String::new(),
            HashMap::new(),
            None,
            Arc::new(ApiKeyAuth::new("")),
        );
        let headers = client.make_headers().unwrap();
        assert_eq!(
            headers.get("content-type").map(String::as_str),
            Some("application/json")
        );
    }

    #[test]
    fn test_make_headers_existing_content_type_preserved() {
        let mut default_headers = HashMap::new();
        default_headers.insert(
            "content-type".to_string(),
            "application/vnd.custom+json".to_string(),
        );
        let client = HttpModelClient::new(String::new(), default_headers, None, test_auth());
        let headers = client.make_headers().unwrap();
        assert_eq!(
            headers.get("content-type").map(String::as_str),
            Some("application/vnd.custom+json")
        );
    }

    #[test]
    fn test_make_headers_api_key_bearer() {
        let client = HttpModelClient::new(String::new(), HashMap::new(), None, test_auth());
        let headers = client.make_headers().unwrap();
        assert_eq!(
            headers.get("authorization").map(String::as_str),
            Some("Bearer sk-test")
        );
    }

    #[test]
    fn test_build_request_relative_path() {
        let client = HttpModelClient::new(
            "https://api.example.com".to_string(),
            HashMap::new(),
            None,
            test_auth(),
        );
        let req = client
            .build_universal_request("/v1/chat", vec![1, 2, 3])
            .unwrap();
        assert_eq!(req.uri, "https://api.example.com/v1/chat");
        assert_eq!(req.method, "POST");
        assert_eq!(req.body, vec![1, 2, 3]);
    }

    #[test]
    fn test_build_request_absolute_url() {
        let client = HttpModelClient::new(
            "https://api.example.com".to_string(),
            HashMap::new(),
            None,
            test_auth(),
        );
        let url = "https://other.example.com/v1/x";
        let req = client.build_universal_request(url, vec![]).unwrap();
        assert_eq!(req.uri, url);
    }

    #[test]
    fn test_capabilities_values() {
        let client = HttpModelClient::new(String::new(), HashMap::new(), None, test_auth());
        let caps = client.capabilities();
        assert!(caps.tool_calling);
        assert!(caps.structured_output);
        assert!(caps.streaming);
    }

    #[test]
    fn test_build_url_appends_query_params_from_auth() {
        use crate::auth::AzureCredential;
        use crate::auth::AzureOpenAiAuth;
        let client = HttpModelClient::new(
            "https://myresource.openai.azure.com/openai/deployments/gpt4".to_string(),
            HashMap::new(),
            None,
            Arc::new(AzureOpenAiAuth::new(
                "2024-02-15-preview",
                AzureCredential::ApiKey("k".into()),
            )),
        );
        let url = client.build_full_url("/chat/completions");
        assert!(
            url.contains("api-version=2024-02-15-preview"),
            "url={url}"
        );
        assert!(url.starts_with("https://myresource.openai.azure.com/"));
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod transport_integration_tests {
    use super::*;
    use crate::auth::ApiKeyAuth;
    use crate::error::LlmError;
    use axum::{
        http::{header::CONTENT_TYPE, HeaderValue, StatusCode},
        response::Response,
        routing::post,
        Router,
    };
    use protocol_transport_core::TransportError;
    use serde_json::json;
    use tokio::{net::TcpListener, task::JoinHandle};

    async fn spawn_server(router: Router) -> (String, JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test listener");
        let addr = listener.local_addr().expect("listener local_addr");
        let handle = tokio::spawn(async move {
            axum::serve(listener, router)
                .await
                .expect("serve test router");
        });
        (format!("http://{addr}"), handle)
    }

    fn build_client(base_url: String) -> HttpModelClient {
        HttpModelClient::new(base_url, HashMap::new(), None, Arc::new(ApiKeyAuth::new("")))
    }

    async fn json_http_error_handler() -> Response<String> {
        let mut response = Response::new(r#"{"error":"rate_limited","retry_after":30}"#.to_string());
        *response.status_mut() = StatusCode::TOO_MANY_REQUESTS;
        response
            .headers_mut()
            .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        response
            .headers_mut()
            .insert("x-ratelimit-reset", HeaderValue::from_static("1735689600"));
        response
    }

    async fn invalid_json_handler() -> Response<String> {
        let mut response = Response::new("not-json".to_string());
        *response.status_mut() = StatusCode::OK;
        response
    }

    async fn sse_http_error_handler() -> Response<String> {
        let mut response = Response::new("upstream unavailable".to_string());
        *response.status_mut() = StatusCode::SERVICE_UNAVAILABLE;
        response
    }

    async fn anthropic_shaped_bad_gateway() -> Response<String> {
        let mut response = Response::new("{\"type\":\"error\",\"error\":{\"type\":\"overloaded\",\"message\":\"upstream\"}}".to_string());
        *response.status_mut() = StatusCode::BAD_GATEWAY;
        response
            .headers_mut()
            .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        response
    }

    #[tokio::test]
    async fn test_post_json_http_error_maps_transport_and_preserves_payload() {
        let app = Router::new().route("/http-error", post(json_http_error_handler));
        let (base_url, server) = spawn_server(app).await;
        let client = build_client(base_url);

        let err = client
            .post_json("/http-error", json!({"hello":"world"}))
            .await
            .expect_err("HTTP 429 must map to LlmError::Transport");
        server.abort();

        match err {
            LlmError::Transport(TransportError::Http {
                status,
                message,
                body,
                headers,
            }) => {
                assert_eq!(status, 429);
                assert!(
                    message.contains("429"),
                    "expected normalized HTTP status in message, got {message}"
                );
                let body = body.expect("HTTP body should be preserved");
                let body_text = String::from_utf8_lossy(&body);
                assert!(
                    body_text.contains("rate_limited"),
                    "expected error payload to be preserved, got {body_text}"
                );

                let headers = headers.expect("HTTP headers should be preserved");
                assert!(
                    headers
                        .iter()
                        .any(|(k, v)| k.eq_ignore_ascii_case("x-ratelimit-reset") && v == "1735689600"),
                    "expected x-ratelimit-reset header to be preserved, got {headers:?}"
                );
            }
            other => panic!("expected HTTP transport error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_post_json_invalid_json_maps_to_serialization_error() {
        let app = Router::new().route("/invalid-json", post(invalid_json_handler));
        let (base_url, server) = spawn_server(app).await;
        let client = build_client(base_url);

        let err = client
            .post_json("/invalid-json", json!({"probe":"value"}))
            .await
            .expect_err("invalid JSON body must fail");
        server.abort();

        assert!(
            matches!(err, LlmError::Serialization(_)),
            "expected serialization error, got {err:?}"
        );
    }

    #[tokio::test]
    async fn test_post_sse_http_error_maps_transport_with_body() {
        let app = Router::new().route("/sse-error", post(sse_http_error_handler));
        let (base_url, server) = spawn_server(app).await;
        let client = build_client(base_url);

        let err = client
            .post_sse("/sse-error", json!({"stream": true}))
            .await
            .expect_err("HTTP 503 must map to LlmError::Transport");
        server.abort();

        match err {
            LlmError::Transport(TransportError::Http {
                status,
                message,
                body,
                headers,
            }) => {
                assert_eq!(status, 503);
                assert_eq!(message, "HTTP 503 error");
                assert!(
                    String::from_utf8_lossy(&body.expect("body should be preserved"))
                        .contains("upstream unavailable")
                );
                assert!(
                    headers.is_none(),
                    "post_sse currently normalizes HTTP errors with no headers"
                );
            }
            other => panic!("expected HTTP transport error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_post_json_anthropic_shaped_error_maps_transport() {
        let app = Router::new().route("/v1/messages", post(anthropic_shaped_bad_gateway));
        let (base_url, server) = spawn_server(app).await;
        let client = build_client(base_url);

        let err = client
            .post_json("/v1/messages", json!({"model":"claude-3","messages":[],"max_tokens":1}))
            .await
            .expect_err("HTTP 502 must map to transport");
        server.abort();

        match err {
            LlmError::Transport(TransportError::Http { status, body, .. }) => {
                assert_eq!(status, 502);
                let body = body.expect("body");
                let txt = String::from_utf8_lossy(&body);
                assert!(
                    txt.contains("overloaded"),
                    "expected anthropic error JSON in body: {txt}"
                );
            }
            other => panic!("expected HTTP transport error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_post_sse_anthropic_shaped_error_maps_transport() {
        let app = Router::new().route("/v1/messages", post(anthropic_shaped_bad_gateway));
        let (base_url, server) = spawn_server(app).await;
        let client = build_client(base_url);

        let err = client
            .post_sse(
                "/v1/messages",
                json!({"model":"claude-3","messages":[],"max_tokens":1}),
            )
            .await
            .expect_err("HTTP 502 on SSE path must map to transport");
        server.abort();

        match err {
            LlmError::Transport(TransportError::Http { status, body, .. }) => {
                assert_eq!(status, 502);
                let body = body.expect("body");
                let txt = String::from_utf8_lossy(&body);
                assert!(
                    txt.contains("overloaded"),
                    "expected anthropic error JSON in body: {txt}"
                );
            }
            other => panic!("expected HTTP transport error, got {other:?}"),
        }
    }
}
