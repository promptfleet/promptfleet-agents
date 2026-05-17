//! # MCP Client
//!
//! MCP client implementation for connecting to MCP servers over either
//! Streamable HTTP or the older direct SSE feature path.

use crate::{
    AuthHandler, CallToolResult, ClientCapabilities, ClientInfo, InitializeRequest, JsonRpcError,
    JsonRpcRequest, JsonRpcResponse, ListToolsResult, MCP_PROTOCOL_VERSION, Tool, ToolCapabilities,
};
use protocol_transport_core::{ProtocolError, TransportError};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Mutex;

#[cfg(feature = "sse-client")]
use crate::ToolProvider;
#[cfg(feature = "sse-client")]
use protocol_transport_core::{SseTransport, Transport, TransportFactory, UniversalRequest};

const CONTENT_TYPE_JSON: &str = "application/json";
const CONTENT_TYPE_EVENT_STREAM: &str = "text/event-stream";
const HEADER_ACCEPT: &str = "Accept";
const HEADER_AUTHORIZATION: &str = "Authorization";
const HEADER_CONTENT_TYPE: &str = "Content-Type";
const HEADER_MCP_SESSION_ID: &str = "Mcp-Session-Id";

enum ClientTransport {
    StreamableHttp(StreamableHttpClientTransport),

    #[cfg(feature = "sse-client")]
    Sse {
        transport: SseTransport,
    },
}

struct StreamableHttpClientTransport {
    endpoint: String,
    auth_token: Option<String>,
    extra_headers: HashMap<String, String>,
    client_info: ClientInfo,
    initialized: Mutex<bool>,
    protocol_version: Mutex<Option<String>>,
    session_id: Mutex<Option<String>>,
    next_id: Mutex<u64>,
}

