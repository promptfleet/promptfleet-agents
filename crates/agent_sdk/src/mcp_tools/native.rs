//! Native MCP backend using `rmcp` (full MCP spec: stdio + streamable HTTP).
//!
//! Supported transports:
//! - **Stdio** (child process): `command` + `args` + `env` — spawn npx, uvx, etc.
//! - **Streamable HTTP** (remote): `url` — connect to hosted MCP servers
//!
//! This backend is native-only (requires Tokio). For WASM agents,
//! see `WasmMcpBackend` which wraps `mcp_protocol::McpClient`.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use rmcp::{model::CallToolRequestParams, service::ServiceExt, transport::TokioChildProcess};
use tokio::process::Command;

use crate::mcp_tools::config::{resolve_env_vars, McpServersConfig, McpTransportType};
use crate::mcp_tools::error::McpToolError;
use crate::mcp_tools::types::{
    build_forwarded_headers_meta, McpCallResult, McpContent, McpToolDescriptor, McpToolSource,
};

// ── Shared service wrapper ──────────────────────────────────────────────────

/// Wrapper around `rmcp::RunningService<RoleClient, ()>`.
///
/// All transports (stdio, HTTP) produce the same service type once the
/// connection is established. This struct provides a uniform `McpPeerHandle`
/// implementation.
struct RmcpClientHandle {
    server_id: String,
    service: tokio::sync::Mutex<rmcp::service::RunningService<rmcp::RoleClient, ()>>,
    reconnect: RmcpReconnectConfig,
    forward_caller_auth: bool,
    has_static_service_auth: bool,
}

#[derive(Clone)]
enum RmcpReconnectConfig {
    Stdio {
        command: String,
        args: Vec<String>,
        env: HashMap<String, String>,
    },
    Http {
        url: String,
        auth_token: Option<String>,
    },
}

#[async_trait::async_trait]
trait McpPeerHandle: Send + Sync {
    async fn peer_list_tools(&self) -> Result<Vec<McpToolDescriptor>, McpToolError>;
    async fn peer_call_tool(
        &self,
        name: &str,
        args: serde_json::Value,
    ) -> Result<McpCallResult, McpToolError>;
    async fn peer_call_tool_with_headers(
        &self,
        name: &str,
        args: serde_json::Value,
        request_headers: Option<&HashMap<String, String>>,
    ) -> Result<McpCallResult, McpToolError> {
        let _ = request_headers;
        self.peer_call_tool(name, args).await
    }
}

#[async_trait::async_trait]
impl McpPeerHandle for RmcpClientHandle {
    async fn peer_list_tools(&self) -> Result<Vec<McpToolDescriptor>, McpToolError> {
        let result = match self.list_tools_once().await {
            Ok(result) => result,
            Err(first_error) if self.should_reconnect(&first_error) => {
                log::warn!(
                    "MCP server '{}' list_tools failed with recoverable transport error; reconnecting: {}",
                    self.server_id,
                    first_error
                );
                self.reconnect().await?;
                self.list_tools_once().await.map_err(|second_error| {
                    McpToolError::ListToolsFailed(format!(
                        "{}: {} (after reconnect: {})",
                        self.server_id, first_error, second_error
                    ))
                })?
            }
            Err(error) => {
                return Err(McpToolError::ListToolsFailed(format!(
                    "{}: {}",
                    self.server_id, error
                )));
            }
        };

        Ok(result
            .tools
            .iter()
            .map(|tool| McpToolDescriptor {
                server_id: self.server_id.clone(),
                name: tool.name.to_string(),
                description: tool.description.as_ref().map(|d| d.to_string()),
                input_schema: serde_json::Value::Object((*tool.input_schema).clone()),
            })
            .collect())
    }

    async fn peer_call_tool(
        &self,
        name: &str,
        args: serde_json::Value,
    ) -> Result<McpCallResult, McpToolError> {
        self.peer_call_tool_with_headers(name, args, None).await
    }

