#![cfg(not(target_arch = "wasm32"))]
#![cfg(feature = "mcp-client")]

use agent_sdk::mcp_tools::{McpServersConfig, McpToolSource, NativeMcpBackend};
use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, HeaderValue},
    response::IntoResponse,
    routing::post,
    Json, Router,
};
use mcp_protocol::McpClient;
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
use tokio::time::{timeout, Duration};

fn expected_tool_name() -> &'static str {
    "search_agents"
}

fn expected_tool_json(query: &str) -> serde_json::Value {
    serde_json::json!({
        "items": [{
            "agent": {
                "aid": "A-01",
                "name": "Planner"
            },
            "metadata": {
                "echoQuery": query
            }
        }],
        "page": {
            "top_k": 1,
            "offset": 0,
            "next_offset": null
        }
    })
}

#[derive(Clone)]
struct RmcpServer {
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl RmcpServer {
    #[tool(description = "Return a deterministic directory-like payload.")]
    async fn search_agents(
        &self,
        Parameters(params): Parameters<serde_json::Value>,
    ) -> Result<CallToolResult, McpError> {
        let q = params
            .get("q")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        Ok(CallToolResult::success(vec![Content::text(
            expected_tool_json(q).to_string(),
        )]))
    }
}

impl RmcpServer {
    fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }
}

impl ServerHandler for RmcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: Default::default(),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation {
                name: "rmcp-test-server".into(),
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
    ) -> impl std::future::Future<Output = Result<ListToolsResult, McpError>> + Send + '_ {
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
    ) -> impl std::future::Future<Output = Result<CallToolResult, McpError>> + Send + '_ {
        let ctx = rmcp::handler::server::tool::ToolCallContext::new(self, request, context);
        async move { self.tool_router.call(ctx).await }
    }
}

async fn start_rmcp_server() -> (String, tokio::task::JoinHandle<()>) {
    let service = rmcp::transport::StreamableHttpService::new(
        || Ok(RmcpServer::new()),
        rmcp::transport::streamable_http_server::session::local::LocalSessionManager::default()
            .into(),
        Default::default(),
    );
    let app = Router::new().nest_service("/mcp", service);
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });
    (format!("http://{addr}/mcp"), handle)
}

#[derive(Clone)]
struct CustomState {
    session_replays: Arc<AtomicUsize>,
}

async fn custom_streamable_handler(
    State(state): State<CustomState>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let request: serde_json::Value = serde_json::from_slice(&body).expect("json body");
    let method = request["method"].as_str().expect("method");

    match method {
        "initialize" => {
            let mut response_headers = HeaderMap::new();
            response_headers.insert("Mcp-Session-Id", HeaderValue::from_static("session-123"));
            (
                response_headers,
                Json(serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": request["id"].clone(),
                    "result": {
                        "protocolVersion": "2025-06-18",
                        "capabilities": {
                            "tools": {}
                        },
                        "serverInfo": {
                            "name": "custom-streamable-server",
                            "version": "0.1.0"
                        }
                    }
                })),
            )
                .into_response()
        }
        "tools/list" => {
            if headers
                .get("Mcp-Session-Id")
                .and_then(|value| value.to_str().ok())
                == Some("session-123")
            {
                state.session_replays.fetch_add(1, Ordering::SeqCst);
            }
            Json(serde_json::json!({
                "jsonrpc": "2.0",
                "id": request["id"].clone(),
                "result": {
                    "tools": [{
                        "name": expected_tool_name(),
                        "description": "Search directory",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "q": { "type": "string" }
                            },
                            "required": ["q"],
                            "additionalProperties": false
                        }
                    }]
                }
            }))
            .into_response()
        }
        "tools/call" => {
            let q = request["params"]["arguments"]["q"]
                .as_str()
                .unwrap_or_default();
            let body = format!(
                "event: message\ndata: {}\n\n",
                serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": request["id"].clone(),
                    "result": {
                        "content": [{
                            "type": "text",
                            "text": expected_tool_json(q).to_string()
                        }],
                        "isError": false
                    }
                })
            );
            ([("content-type", "text/event-stream")], body).into_response()
        }
        other => Json(serde_json::json!({
            "jsonrpc": "2.0",
            "id": request["id"].clone(),
            "error": {
                "code": -32601,
                "message": format!("unknown method {other}")
            }
        }))
        .into_response(),
    }
}

async fn start_custom_streamable_server() -> (
    String,
    Arc<AtomicUsize>,
    tokio::task::JoinHandle<()>,
) {
    let state = CustomState {
        session_replays: Arc::new(AtomicUsize::new(0)),
    };
    let session_replays = state.session_replays.clone();
    let app = Router::new()
        .route("/mcp", post(custom_streamable_handler))
        .with_state(state);
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });
    (format!("http://{addr}/mcp"), session_replays, handle)
}

#[tokio::test]
async fn native_rmcp_and_streamable_client_match_expected_contract() {
    timeout(Duration::from_secs(20), async {
        let (native_url, native_handle) = start_rmcp_server().await;
        let native_config = McpServersConfig::from_json(&format!(
            r#"{{
                "mcp_servers": {{
                    "directory": {{
                        "url": "{native_url}"
                    }}
                }}
            }}"#
        ))
        .expect("native config");

        let native = NativeMcpBackend::connect(&native_config)
            .await
            .expect("native backend");
        let native_tools = native.list_tools("directory").await.expect("native tools");
        assert_eq!(native_tools.len(), 1);
        assert_eq!(native_tools[0].name, expected_tool_name());

        let native_result = native
            .call_tool(
                "directory",
                expected_tool_name(),
                serde_json::json!({ "q": "planner" }),
            )
            .await
            .expect("native tool call");
        assert_eq!(native_result.to_json(), expected_tool_json("planner"));
        native_handle.abort();

        let (custom_url, session_replays, custom_handle) = start_custom_streamable_server().await;
        let wasm_compatible = McpClient::new().with_streamable_http_server(&custom_url);
        let wasm_tools = wasm_compatible
            .list_tools_async()
            .await
            .expect("streamable tools");
        assert_eq!(wasm_tools.len(), 1);
        assert_eq!(wasm_tools[0].name, expected_tool_name());
        assert!(session_replays.load(Ordering::SeqCst) >= 1);

        let wasm_result = wasm_compatible
            .call_tool_async(expected_tool_name(), Some(serde_json::json!({ "q": "planner" })))
            .await
            .expect("streamable tool call");
        let wasm_json = {
            let content = wasm_result
                .content
                .iter()
                .filter_map(|item| match item {
                    mcp_protocol::Content::Text { text } => Some(text.as_str()),
                })
                .collect::<Vec<_>>()
                .join("\n");
            serde_json::from_str::<serde_json::Value>(&content).expect("json text")
        };
        assert_eq!(wasm_json, expected_tool_json("planner"));
        custom_handle.abort();
    })
    .await
    .expect("integration test timed out");
}