impl StreamableHttpClientTransport {
    fn new(endpoint: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
            auth_token: None,
            extra_headers: HashMap::new(),
            client_info: ClientInfo {
                name: "promptfleet-mcp-client".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                description: Some("PromptFleet Streamable HTTP MCP client".to_string()),
            },
            initialized: Mutex::new(false),
            protocol_version: Mutex::new(None),
            session_id: Mutex::new(None),
            next_id: Mutex::new(0),
        }
    }

    fn with_auth_token(mut self, token: impl Into<String>) -> Self {
        self.auth_token = Some(token.into());
        self
    }

    fn with_headers(mut self, headers: HashMap<String, String>) -> Self {
        self.extra_headers = headers;
        self
    }

    fn with_client_info(mut self, client_info: ClientInfo) -> Self {
        self.client_info = client_info;
        self
    }

    async fn initialize_if_needed(&self) -> Result<(), ProtocolError> {
        let already_initialized = *self
            .initialized
            .lock()
            .map_err(|_| ProtocolError::internal_error("streamable client init mutex poisoned"))?;
        if already_initialized {
            return Ok(());
        }

        let init_request = InitializeRequest {
            protocol_version: MCP_PROTOCOL_VERSION.to_string(),
            capabilities: ClientCapabilities {
                tools: Some(ToolCapabilities { supported: true }),
            },
            client_info: self.client_info.clone(),
        };

        let result = self
            .send_jsonrpc_raw(
                "initialize",
                Some(
                    serde_json::to_value(init_request)
                        .map_err(|e| ProtocolError::Parsing(format!("init serialize: {e}")))?,
                ),
            )
            .await?;

        let negotiated_protocol_version = result
            .get("protocolVersion")
            .or_else(|| result.get("protocol_version"))
            .and_then(|value| value.as_str())
            .map(ToString::to_string);
        if negotiated_protocol_version.is_none() {
            return Err(ProtocolError::Parsing(
                "invalid initialize result: missing protocolVersion".to_string(),
            ));
        }
        *self.protocol_version.lock().map_err(|_| {
            ProtocolError::internal_error("streamable client protocol-version mutex poisoned")
        })? = negotiated_protocol_version;

        self.send_notification_raw("notifications/initialized", None)
            .await?;

        let mut initialized = self
            .initialized
            .lock()
            .map_err(|_| ProtocolError::internal_error("streamable client init mutex poisoned"))?;
        *initialized = true;
        Ok(())
    }

    async fn list_tools_with_meta(
        &self,
        meta: Option<serde_json::Value>,
    ) -> Result<Vec<Tool>, ProtocolError> {
        let params = match meta {
            Some(meta) => json!({ "_meta": meta }),
            None => json!({}),
        };
        let result = self
            .send_jsonrpc("tools/list", Some(params), true)
            .await?;
        let list_result: ListToolsResult = serde_json::from_value(result)
            .map_err(|e| ProtocolError::Parsing(format!("invalid tools list format: {e}")))?;
        Ok(list_result.tools)
    }

    async fn call_tool(
        &self,
        name: &str,
        arguments: Option<serde_json::Value>,
        meta: Option<serde_json::Value>,
    ) -> Result<CallToolResult, ProtocolError> {
        let result = self
            .send_jsonrpc(
                "tools/call",
                Some(json!({
                    "name": name,
                    "arguments": arguments,
                    "_meta": meta,
                })),
                true,
            )
            .await?;

        serde_json::from_value(result)
            .map_err(|e| ProtocolError::Parsing(format!("invalid tool call result format: {e}")))
    }

    async fn send_jsonrpc(
        &self,
        method: &str,
        params: Option<serde_json::Value>,
        require_initialized: bool,
    ) -> Result<serde_json::Value, ProtocolError> {
        if require_initialized {
            self.initialize_if_needed().await?;
        }
        self.send_jsonrpc_raw(method, params).await
    }

    async fn send_jsonrpc_raw(
        &self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, ProtocolError> {
        let id = {
            let mut next_id = self.next_id.lock().map_err(|_| {
                ProtocolError::internal_error("streamable client request-id mutex poisoned")
            })?;
            *next_id += 1;
            *next_id
        };

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(id)),
            method: method.to_string(),
            params,
        };

        let body = serde_json::to_vec(&request)?;
        let response = self.http_post(body).await?;

        if let Some(session_id) = find_header(&response.headers, HEADER_MCP_SESSION_ID) {
            let mut stored = self.session_id.lock().map_err(|_| {
                ProtocolError::internal_error("streamable client session mutex poisoned")
            })?;
            *stored = Some(session_id.to_string());
        }

        let content_type = find_header(&response.headers, HEADER_CONTENT_TYPE)
            .map(|value| value.to_ascii_lowercase())
            .unwrap_or_else(|| CONTENT_TYPE_JSON.to_string());

        let rpc_response = if content_type.contains(CONTENT_TYPE_JSON) {
            serde_json::from_slice::<JsonRpcResponse>(&response.body).map_err(|e| {
                ProtocolError::Parsing(format!("invalid JSON-RPC response body: {e}"))
            })?
        } else if content_type.contains(CONTENT_TYPE_EVENT_STREAM) {
            parse_sse_jsonrpc_response(&response.body)?
        } else {
            return Err(ProtocolError::Parsing(format!(
                "unsupported response content-type '{content_type}'"
            )));
        };

        if let Some(error) = rpc_response.error {
            return Err(protocol_error_from_jsonrpc(error));
        }

        rpc_response
            .result
            .ok_or_else(|| ProtocolError::Parsing("missing JSON-RPC result field".to_string()))
    }

    async fn send_notification_raw(
        &self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<(), ProtocolError> {
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: None,
            method: method.to_string(),
            params,
        };

        let body = serde_json::to_vec(&request)
            .map_err(|e| ProtocolError::Parsing(format!("request serialize: {e}")))?;
        let _ = self.http_post(body).await?;
        Ok(())
    }

    async fn http_post(&self, body: Vec<u8>) -> Result<HttpResponse, ProtocolError> {
        let mut headers = HashMap::new();
        for (key, value) in &self.extra_headers {
            if !key.eq_ignore_ascii_case(HEADER_MCP_SESSION_ID) {
                headers.insert(key.clone(), value.clone());
            }
        }
        headers.insert(
            HEADER_ACCEPT.to_string(),
            format!("{CONTENT_TYPE_JSON}, {CONTENT_TYPE_EVENT_STREAM}"),
        );
        headers.insert(
            HEADER_CONTENT_TYPE.to_string(),
            CONTENT_TYPE_JSON.to_string(),
        );
        if let Some(protocol_version) = self
            .protocol_version
            .lock()
            .map_err(|_| {
                ProtocolError::internal_error("streamable client protocol-version mutex poisoned")
            })?
            .clone()
        {
            headers.insert("MCP-Protocol-Version".to_string(), protocol_version);
        }

        if let Some(token) = &self.auth_token {
            headers.insert(HEADER_AUTHORIZATION.to_string(), format!("Bearer {token}"));
        }

        if let Some(session_id) = self
            .session_id
            .lock()
            .map_err(|_| ProtocolError::internal_error("streamable client session mutex poisoned"))?
            .clone()
        {
            headers.insert(HEADER_MCP_SESSION_ID.to_string(), session_id);
        }

        #[cfg(target_arch = "wasm32")]
        {
            use spin_sdk::http::{Method, Request as SpinRequest, Response as SpinResponse, send};

            let mut builder = SpinRequest::builder();
            builder.method(Method::Post);
            builder.uri(&self.endpoint);
            for (key, value) in &headers {
                builder.header(key, value);
            }
            let request = builder.body(body).build();
            let response: SpinResponse = send(request).await.map_err(|e| {
                ProtocolError::Transport(TransportError::Network(format!(
                    "Spin HTTP send failed: {e}"
                )))
            })?;

            let response_headers = response
                .headers()
                .filter_map(|(name, value)| {
                    value
                        .as_str()
                        .map(|value| (name.to_string(), value.to_string()))
                })
                .collect::<HashMap<_, _>>();
            let status = *response.status();
            let body = response.body().to_vec();

            if !(200..300).contains(&status) {
                return Err(ProtocolError::Transport(TransportError::Http {
                    status,
                    message: format!("streamable HTTP request failed with status {status}"),
                    body: Some(body),
                    headers: Some(response_headers),
                }));
            }

            Ok(HttpResponse {
                headers: response_headers,
                body,
            })
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            let client = reqwest::Client::builder()
                .use_rustls_tls()
                .build()
                .map_err(|e| {
                    ProtocolError::Transport(TransportError::Network(format!(
                        "streamable HTTP client build failed: {e}; debug={e:?}"
                    )))
                })?;
            let mut request = client.post(&self.endpoint);
            for (key, value) in &headers {
                request = request.header(key, value);
            }
            let response = request.body(body).send().await.map_err(|e| {
                ProtocolError::Transport(TransportError::Network(format!(
                    "streamable HTTP request failed: {e}; debug={e:?}"
                )))
            })?;

            let status = response.status().as_u16();
            let response_headers = response
                .headers()
                .iter()
                .filter_map(|(name, value)| {
                    value
                        .to_str()
                        .ok()
                        .map(|value| (name.to_string(), value.to_string()))
                })
                .collect::<HashMap<_, _>>();
            let body = response.bytes().await.map_err(|e| {
                ProtocolError::Transport(TransportError::Network(format!(
                    "streamable HTTP response read failed: {e}"
                )))
            })?;
            let body = body.to_vec();

            if !(200..300).contains(&status) {
                return Err(ProtocolError::Transport(TransportError::Http {
                    status,
                    message: format!("streamable HTTP request failed with status {status}"),
                    body: Some(body),
                    headers: Some(response_headers),
                }));
            }

            Ok(HttpResponse {
                headers: response_headers,
                body,
            })
        }
    }
}