    async fn peer_call_tool_with_headers(
        &self,
        name: &str,
        args: serde_json::Value,
        request_headers: Option<&HashMap<String, String>>,
    ) -> Result<McpCallResult, McpToolError> {
        let forwarded_meta = build_forwarded_headers_meta(
            request_headers,
            self.forward_caller_auth && !self.has_static_service_auth,
        )
        .map(rmcp::model::Meta);

        let params = CallToolRequestParams {
            name: name.to_string().into(),
            arguments: args.as_object().cloned(),
            meta: None,
            task: None,
        };

        let result = match self
            .call_tool_once(params.clone(), forwarded_meta.clone())
            .await
        {
            Ok(result) => result,
            Err(first_error) if self.should_reconnect(&first_error) => {
                log::warn!(
                    "MCP server '{}' tool '{}' failed with recoverable transport error; reconnecting: {}",
                    self.server_id,
                    name,
                    first_error
                );
                self.reconnect().await?;
                self.call_tool_once(params, forwarded_meta)
                    .await
                    .map_err(|second_error| {
                        McpToolError::CallToolFailed(format!(
                            "{}::{}: {} (after reconnect: {})",
                            self.server_id, name, first_error, second_error
                        ))
                    })?
            }
            Err(error) => {
                return Err(McpToolError::CallToolFailed(format!(
                    "{}::{}: {}",
                    self.server_id, name, error
                )));
            }
        };

        let content = result
            .content
            .iter()
            .filter_map(|c| {
                if let Some(text) = c.as_text() {
                    Some(McpContent::Text(text.text.clone()))
                } else {
                    None
                }
            })
            .collect();

        Ok(McpCallResult {
            content,
            is_error: result.is_error.unwrap_or(false),
        })
    }
}

impl RmcpClientHandle {
    async fn list_tools_once(
        &self,
    ) -> Result<rmcp::model::ListToolsResult, rmcp::service::ServiceError> {
        let service = self.service.lock().await;
        service.list_tools(Default::default()).await
    }

    async fn call_tool_once(
        &self,
        params: CallToolRequestParams,
        forwarded_meta: Option<rmcp::model::Meta>,
    ) -> Result<rmcp::model::CallToolResult, rmcp::service::ServiceError> {
        let service = self.service.lock().await;
        match forwarded_meta {
            Some(meta) => {
                use rmcp::model::{ClientRequest, ServerResult};
                use rmcp::service::PeerRequestOptions;

                let request = ClientRequest::CallToolRequest(rmcp::model::CallToolRequest {
                    method: Default::default(),
                    params,
                    extensions: Default::default(),
                });
                let options = PeerRequestOptions {
                    meta: Some(meta),
                    timeout: None,
                };
                let result = service
                    .send_request_with_option(request, options)
                    .await?
                    .await_response()
                    .await?;
                match result {
                    ServerResult::CallToolResult(r) => Ok(r),
                    _ => Err(rmcp::service::ServiceError::UnexpectedResponse),
                }
            }
            None => service.call_tool(params).await,
        }
    }

    fn should_reconnect(&self, error: &rmcp::service::ServiceError) -> bool {
        let message = error.to_string().to_ascii_lowercase();
        message.contains("unexpected content type")
            || message.contains("missing session id")
            || message.contains("session not found")
            || message.contains("unauthorized")
            || message.contains("transport closed")
    }

    async fn reconnect(&self) -> Result<(), McpToolError> {
        let new_service = self.connect_service().await?;
        let mut service = self.service.lock().await;
        let _ = service.close_with_timeout(Duration::from_secs(1)).await;
        *service = new_service;
        Ok(())
    }

    async fn connect_service(
        &self,
    ) -> Result<rmcp::service::RunningService<rmcp::RoleClient, ()>, McpToolError> {
        match &self.reconnect {
            RmcpReconnectConfig::Stdio { command, args, env } => {
                NativeMcpBackend::connect_stdio_service(&self.server_id, command, args, env).await
            }
            RmcpReconnectConfig::Http { url, auth_token } => {
                NativeMcpBackend::connect_http_service(&self.server_id, url, auth_token.as_deref())
                    .await
            }
        }
    }
}