struct HttpResponse {
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

fn find_header<'a>(headers: &'a HashMap<String, String>, name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(header_name, _)| header_name.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

fn protocol_error_from_jsonrpc(error: JsonRpcError) -> ProtocolError {
    let details = error
        .data
        .map(|value| format!(" data={value}"))
        .unwrap_or_default();
    ProtocolError::Validation(format!(
        "JSON-RPC error {}: {}{}",
        error.code, error.message, details
    ))
}

fn parse_sse_jsonrpc_response(body: &[u8]) -> Result<JsonRpcResponse, ProtocolError> {
    let text = std::str::from_utf8(body)
        .map_err(|e| ProtocolError::Parsing(format!("invalid UTF-8 event-stream body: {e}")))?;
    let mut data_lines = Vec::new();

    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("data:") {
            data_lines.push(rest.trim_start().to_string());
            continue;
        }

        if line.trim().is_empty() && !data_lines.is_empty() {
            let payload = data_lines.join("\n");
            if let Ok(response) = serde_json::from_str::<JsonRpcResponse>(&payload) {
                return Ok(response);
            }
            data_lines.clear();
        }
    }

    if !data_lines.is_empty() {
        let payload = data_lines.join("\n");
        if let Ok(response) = serde_json::from_str::<JsonRpcResponse>(&payload) {
            return Ok(response);
        }
    }

    Err(ProtocolError::Parsing(format!(
        "event-stream response did not contain an MCP JSON-RPC payload; legacy SSE-only endpoints are unsupported; body={text:?}"
    )))
}

/// **MCP Client** - Connect to MCP servers
pub struct McpClient {
    auth_handler: Option<Box<dyn AuthHandler>>,
    transport: Option<ClientTransport>,
}

impl McpClient {
    /// Create new MCP client
    pub fn new() -> Self {
        Self {
            auth_handler: None,
            transport: None,
        }
    }

    /// Configure authentication
    pub fn with_auth_handler<H: AuthHandler + 'static>(mut self, handler: H) -> Self {
        self.auth_handler = Some(Box::new(handler));
        self
    }

    /// Connect to MCP server via Streamable HTTP.
    pub fn with_streamable_http_server(mut self, endpoint: &str) -> Self {
        self.transport = Some(ClientTransport::StreamableHttp(
            StreamableHttpClientTransport::new(endpoint),
        ));
        self
    }

    /// Connect to MCP server via Streamable HTTP with bearer authentication.
    pub fn with_streamable_http_server_auth(mut self, endpoint: &str, auth_token: &str) -> Self {
        self.transport = Some(ClientTransport::StreamableHttp(
            StreamableHttpClientTransport::new(endpoint).with_auth_token(auth_token),
        ));
        self
    }

    /// Add extra HTTP headers to the Streamable HTTP transport.
    pub fn with_streamable_http_headers(mut self, headers: HashMap<String, String>) -> Self {
        let transport = match self.transport.take() {
            Some(ClientTransport::StreamableHttp(transport)) => {
                ClientTransport::StreamableHttp(transport.with_headers(headers))
            }
            other => {
                self.transport = other;
                return self;
            }
        };
        self.transport = Some(transport);
        self
    }

    /// Override the Streamable HTTP client identity used during initialize.
    pub fn with_streamable_http_client_info(mut self, client_info: ClientInfo) -> Self {
        let transport = match self.transport.take() {
            Some(ClientTransport::StreamableHttp(transport)) => {
                ClientTransport::StreamableHttp(transport.with_client_info(client_info))
            }
            other => {
                self.transport = other;
                return self;
            }
        };
        self.transport = Some(transport);
        self
    }

    /// Connect to MCP server via SSE (feature: "sse-client")
    #[cfg(feature = "sse-client")]
    pub fn with_sse_server(mut self, endpoint: &str) -> Self {
        self.transport = Some(ClientTransport::Sse {
            transport: TransportFactory::mcp_sse(endpoint),
        });
        self
    }

    /// Connect to MCP server via SSE with authentication (feature: "sse-client")
    #[cfg(feature = "sse-client")]
    pub fn with_sse_server_auth(mut self, endpoint: &str, auth_token: &str) -> Self {
        self.transport = Some(ClientTransport::Sse {
            transport: TransportFactory::mcp_sse_auth(endpoint, auth_token),
        });
        self
    }

    /// List tools from server.
    pub async fn list_tools_async(&self) -> Result<Vec<Tool>, ProtocolError> {
        self.list_tools_with_meta_async(None).await
    }

    /// List tools from server with request metadata.
    pub async fn list_tools_with_meta_async(
        &self,
        meta: Option<serde_json::Value>,
    ) -> Result<Vec<Tool>, ProtocolError> {
        match self
            .transport
            .as_ref()
            .ok_or_else(|| ProtocolError::internal_error("no MCP transport configured"))?
        {
            ClientTransport::StreamableHttp(transport) => {
                transport.list_tools_with_meta(meta).await
            }

            #[cfg(feature = "sse-client")]
            ClientTransport::Sse { transport } => {
                let params = match meta {
                    Some(meta) => json!({ "_meta": meta }),
                    None => json!({}),
                };
                let result = send_sse_request(transport, "tools/list", params).await?;
                let list_result: ListToolsResult = serde_json::from_value(result).map_err(|e| {
                    ProtocolError::Parsing(format!("invalid tools list format: {e}"))
                })?;
                Ok(list_result.tools)
            }
        }
    }

    /// Call tool on server.
    pub async fn call_tool_async(
        &self,
        name: &str,
        arguments: Option<serde_json::Value>,
    ) -> Result<CallToolResult, ProtocolError> {
        self.call_tool_with_meta_async(name, arguments, None).await
    }

    pub async fn call_tool_with_meta_async(
        &self,
        name: &str,
        arguments: Option<serde_json::Value>,
        meta: Option<serde_json::Value>,
    ) -> Result<CallToolResult, ProtocolError> {
        match self
            .transport
            .as_ref()
            .ok_or_else(|| ProtocolError::internal_error("no MCP transport configured"))?
        {
            ClientTransport::StreamableHttp(transport) => {
                transport.call_tool(name, arguments, meta).await
            }

            #[cfg(feature = "sse-client")]
            ClientTransport::Sse { transport } => {
                let result = send_sse_request(
                    transport,
                    "tools/call",
                    json!({
                        "name": name,
                        "arguments": arguments,
                        "_meta": meta,
                    }),
                )
                .await?;

                serde_json::from_value(result).map_err(|e| {
                    ProtocolError::Parsing(format!("invalid tool call result format: {e}"))
                })
            }
        }
    }

    /// Initialize the current Streamable HTTP transport explicitly.
    pub async fn initialize_async(&self) -> Result<(), ProtocolError> {
        match self
            .transport
            .as_ref()
            .ok_or_else(|| ProtocolError::internal_error("no MCP transport configured"))?
        {
            ClientTransport::StreamableHttp(transport) => transport.initialize_if_needed().await,

            #[cfg(feature = "sse-client")]
            ClientTransport::Sse { .. } => Ok(()),
        }
    }

    /// Check server health.
    pub async fn health_check(&self) -> Result<(), ProtocolError> {
        match self
            .transport
            .as_ref()
            .ok_or_else(|| ProtocolError::internal_error("no MCP transport configured"))?
        {
            ClientTransport::StreamableHttp(transport) => transport.initialize_if_needed().await,

            #[cfg(feature = "sse-client")]
            ClientTransport::Sse { transport } => transport.health_check().await.map_err(|e| {
                ProtocolError::internal_error(&format!("health check failed: {:?}", e))
            }),
        }
    }
}