// ── Public backend ──────────────────────────────────────────────────────────

/// Native MCP backend using `rmcp`.
///
/// Connects to MCP servers as specified in [`McpServersConfig`] and
/// implements [`McpToolSource`] for unified tool access.
///
/// | Transport | Config | Example |
/// |-----------|--------|---------|
/// | Stdio (child process) | `command` + `args` + `env` | `npx -y tavily-mcp@latest` |
/// | Streamable HTTP | `url` | `https://mcp.tavily.com/mcp/` |
pub struct NativeMcpBackend {
    servers: HashMap<String, Arc<dyn McpPeerHandle>>,
}

impl NativeMcpBackend {
    /// Connect to all enabled MCP servers from config.
    ///
    /// Servers that fail to connect log a warning and are skipped —
    /// partial availability is better than total failure.
    pub async fn connect(config: &McpServersConfig) -> Result<Self, McpToolError> {
        let mut servers: HashMap<String, Arc<dyn McpPeerHandle>> = HashMap::new();

        for (server_id, entry) in config.enabled_servers() {
            let result = match entry.transport_type() {
                McpTransportType::Stdio => {
                    let command = entry.command.as_deref().ok_or_else(|| {
                        McpToolError::ConfigError(format!(
                            "Server '{}': stdio transport requires 'command'",
                            server_id
                        ))
                    })?;
                    Self::connect_stdio(server_id, command, &entry.args, &entry.env).await
                }
                McpTransportType::Remote => {
                    let url = entry.url.as_deref().ok_or_else(|| {
                        McpToolError::ConfigError(format!(
                            "Server '{}': remote transport requires 'url'",
                            server_id
                        ))
                    })?;
                    let auth_token = entry.auth_token.as_deref().map(resolve_env_vars);
                    Self::connect_http(
                        server_id,
                        url,
                        auth_token.as_deref(),
                        entry.forward_caller_auth,
                    )
                    .await
                }
                McpTransportType::Unknown => {
                    log::warn!(
                        "MCP server '{}': no 'command' or 'url' specified, skipping",
                        server_id
                    );
                    continue;
                }
            };

            match result {
                Ok(handle) => {
                    let transport_label = match entry.transport_type() {
                        McpTransportType::Stdio => {
                            format!("stdio: {}", entry.command.as_deref().unwrap_or("?"))
                        }
                        McpTransportType::Remote => {
                            format!("http: {}", entry.url.as_deref().unwrap_or("?"))
                        }
                        _ => "unknown".to_string(),
                    };
                    log::info!("MCP server '{}' connected ({})", server_id, transport_label);
                    servers.insert(server_id.clone(), Arc::new(handle));
                }
                Err(e) => {
                    log::warn!(
                        "MCP server '{}' connection failed (skipping): {}",
                        server_id,
                        e
                    );
                }
            }
        }

        if servers.is_empty() && config.enabled_servers().count() > 0 {
            log::warn!("No MCP servers connected successfully");
        }

        Ok(Self { servers })
    }

    /// Connect to a single stdio MCP server (child process).
    async fn connect_stdio(
        server_id: &str,
        command: &str,
        args: &[String],
        env: &HashMap<String, String>,
    ) -> Result<RmcpClientHandle, McpToolError> {
        let service = Self::connect_stdio_service(server_id, command, args, env).await?;
        log::debug!(
            "MCP server '{}' initialized (stdio): {:?}",
            server_id,
            service.peer_info()
        );

        Ok(RmcpClientHandle {
            server_id: server_id.to_string(),
            service: tokio::sync::Mutex::new(service),
            reconnect: RmcpReconnectConfig::Stdio {
                command: command.to_string(),
                args: args.to_vec(),
                env: env.clone(),
            },
            forward_caller_auth: false,
            has_static_service_auth: false,
        })
    }

    async fn connect_stdio_service(
        server_id: &str,
        command: &str,
        args: &[String],
        env: &HashMap<String, String>,
    ) -> Result<rmcp::service::RunningService<rmcp::RoleClient, ()>, McpToolError> {
        let mut cmd = Command::new(command);
        cmd.args(args);

        for (key, value) in env {
            cmd.env(key, resolve_env_vars(value));
        }

        let child = TokioChildProcess::new(cmd)
            .map_err(|e| McpToolError::ProcessSpawnFailed(format!("{}: {}", server_id, e)))?;

        let service: rmcp::service::RunningService<rmcp::RoleClient, ()> =
            ().serve(child).await.map_err(|e| {
                McpToolError::ConnectionError(format!("{}: stdio init failed: {}", server_id, e))
            })?;

        Ok(service)
    }

    /// Connect to a single remote MCP server (streamable HTTP).
    async fn connect_http(
        server_id: &str,
        url: &str,
        auth_token: Option<&str>,
        forward_caller_auth: bool,
    ) -> Result<RmcpClientHandle, McpToolError> {
        let service = Self::connect_http_service(server_id, url, auth_token).await?;
        log::debug!(
            "MCP server '{}' initialized (http: {}): {:?}",
            server_id,
            url,
            service.peer_info()
        );

        Ok(RmcpClientHandle {
            server_id: server_id.to_string(),
            service: tokio::sync::Mutex::new(service),
            reconnect: RmcpReconnectConfig::Http {
                url: url.to_string(),
                auth_token: auth_token.map(ToString::to_string),
            },
            forward_caller_auth,
            has_static_service_auth: auth_token.is_some(),
        })
    }

    async fn connect_http_service(
        server_id: &str,
        url: &str,
        auth_token: Option<&str>,
    ) -> Result<rmcp::service::RunningService<rmcp::RoleClient, ()>, McpToolError> {
        use rmcp::transport::streamable_http_client::{
            StreamableHttpClientTransport, StreamableHttpClientTransportConfig,
        };

        let mut config = StreamableHttpClientTransportConfig::with_uri(url);
        if let Some(token) = auth_token {
            config = config.auth_header(token);
        }
        let transport = StreamableHttpClientTransport::from_config(config);

        let service: rmcp::service::RunningService<rmcp::RoleClient, ()> =
            ().serve(transport).await.map_err(|e| {
                McpToolError::ConnectionError(format!("{}: http init failed: {}", server_id, e))
            })?;

        Ok(service)
    }
}

#[async_trait::async_trait]
impl McpToolSource for NativeMcpBackend {
    async fn list_tools(&self, server_id: &str) -> Result<Vec<McpToolDescriptor>, McpToolError> {
        let handle = self
            .servers
            .get(server_id)
            .ok_or_else(|| McpToolError::ServerNotFound(server_id.to_string()))?;
        handle.peer_list_tools().await
    }

    async fn call_tool(
        &self,
        server_id: &str,
        tool_name: &str,
        args: serde_json::Value,
    ) -> Result<McpCallResult, McpToolError> {
        self.call_tool_with_headers(server_id, tool_name, args, None)
            .await
    }

    async fn call_tool_with_headers(
        &self,
        server_id: &str,
        tool_name: &str,
        args: serde_json::Value,
        request_headers: Option<&HashMap<String, String>>,
    ) -> Result<McpCallResult, McpToolError> {
        let handle = self
            .servers
            .get(server_id)
            .ok_or_else(|| McpToolError::ServerNotFound(server_id.to_string()))?;
        handle
            .peer_call_tool_with_headers(tool_name, args, request_headers)
            .await
    }