#[cfg(feature = "sse-client")]
async fn send_sse_request(
    transport: &SseTransport,
    method: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, ProtocolError> {
    let request = UniversalRequest {
        method: method.to_string(),
        uri: "/".to_string(),
        headers: HashMap::new(),
        body: json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
            "id": 1,
        })
        .to_string()
        .into_bytes(),
        protocol: "MCP".to_string(),
        correlation_id: format!("mcp-client-{}", method.replace('/', "-")),
    };

    let response = transport
        .send(request)
        .await
        .map_err(|e| ProtocolError::internal_error(&format!("transport error: {e:?}")))?;

    let response_json: serde_json::Value = serde_json::from_slice(&response.body)
        .map_err(|e| ProtocolError::Parsing(format!("invalid JSON response: {e}")))?;

    response_json
        .get("result")
        .cloned()
        .ok_or_else(|| ProtocolError::Parsing("missing 'result' field".to_string()))
}

#[cfg(feature = "sse-client")]
#[async_trait::async_trait]
impl ToolProvider for McpClient {
    fn list_tools(&self) -> Result<Vec<Tool>, ProtocolError> {
        Err(ProtocolError::internal_error(
            "async tool listing not supported in sync context. Use list_tools_async().",
        ))
    }

    async fn call_tool(
        &self,
        name: &str,
        _arguments: Option<serde_json::Value>,
    ) -> Result<CallToolResult, ProtocolError> {
        Err(ProtocolError::internal_error(&format!(
            "async tool calls not supported in sync context. Use call_tool_async() for tool '{name}'.",
        )))
    }
}

impl Default for McpClient {
    fn default() -> Self {
        Self::new()
    }
}

/// **MCP Client Builder** - Convenient client configuration
pub struct McpClientBuilder {
    auth_handler: Option<Box<dyn AuthHandler>>,
    streamable_http_endpoint: Option<String>,
    streamable_http_auth_token: Option<String>,