    async fn health_check(&self, server_id: &str) -> Result<bool, McpToolError> {
        let handle = self
            .servers
            .get(server_id)
            .ok_or_else(|| McpToolError::ServerNotFound(server_id.to_string()))?;

        match handle.peer_list_tools().await {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }

    fn server_ids(&self) -> Vec<String> {
        self.servers.keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::Request,
        middleware::{self, Next},
        response::Response,
        Router,
    };
    use rmcp::{
        handler::server::{tool::ToolRouter, wrapper::Parameters},
        model::{
            CallToolResult, Content, Implementation, ListToolsResult, PaginatedRequestParams,
            ServerCapabilities, ServerInfo,
        },
        service::RequestContext,
        tool, tool_router, ErrorData as McpError, RoleServer, ServerHandler,
    };
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use tokio::net::TcpListener;

    #[derive(Clone)]
    struct TestServer {
        tool_router: ToolRouter<Self>,
    }

    #[tool_router]
    impl TestServer {
        #[tool(description = "Return pong for reconnect testing.")]
        async fn ping(
            &self,
            Parameters(_): Parameters<serde_json::Value>,
        ) -> Result<CallToolResult, McpError> {
            Ok(CallToolResult::success(vec![Content::text("pong")]))
        }
    }

    impl TestServer {
        fn new() -> Self {
            Self {
                tool_router: Self::tool_router(),
            }
        }
    }

    impl ServerHandler for TestServer {
        fn get_info(&self) -> ServerInfo {
            ServerInfo {
                protocol_version: Default::default(),
                capabilities: ServerCapabilities::builder().enable_tools().build(),
                server_info: Implementation {
                    name: "test-mcp".into(),
                    version: "0.1.0".into(),
                    title: None,
                    website_url: None,
                    icons: None,
                },
                instructions: None,
            }
        }

        fn list_tools(
            &self,
            _request: Option<PaginatedRequestParams>,
            _context: RequestContext<RoleServer>,
        ) -> impl std::future::Future<Output = Result<ListToolsResult, McpError>> + Send + '_
        {
            std::future::ready(Ok(ListToolsResult {
                tools: self.tool_router.list_all(),
                next_cursor: None,
                meta: None,
            }))
        }

        fn call_tool(
            &self,
            request: rmcp::model::CallToolRequestParams,
            context: RequestContext<RoleServer>,
        ) -> impl std::future::Future<Output = Result<CallToolResult, McpError>> + Send + '_
        {
            let ctx = rmcp::handler::server::tool::ToolCallContext::new(self, request, context);
            async move { self.tool_router.call(ctx).await }
        }
    }

    async fn drop_first_tool_request_session_header(
        request: Request<Body>,
        next: Next,
        session_posts_seen: Arc<AtomicUsize>,
    ) -> Response {
        let mut request = request;
        if request.uri().path() == "/mcp"
            && request.method() == axum::http::Method::POST
            && request.headers().contains_key("mcp-session-id")
        {
            let seen = session_posts_seen.fetch_add(1, Ordering::SeqCst);
            if seen == 1 {
                request.headers_mut().remove("mcp-session-id");
            }
        }
        next.run(request).await
    }

    #[tokio::test]
    async fn http_backend_reconnects_after_session_header_loss() {
        let session_posts_seen = Arc::new(AtomicUsize::new(0));
        let session_posts_seen_for_layer = session_posts_seen.clone();
        let service = rmcp::transport::StreamableHttpService::new(
            || Ok(TestServer::new()),
            rmcp::transport::streamable_http_server::session::local::LocalSessionManager::default()
                .into(),
            Default::default(),
        );

        let app = Router::new()
            .nest_service("/mcp", service)
            .layer(middleware::from_fn(move |request, next| {
                drop_first_tool_request_session_header(
                    request,
                    next,
                    session_posts_seen_for_layer.clone(),
                )
            }));

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let config = McpServersConfig::from_json(&format!(
            r#"{{
                "mcp_servers": {{
                    "platform": {{
                        "url": "http://{}/mcp"
                    }}
                }}
            }}"#,
            addr
        ))
        .unwrap();

        let backend = NativeMcpBackend::connect(&config).await.unwrap();

        let tools = backend.list_tools("platform").await.unwrap();
        assert!(!tools.is_empty());
        assert!(session_posts_seen.load(Ordering::SeqCst) >= 2);

        let result = backend
            .call_tool("platform", "ping", serde_json::json!({}))
            .await
            .unwrap();
        assert_eq!(result.is_error, false);
        assert_eq!(result.content.len(), 1);
        match &result.content[0] {
            McpContent::Text(text) => assert_eq!(text, "pong"),
        }

        server.abort();
    }
}