    #[cfg(feature = "sse-client")]
    sse_endpoint: Option<String>,

    #[cfg(feature = "sse-client")]
    sse_auth_token: Option<String>,
}

impl McpClientBuilder {
    /// Create new client builder
    pub fn new() -> Self {
        Self {
            auth_handler: None,
            streamable_http_endpoint: None,
            streamable_http_auth_token: None,

            #[cfg(feature = "sse-client")]
            sse_endpoint: None,

            #[cfg(feature = "sse-client")]
            sse_auth_token: None,
        }
    }

    /// Set authentication handler
    pub fn with_auth_handler<H: AuthHandler + 'static>(mut self, handler: H) -> Self {
        self.auth_handler = Some(Box::new(handler));
        self
    }

    /// Configure Streamable HTTP transport.
    pub fn with_streamable_http_server(mut self, endpoint: &str) -> Self {
        self.streamable_http_endpoint = Some(endpoint.to_string());
        self
    }

    /// Set bearer token for Streamable HTTP transport.
    pub fn with_streamable_http_auth_token(mut self, token: &str) -> Self {
        self.streamable_http_auth_token = Some(token.to_string());
        self
    }

    /// Connect to MCP server via SSE (feature: "sse-client")
    #[cfg(feature = "sse-client")]
    pub fn with_sse_server(mut self, endpoint: &str) -> Self {
        self.sse_endpoint = Some(endpoint.to_string());
        self
    }

    /// Set authentication token for SSE server (feature: "sse-client")
    #[cfg(feature = "sse-client")]
    pub fn with_auth_token(mut self, token: &str) -> Self {
        self.sse_auth_token = Some(token.to_string());
        self
    }

    /// Build the MCP client
    pub fn build(self) -> McpClient {
        let mut client = McpClient::new();

        if let Some(handler) = self.auth_handler {
            client.auth_handler = Some(handler);
        }

        if let Some(endpoint) = self.streamable_http_endpoint {
            client = if let Some(token) = self.streamable_http_auth_token {
                client.with_streamable_http_server_auth(&endpoint, &token)
            } else {
                client.with_streamable_http_server(&endpoint)
            };
        }

        #[cfg(feature = "sse-client")]
        {
            if let Some(endpoint) = self.sse_endpoint {
                client = if let Some(token) = self.sse_auth_token {
                    client.with_sse_server_auth(&endpoint, &token)
                } else {
                    client.with_sse_server(&endpoint)
                };
            }
        }

        client
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use axum::{
        Json, Router,
        body::Bytes,
        extract::State,
        http::{HeaderMap, HeaderValue, StatusCode},
        response::IntoResponse,
        routing::post,
    };
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use tokio::net::TcpListener;

    #[derive(Clone)]
    struct TestState {
        session_seen: Arc<AtomicUsize>,
        initialized_seen: Arc<AtomicUsize>,
    }

    async fn json_handler(
        State(state): State<TestState>,
        headers: HeaderMap,
        body: Bytes,
    ) -> impl IntoResponse {
        let request: serde_json::Value = serde_json::from_slice(&body).expect("json body");
        let method = request["method"].as_str().expect("method");
        match method {
            "initialize" => {
                let mut response_headers = HeaderMap::new();
                response_headers.insert(
                    HEADER_MCP_SESSION_ID,
                    HeaderValue::from_static("session-123"),
                );
                (
                    response_headers,
                    Json(json!({
                        "jsonrpc": "2.0",
                        "id": request["id"].clone(),
                        "result": {
                            "protocolVersion": MCP_PROTOCOL_VERSION,
                            "capabilities": { "tools": { "supported": true } },
                            "serverInfo": {
                                "name": "test-server",
                                "version": "0.1.0"
                            }
                        }
                    })),
                )
                    .into_response()
            }
            "notifications/initialized" => {
                assert!(
                    request.get("id").is_none(),
                    "initialized notification must not carry an id"
                );
                state.initialized_seen.fetch_add(1, Ordering::SeqCst);
                StatusCode::ACCEPTED.into_response()
            }
            "tools/list" => {
                assert_eq!(
                    state.initialized_seen.load(Ordering::SeqCst),
                    1,
                    "tools/list should only be called after notifications/initialized"
                );
                if headers
                    .get(HEADER_MCP_SESSION_ID)
                    .and_then(|value| value.to_str().ok())
                    == Some("session-123")
                {
                    state.session_seen.fetch_add(1, Ordering::SeqCst);
                }
                Json(json!({
                    "jsonrpc": "2.0",
                    "id": request["id"].clone(),
                    "result": {
                        "tools": [{
                            "name": "search_agents",
                            "description": "Search directory",
                            "inputSchema": { "type": "object", "properties": {} }
                        }]
                    }
                }))
                .into_response()
            }
            "tools/call" => {
                let body = format!(
                    "event: message\ndata: {}\n\n",
                    json!({
                        "jsonrpc": "2.0",
                        "id": request["id"].clone(),
                        "result": {
                            "content": [{ "type": "text", "text": "{\"ok\":true}" }],
                            "isError": false
                        }
                    })
                );
                ([(HEADER_CONTENT_TYPE, CONTENT_TYPE_EVENT_STREAM)], body).into_response()
            }
            _ => StatusCode::NOT_FOUND.into_response(),
        }
    }

    async fn error_handler(body: Bytes) -> impl IntoResponse {
        let request: serde_json::Value = serde_json::from_slice(&body).expect("json body");
        Json(json!({
            "jsonrpc": "2.0",
            "id": request["id"].clone(),
            "error": {
                "code": -32602,
                "message": "bad input"
            }
        }))
    }

    async fn legacy_sse_handler(_body: Bytes) -> impl IntoResponse {
        (
            [(HEADER_CONTENT_TYPE, CONTENT_TYPE_EVENT_STREAM)],
            "event: endpoint\ndata: /messages?session=abc\n\n".to_string(),
        )
    }

    async fn start_server(app: Router) -> (String, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("local addr");
        let handle = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("server");
        });
        (format!("http://{addr}/mcp"), handle)
    }

    #[tokio::test]
    async fn streamable_http_initializes_and_replays_session_header() {
        let state = TestState {
            session_seen: Arc::new(AtomicUsize::new(0)),
            initialized_seen: Arc::new(AtomicUsize::new(0)),
        };
        let session_seen = state.session_seen.clone();
        let initialized_seen = state.initialized_seen.clone();
        let app = Router::new()
            .route("/mcp", post(json_handler))
            .with_state(state);
        let (url, handle) = start_server(app).await;

        let client = McpClient::new().with_streamable_http_server(&url);
        let tools = client.list_tools_async().await.expect("list tools");

        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "search_agents");
        assert_eq!(session_seen.load(Ordering::SeqCst), 1);
        assert_eq!(initialized_seen.load(Ordering::SeqCst), 1);

        handle.abort();
    }

    #[tokio::test]
    async fn streamable_http_parses_event_stream_tool_results() {
        let app = Router::new()
            .route("/mcp", post(json_handler))
            .with_state(TestState {
                session_seen: Arc::new(AtomicUsize::new(0)),
                initialized_seen: Arc::new(AtomicUsize::new(0)),
            });
        let (url, handle) = start_server(app).await;

        let client = McpClient::new().with_streamable_http_server(&url);
        let result = client
            .call_tool_async("search_agents", Some(json!({"q": "planner"})))
            .await
            .expect("tool call");

        assert_eq!(result.is_error, Some(false));
        assert_eq!(result.content.len(), 1);

        handle.abort();
    }

    #[tokio::test]
    async fn streamable_http_surfaces_jsonrpc_errors() {
        let app = Router::new().route("/mcp", post(error_handler));
        let (url, handle) = start_server(app).await;

        let client = McpClient::new().with_streamable_http_server(&url);
        let error = client.list_tools_async().await.expect_err("should fail");

        assert!(error.to_string().contains("JSON-RPC error -32602"));

        handle.abort();
    }

    #[tokio::test]
    async fn streamable_http_rejects_legacy_sse_only_responses() {
        let app = Router::new().route("/mcp", post(legacy_sse_handler));
        let (url, handle) = start_server(app).await;

        let client = McpClient::new().with_streamable_http_server(&url);
        let error = client.list_tools_async().await.expect_err("should fail");

        assert!(
            error
                .to_string()
                .contains("legacy SSE-only endpoints are unsupported")
        );

        handle.abort();
    }
}
