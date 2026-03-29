//! Native A2A HTTP Server using Axum

use a2a_protocol_core::{
    A2A_PROTOCOL_VERSION, A2AProtocol, AgentCard,
    services::{InMemoryTaskStorage, TaskStorage},
};
use anyhow::Result;
use axum::{
    Router,
    body::Body,
    http::{HeaderMap, StatusCode},
    response::{Json, Response},
    routing::{get, post},
};
use log::{debug, error, info, trace, warn};
use protocol_transport_core::{JSONRPC_VERSION, JsonRpcIncoming, JsonRpcResponse};
use serde_json::json;
use std::sync::Arc;
use tokio::net::TcpListener;
use tower_http::cors::CorsLayer;
#[cfg(feature = "event-stream")]
use {
    a2a_protocol_core::methods::params::{MessageSendParams, MessageSendResponse},
    a2a_protocol_core::streaming::StreamResponse,
    axum::response::IntoResponse,
    futures_util::Stream,
    protocol_transport_core::JsonRpcRequest,
};

#[cfg(feature = "observability")]
use {
    observability::{
        ObsHandle, SpanStatus, TraceContext, W3CTraceContext, attr, clear_current_context,
        get_current_context, metric, set_current_context, span, value, with_context_future,
    },
    web_time::Instant,
};

/// **A2A HTTP Server** - Native implementation using Axum
///
/// Wraps an A2AProtocol instance to provide HTTP transport.
/// No global state, pure dependency injection.
pub struct A2AHttpServer {
    protocol: A2AProtocol,
    app: Option<std::sync::Arc<dyn a2a_app_ports::A2AAppPort>>,
    app_async: Option<std::sync::Arc<dyn a2a_app_ports::A2AAppPortAsync>>,
    task_storage: Option<Arc<dyn TaskStorage>>,
    #[cfg(feature = "event-stream")]
    streaming_port: Option<Arc<dyn A2AStreamingAppPort>>,
    #[cfg(feature = "observability")]
    obs: Option<observability::Obs>,
}

#[cfg(feature = "event-stream")]
pub trait A2AStreamingAppPort: Send + Sync {
    fn handle_streaming_task(
        &self,
        task_id: String,
        message: a2a_protocol_core::data::Message,
        request_headers: std::collections::HashMap<String, String>,
    ) -> Result<
        std::pin::Pin<Box<dyn Stream<Item = StreamResponse> + Send>>,
        a2a_protocol_core::A2AError,
    >;
}

impl A2AHttpServer {
    /// Create new HTTP server with protocol instance
    pub fn new(protocol: A2AProtocol) -> Self {
        debug!(
            "Creating native A2A HTTP server with agent_id: {}",
            protocol.agent_card().name
        );
        Self {
            protocol,
            app: None,
            app_async: None,
            task_storage: None,
            #[cfg(feature = "event-stream")]
            streaming_port: None,
            #[cfg(feature = "observability")]
            obs: None,
        }
    }

    /// **Recommended Constructor**: Create HTTP server with full A2A standard methods
    pub fn new_with_a2a_methods(agent_card: AgentCard) -> Self {
        let agent_id = agent_card.name.clone();
        debug!(
            "Creating native A2A HTTP server with standard methods for agent: {}",
            agent_id
        );

        let mut protocol = A2AProtocol::new(agent_card);

        let storage: Arc<dyn TaskStorage> = Arc::new(InMemoryTaskStorage::new());
        debug!(
            "Initialized in-memory task storage for native server, agent: {}",
            agent_id
        );

        protocol.register_a2a_methods(Some(storage.clone()));
        info!(
            "Registered A2A standard methods for native server, agent: {}",
            agent_id
        );

        Self {
            protocol,
            app: None,
            app_async: None,
            task_storage: Some(storage),
            #[cfg(feature = "event-stream")]
            streaming_port: None,
            #[cfg(feature = "observability")]
            obs: None,
        }
    }

    /// Attach application adapter (SDK) for delegation of selected methods
    pub fn with_app_adapter(mut self, app: std::sync::Arc<dyn a2a_app_ports::A2AAppPort>) -> Self {
        self.app = Some(app);
        self
    }

    /// Attach async application adapter (SDK) for delegation of selected methods
    pub fn with_app_adapter_async(
        mut self,
        app: std::sync::Arc<dyn a2a_app_ports::A2AAppPortAsync>,
    ) -> Self {
        self.app_async = Some(app);
        self
    }

    #[cfg(feature = "event-stream")]
    pub fn with_streaming_port(mut self, port: Arc<dyn A2AStreamingAppPort>) -> Self {
        let mut card = self.protocol.agent_card().clone();
        card.capabilities
            .get_or_insert_with(a2a_protocol_core::AgentCapabilities::default)
            .streaming = true;
        self.protocol.update_agent_card(card);
        self.streaming_port = Some(port);
        self
    }

    /// Attach an observability handle (happy-path facade).
    #[cfg(feature = "observability")]
    pub fn with_observability(mut self, obs: observability::Obs) -> Self {
        self.obs = Some(obs);
        self
    }

    /// Create HTTP server with custom task storage
    pub fn new_with_storage(agent_card: AgentCard, storage: Arc<dyn TaskStorage>) -> Self {
        let agent_id = agent_card.name.clone();
        debug!(
            "Creating native A2A HTTP server with custom storage for agent: {}",
            agent_id
        );

        let mut protocol = A2AProtocol::new(agent_card);
        protocol.register_a2a_methods(Some(storage.clone()));
        info!(
            "Registered A2A methods with custom storage for native server, agent: {}",
            agent_id
        );

        Self {
            protocol,
            app: None,
            app_async: None,
            task_storage: Some(storage),
            #[cfg(feature = "event-stream")]
            streaming_port: None,
            #[cfg(feature = "observability")]
            obs: None,
        }
    }

    /// Get agent ID (for testing)
    pub fn agent_id(&self) -> &str {
        &self.protocol.agent_card().name
    }

    /// Check if server can serve (for testing)
    pub fn can_serve(&self) -> bool {
        true
    }

    /// **BUILD AXUM ROUTER**: Create router for native HTTP serving
    pub fn build_router(self) -> Router {
        let agent_id = self.agent_id().to_string();
        debug!("Building Axum router for agent: {}", agent_id);

        let server = Arc::new(self);

        let router = Router::new()
            .route(
                "/",
                post({
                    let server = server.clone();
                    move |headers, body| server.handle_jsonrpc(headers, body)
                }),
            )
            .route(
                "/jsonrpc",
                post({
                    let server = server.clone();
                    move |headers, body| server.handle_jsonrpc(headers, body)
                }),
            )
            .route(
                "/.well-known/agent-card.json",
                get({
                    let server = server.clone();
                    move || server.handle_agent_card()
                }),
            )
            .route(
                "/v1/agent/card:get",
                get({
                    let server = server.clone();
                    move || server.handle_agent_card()
                }),
            )
            .route(
                "/health",
                get({
                    let server = server.clone();
                    move || server.handle_health()
                }),
            )
            .layer(CorsLayer::permissive());

        info!("Built Axum router with CORS for agent: {}", agent_id);
        router
    }

    /// **START SERVER**: Start the server on given address (for testing)
    pub async fn serve(self, addr: &str) -> Result<()> {
        let agent_id = self.agent_id().to_string();
        info!(
            "Starting native A2A HTTP server on {} for agent: {}",
            addr, agent_id
        );

        let router = self.build_router();
        let listener = TcpListener::bind(addr).await?;

        info!(
            "Native A2A HTTP server listening on {} for agent: {}",
            addr, agent_id
        );
        axum::serve(listener, router)
            .with_graceful_shutdown(sigterm_signal())
            .await?;
        info!(
            "A2A HTTP server shut down gracefully for agent: {}",
            agent_id
        );
        Ok(())
    }

    /// **SERVE REQUEST SIMULATION**: For API compatibility testing
    pub async fn serve_request(
        &self,
        method: &str,
        path: &str,
        headers: HeaderMap,
        body: Vec<u8>,
    ) -> Result<(StatusCode, HeaderMap, Vec<u8>)> {
        let agent_id = self.agent_id();
        debug!(
            "Native serve_request simulation: {} {} for agent: {}",
            method, path, agent_id
        );
        trace!("Request headers: {:?}", headers);

        let request_str = std::str::from_utf8(&body)?;
        debug!(
            "Request body size: {} bytes for agent: {}",
            body.len(),
            agent_id
        );

        let result = match path {
            "/jsonrpc" | "/" => {
                debug!("Routing to JSON-RPC simulation for agent: {}", agent_id);
                if method != "POST" {
                    warn!(
                        "Invalid HTTP method for JSON-RPC simulation: {} (expected POST) for agent: {}",
                        method, agent_id
                    );
                    let error_response = json!({
                        "jsonrpc": JSONRPC_VERSION,
                        "error": {
                            "code": -32600,
                            "message": "Method not allowed. Use POST for JSON-RPC requests."
                        },
                        "id": null
                    });

                    let mut response_headers = HeaderMap::new();
                    response_headers.insert("content-type", "application/json".parse().unwrap());
                    response_headers.insert("allow", "POST".parse().unwrap());

                    (
                        StatusCode::METHOD_NOT_ALLOWED,
                        response_headers,
                        serde_json::to_vec(&error_response)?,
                    )
                } else {
                    debug!(
                        "Parsing JSON-RPC request for native simulation, agent: {}",
                        agent_id
                    );
                    let incoming: JsonRpcIncoming = serde_json::from_str(request_str)?;

                    let (method, id) = match &incoming {
                        JsonRpcIncoming::Request(req) => (req.method.clone(), Some(&req.id)),
                        JsonRpcIncoming::Notification(notif) => (notif.method.clone(), None),
                    };

                    debug!(
                        "Successfully parsed JSON-RPC: method={} id={:?} for agent: {}",
                        method, id, agent_id
                    );

                    #[cfg(feature = "observability")]
                    let (span_guard, start_time, prev_ctx) = {
                        let mut h = std::collections::HashMap::<String, String>::new();
                        for (k, v) in headers.iter() {
                            if let Ok(v) = v.to_str() {
                                h.insert(k.as_str().to_lowercase(), v.to_string());
                            }
                        }

                        let peer = h
                            .get("x-a2a-peer-service")
                            .map(|s| s.as_str())
                            .unwrap_or("unknown");

                        let parent = observability::Obs::extract_context(&h)
                            .ok()
                            .flatten()
                            .unwrap_or_else(W3CTraceContext::new_root);

                        let prev = get_current_context();

                        let span_guard = self.obs.as_ref().and_then(|obs| {
                            if let Some(otel) = obs.otel_plugin() {
                                Some(otel.start_span_with_w3c_context(
                                    span::A2A_SERVER,
                                    &parent,
                                    &[
                                        (attr::COMPONENT, "a2a_server"),
                                        (attr::OPERATION, method.as_str()),
                                        (attr::PEER_SERVICE, peer),
                                        (attr::RPC_SYSTEM, value::RPC_SYSTEM_JSONRPC),
                                        (attr::RPC_METHOD, method.as_str()),
                                        (attr::PF_KIND, value::KIND_A2A),
                                    ],
                                ))
                            } else {
                                Some(obs.span(
                                    span::A2A_SERVER,
                                    &[
                                        (attr::COMPONENT, "a2a_server"),
                                        (attr::OPERATION, method.as_str()),
                                        (attr::PEER_SERVICE, peer),
                                        (attr::RPC_SYSTEM, value::RPC_SYSTEM_JSONRPC),
                                        (attr::RPC_METHOD, method.as_str()),
                                        (attr::PF_KIND, value::KIND_A2A),
                                    ],
                                ))
                            }
                        });

                        if let Some(g) = &span_guard {
                            set_current_context(TraceContext {
                                trace_id: parent.trace_id.clone(),
                                span_id: g.span_id().to_string(),
                                parent_span_id: Some(parent.parent_id.clone()),
                                sampled: parent.is_sampled(),
                            });
                        }

                        (span_guard, Instant::now(), prev)
                    };

                    debug!(
                        "Delegating JSON-RPC simulation for native server, agent: {} method: {} app_present={} app_async_present={}",
                        agent_id,
                        method,
                        self.app.is_some(),
                        self.app_async.is_some()
                    );

                    let response = if let Some(app) = &self.app_async {
                        match serde_json::from_str::<serde_json::Value>(request_str) {
                            Ok(root) => {
                                let method =
                                    root.get("method").and_then(|m| m.as_str()).unwrap_or("");
                                if method == crate::method::SEND_MESSAGE {
                                    let params_val = root
                                        .get("params")
                                        .cloned()
                                        .unwrap_or(serde_json::Value::Null);
                                    let params =
                                        a2a_protocol_core::methods::params::MessageSendParams::from_json(
                                            params_val,
                                        )?;
                                    let id =
                                        root.get("id").cloned().unwrap_or(serde_json::Value::Null);
                                    let response_future = app.handle_send_message_async(params);
                                    #[cfg(feature = "observability")]
                                    let response_result = if let Some(current_context) =
                                        get_current_context()
                                    {
                                        with_context_future(current_context, response_future).await
                                    } else {
                                        response_future.await
                                    };
                                    #[cfg(not(feature = "observability"))]
                                    let response_result = response_future.await;
                                    match response_result {
                                        Ok(result) => {
                                            let result_value =
                                                if let a2a_protocol_core::methods::params::MessageSendResponse::Task(
                                                    task,
                                                ) = &result
                                                {
                                                    if let Some(storage) = &self.task_storage {
                                                        let _ = storage.store_task(task.clone());
                                                    }
                                                    serde_json::to_value(result)?
                                                } else {
                                                    serde_json::to_value(result)?
                                                };
                                            JsonRpcResponse::success(id, result_value)
                                        }
                                        Err(err) => {
                                            let jsonrpc_error = err.to_jsonrpc_error();
                                            JsonRpcResponse::error(
                                                id,
                                                jsonrpc_error.code,
                                                jsonrpc_error.message,
                                            )
                                        }
                                    }
                                } else if method == crate::method::GET_AGENT_CARD {
                                    let card = app.build_agent_card();
                                    let id =
                                        root.get("id").cloned().unwrap_or(serde_json::Value::Null);
                                    JsonRpcResponse::success(id, serde_json::to_value(card)?)
                                } else {
                                    self.protocol.handle_incoming(incoming)?.unwrap_or_else(|| {
                                        JsonRpcResponse::success(
                                            serde_json::json!(null),
                                            serde_json::json!({"status":"notification processed"}),
                                        )
                                    })
                                }
                            }
                            Err(e) => {
                                error!("Failed to parse JSON body (async app): {}", e);
                                return Err(e.into());
                            }
                        }
                    } else if let Some(app) = &self.app {
                        match serde_json::from_str::<serde_json::Value>(request_str) {
                            Ok(root) => {
                                let method =
                                    root.get("method").and_then(|m| m.as_str()).unwrap_or("");
                                if method == crate::method::SEND_MESSAGE {
                                    let params_val = root
                                        .get("params")
                                        .cloned()
                                        .unwrap_or(serde_json::Value::Null);
                                    let params =
                                        a2a_protocol_core::methods::params::MessageSendParams::from_json(
                                            params_val,
                                        )?;
                                    let id =
                                        root.get("id").cloned().unwrap_or(serde_json::Value::Null);
                                    match app.handle_send_message(params) {
                                        Ok(result) => {
                                            let result_value =
                                                if let a2a_protocol_core::methods::params::MessageSendResponse::Task(
                                                    task,
                                                ) = &result
                                                {
                                                    if let Some(storage) = &self.task_storage {
                                                        let _ = storage.store_task(task.clone());
                                                    }
                                                    serde_json::to_value(result)?
                                                } else {
                                                    serde_json::to_value(result)?
                                                };
                                            JsonRpcResponse::success(id, result_value)
                                        }
                                        Err(err) => {
                                            let jsonrpc_error = err.to_jsonrpc_error();
                                            JsonRpcResponse::error(
                                                id,
                                                jsonrpc_error.code,
                                                jsonrpc_error.message,
                                            )
                                        }
                                    }
                                } else if method == crate::method::GET_AGENT_CARD {
                                    let card = app.build_agent_card();
                                    let id =
                                        root.get("id").cloned().unwrap_or(serde_json::Value::Null);
                                    JsonRpcResponse::success(id, serde_json::to_value(card)?)
                                } else {
                                    self.protocol.handle_incoming(incoming)?.unwrap_or_else(|| {
                                        JsonRpcResponse::success(
                                            serde_json::json!(null),
                                            serde_json::json!({"status":"notification processed"}),
                                        )
                                    })
                                }
                            }
                            Err(e) => {
                                error!("Failed to parse JSON body (sync app): {}", e);
                                return Err(e.into());
                            }
                        }
                    } else {
                        debug!(
                            "Delegating to A2A protocol instance for native simulation, agent: {} method: {}",
                            agent_id, method
                        );
                        match self.protocol.handle_incoming(incoming)? {
                            Some(response) => {
                                debug!(
                                    "A2A protocol returned response for native simulation, agent: {} id: {:?}",
                                    agent_id, response.id
                                );
                                response
                            }
                            None => {
                                debug!(
                                    "A2A protocol processed notification (no response) for native simulation, agent: {}",
                                    agent_id
                                );
                                JsonRpcResponse::success(
                                    json!(null),
                                    json!({"status": "notification processed"}),
                                )
                            }
                        }
                    };

                    #[cfg(feature = "observability")]
                    if let Some(obs) = &self.obs {
                        let duration_ms = start_time.elapsed().as_secs_f64() * 1000.0;
                        let status = if response.error.is_some() {
                            value::STATUS_ERROR
                        } else {
                            value::STATUS_OK
                        };
                        let outcome = if response.error.is_some() {
                            value::OUTCOME_ERROR
                        } else {
                            value::OUTCOME_OK
                        };

                        if let Some(g) = &span_guard {
                            g.add_attribute(attr::STATUS, status);
                            g.add_attribute(attr::PF_OUTCOME, outcome);
                            g.set_status(if status == value::STATUS_OK {
                                SpanStatus::Ok
                            } else {
                                SpanStatus::Error
                            });
                        }

                        obs.metric(
                            metric::A2A_REQUESTS_TOTAL,
                            1.0,
                            &[
                                (attr::COMPONENT, "a2a_server"),
                                (attr::OPERATION, method.as_str()),
                                (attr::STATUS, status),
                            ],
                        );
                        obs.metric(
                            metric::A2A_LATENCY_MS,
                            duration_ms,
                            &[
                                (attr::COMPONENT, "a2a_server"),
                                (attr::OPERATION, method.as_str()),
                                (attr::STATUS, status),
                            ],
                        );

                        drop(span_guard);

                        if let Err(err) = obs.maybe_flush() {
                            warn!("observability:flush_failed error={}", err);
                        }

                        match prev_ctx {
                            Some(ctx) => set_current_context(ctx),
                            None => clear_current_context(),
                        }
                    }

                    let mut response_headers = HeaderMap::new();
                    response_headers.insert("content-type", "application/json".parse().unwrap());
                    response_headers.insert("a2a-version", A2A_PROTOCOL_VERSION.parse().unwrap());
                    response_headers.insert("x-server", "a2a-http-server".parse().unwrap());

                    let response_body = serde_json::to_vec(&response)?;
                    debug!(
                        "Native simulation response serialized for agent: {} (size: {} bytes)",
                        agent_id,
                        response_body.len()
                    );

                    (StatusCode::OK, response_headers, response_body)
                }
            }

            "/.well-known/agent-card.json" | "/v1/agent/card:get" => {
                debug!(
                    "Serving agent card simulation (alias) for agent: {}",
                    agent_id
                );
                let agent_card = if let Some(app) = &self.app {
                    app.build_agent_card()
                } else {
                    self.protocol.agent_card().clone()
                };
                let mut response_headers = HeaderMap::new();
                response_headers.insert("content-type", "application/json".parse().unwrap());
                response_headers.insert("a2a-version", A2A_PROTOCOL_VERSION.parse().unwrap());

                let response_body = serde_json::to_vec(&agent_card)?;
                debug!(
                    "Agent card alias served in simulation for agent: {} (size: {} bytes)",
                    agent_id,
                    response_body.len()
                );

                (StatusCode::OK, response_headers, response_body)
            }
            "/health" => {
                debug!("Serving health check simulation for agent: {}", agent_id);
                let health_data = json!({
                    "status": "healthy",
                    "server": "a2a-http-server",
                    "protocol": A2A_PROTOCOL_VERSION,
                    "agent": agent_id
                });

                let mut response_headers = HeaderMap::new();
                response_headers.insert("content-type", "application/json".parse().unwrap());

                let response_body = serde_json::to_vec(&health_data)?;
                debug!(
                    "Health check completed in simulation for agent: {}",
                    agent_id
                );

                (StatusCode::OK, response_headers, response_body)
            }
            _ => {
                warn!(
                    "Unknown path requested in simulation: {} for agent: {}",
                    path, agent_id
                );
                let error_response = json!({
                    "error": "Not Found",
                    "message": format!("Path '{}' not found", path),
                    "available_endpoints": ["/jsonrpc", "/health"]
                });

                let mut response_headers = HeaderMap::new();
                response_headers.insert("content-type", "application/json".parse().unwrap());

                (
                    StatusCode::NOT_FOUND,
                    response_headers,
                    serde_json::to_vec(&error_response)?,
                )
            }
        };

        match result.0 {
            StatusCode::OK => {
                info!(
                    "Native simulation completed: {} {} -> {} for agent: {}",
                    method, path, result.0, agent_id
                );
            }
            status => {
                warn!(
                    "Native simulation completed with error: {} {} -> {} for agent: {}",
                    method, path, status, agent_id
                );
            }
        }

        Ok(result)
    }

    // Axum handler methods
    async fn handle_jsonrpc(
        self: Arc<Self>,
        headers: HeaderMap,
        body: String,
    ) -> Result<Response<Body>, StatusCode> {
        let agent_id = self.agent_id();
        debug!(
            "Handling Axum JSON-RPC request for agent: {} (size: {} bytes)",
            agent_id,
            body.len()
        );
        trace!("Axum request headers: {:?}", headers);
        trace!("Axum request body: {}", body);

        let incoming: JsonRpcIncoming = serde_json::from_str(&body).map_err(|e| {
            error!(
                "Failed to parse JSON-RPC in Axum handler for agent: {} - {}",
                agent_id, e
            );
            StatusCode::BAD_REQUEST
        })?;

        let (method, id) = match &incoming {
            JsonRpcIncoming::Request(req) => (req.method.clone(), Some(&req.id)),
            JsonRpcIncoming::Notification(notif) => (notif.method.clone(), None),
        };

        debug!(
            "Successfully parsed Axum JSON-RPC: method={} id={:?} for agent: {}",
            method, id, agent_id
        );

        #[cfg(feature = "event-stream")]
        if method == crate::method::SEND_STREAMING_MESSAGE {
            if let JsonRpcIncoming::Request(req) = &incoming {
                if self.streaming_port.is_some() {
                    let prop_headers =
                        protocol_transport_core::sanitize_header_map(&headers).into_map();
                    return self
                        .handle_send_streaming_message(req.clone(), prop_headers)
                        .await;
                }
            }
        }

        // Observability: extract context and start span (best-effort).
        #[cfg(feature = "observability")]
        let (span_guard, start_time, prev_ctx) = {
            let mut h = std::collections::HashMap::<String, String>::new();
            for (k, v) in headers.iter() {
                if let Ok(v) = v.to_str() {
                    h.insert(k.as_str().to_lowercase(), v.to_string());
                }
            }

            let peer = h
                .get("x-a2a-peer-service")
                .map(|s| s.as_str())
                .unwrap_or("unknown");

            let parent = observability::Obs::extract_context(&h)
                .ok()
                .flatten()
                .unwrap_or_else(W3CTraceContext::new_root);

            let prev = get_current_context();

            let span_guard = self.obs.as_ref().and_then(|obs| {
                if let Some(otel) = obs.otel_plugin() {
                    Some(otel.start_span_with_w3c_context(
                        span::A2A_SERVER,
                        &parent,
                        &[
                            (attr::COMPONENT, "a2a_server"),
                            (attr::OPERATION, method.as_str()),
                            (attr::PEER_SERVICE, peer),
                            (attr::RPC_SYSTEM, value::RPC_SYSTEM_JSONRPC),
                            (attr::RPC_METHOD, method.as_str()),
                            (attr::PF_KIND, value::KIND_A2A),
                        ],
                    ))
                } else {
                    Some(obs.span(
                        span::A2A_SERVER,
                        &[
                            (attr::COMPONENT, "a2a_server"),
                            (attr::OPERATION, method.as_str()),
                            (attr::PEER_SERVICE, peer),
                            (attr::RPC_SYSTEM, value::RPC_SYSTEM_JSONRPC),
                            (attr::RPC_METHOD, method.as_str()),
                            (attr::PF_KIND, value::KIND_A2A),
                        ],
                    ))
                }
            });

            if let Some(g) = &span_guard {
                set_current_context(TraceContext {
                    trace_id: parent.trace_id.clone(),
                    span_id: g.span_id().to_string(),
                    parent_span_id: Some(parent.parent_id.clone()),
                    sampled: parent.is_sampled(),
                });
            }

            (span_guard, Instant::now(), prev)
        };

        debug!(
            "Delegating to A2A protocol from Axum handler for agent: {} method: {}",
            agent_id, method
        );

        let response = if let Some(app) = &self.app_async {
            match serde_json::from_str::<serde_json::Value>(&body) {
                Ok(root) => {
                    let method = root.get("method").and_then(|m| m.as_str()).unwrap_or("");
                    if method == crate::method::SEND_MESSAGE {
                        let params_val = root
                            .get("params")
                            .cloned()
                            .unwrap_or(serde_json::Value::Null);
                        let params =
                            a2a_protocol_core::methods::params::MessageSendParams::from_json(
                                params_val,
                            )
                            .map_err(|e| {
                                error!("SendMessage (async): failed to parse params: {}", e);
                                StatusCode::BAD_REQUEST
                            })?;
                        let id = root.get("id").cloned().unwrap_or(serde_json::Value::Null);
                        let response_future = app.handle_send_message_async(params);
                        #[cfg(feature = "observability")]
                        let response_result = if let Some(current_context) = get_current_context() {
                            with_context_future(current_context, response_future).await
                        } else {
                            response_future.await
                        };
                        #[cfg(not(feature = "observability"))]
                        let response_result = response_future.await;
                        match response_result {
                            Ok(result) => {
                                let result_value =
                                    if let a2a_protocol_core::methods::params::MessageSendResponse::Task(
                                        task,
                                    ) = &result
                                    {
                                        if let Some(storage) = &self.task_storage {
                                            let _ = storage.store_task(task.clone());
                                        }
                                        serde_json::to_value(result)
                                            .map_err(|e| {
                                                error!("SendMessage (async): failed to serialize response: {}", e);
                                                StatusCode::INTERNAL_SERVER_ERROR
                                            })?
                                    } else {
                                        serde_json::to_value(result)
                                            .map_err(|e| {
                                                error!("SendMessage (async): failed to serialize response: {}", e);
                                                StatusCode::INTERNAL_SERVER_ERROR
                                            })?
                                    };
                                JsonRpcResponse::success(id, result_value)
                            }
                            Err(err) => {
                                let jsonrpc_error = err.to_jsonrpc_error();
                                JsonRpcResponse::error(
                                    id,
                                    jsonrpc_error.code,
                                    jsonrpc_error.message,
                                )
                            }
                        }
                    } else if method == crate::method::GET_AGENT_CARD {
                        let card = app.build_agent_card();
                        let id = root.get("id").cloned().unwrap_or(serde_json::Value::Null);
                        JsonRpcResponse::success(
                            id,
                            serde_json::to_value(card).map_err(|e| {
                                error!("GetAgentCard (async): failed to serialize card: {}", e);
                                StatusCode::INTERNAL_SERVER_ERROR
                            })?,
                        )
                    } else {
                        self.protocol
                            .handle_incoming(incoming)
                            .map_err(|e| {
                                error!("protocol fallback (async): handle_incoming failed: {}", e);
                                StatusCode::INTERNAL_SERVER_ERROR
                            })?
                            .unwrap_or_else(|| {
                                JsonRpcResponse::success(
                                    serde_json::json!(null),
                                    serde_json::json!({"status":"notification processed"}),
                                )
                            })
                    }
                }
                Err(e) => {
                    error!("Failed to parse JSON body (async app): {}", e);
                    return Err(StatusCode::BAD_REQUEST);
                }
            }
        } else if let Some(app) = &self.app {
            match serde_json::from_str::<serde_json::Value>(&body) {
                Ok(root) => {
                    let method = root.get("method").and_then(|m| m.as_str()).unwrap_or("");
                    if method == crate::method::SEND_MESSAGE {
                        let params_val = root
                            .get("params")
                            .cloned()
                            .unwrap_or(serde_json::Value::Null);
                        let params =
                            a2a_protocol_core::methods::params::MessageSendParams::from_json(
                                params_val,
                            )
                            .map_err(|e| {
                                error!("SendMessage (sync): failed to parse params: {}", e);
                                StatusCode::BAD_REQUEST
                            })?;
                        let id = root.get("id").cloned().unwrap_or(serde_json::Value::Null);
                        match app.handle_send_message(params) {
                            Ok(result) => {
                                let result_value =
                                    if let a2a_protocol_core::methods::params::MessageSendResponse::Task(
                                        task,
                                    ) = &result
                                    {
                                        if let Some(storage) = &self.task_storage {
                                            let _ = storage.store_task(task.clone());
                                        }
                                        serde_json::to_value(result)
                                            .map_err(|e| {
                                                error!("SendMessage (sync): failed to serialize response: {}", e);
                                                StatusCode::INTERNAL_SERVER_ERROR
                                            })?
                                    } else {
                                        serde_json::to_value(result)
                                            .map_err(|e| {
                                                error!("SendMessage (sync): failed to serialize response: {}", e);
                                                StatusCode::INTERNAL_SERVER_ERROR
                                            })?
                                    };
                                JsonRpcResponse::success(id, result_value)
                            }
                            Err(err) => {
                                let jsonrpc_error = err.to_jsonrpc_error();
                                JsonRpcResponse::error(
                                    id,
                                    jsonrpc_error.code,
                                    jsonrpc_error.message,
                                )
                            }
                        }
                    } else if method == crate::method::GET_AGENT_CARD {
                        let card = app.build_agent_card();
                        let id = root.get("id").cloned().unwrap_or(serde_json::Value::Null);
                        JsonRpcResponse::success(
                            id,
                            serde_json::to_value(card).map_err(|e| {
                                error!("GetAgentCard (sync): failed to serialize card: {}", e);
                                StatusCode::INTERNAL_SERVER_ERROR
                            })?,
                        )
                    } else {
                        self.protocol
                            .handle_incoming(incoming)
                            .map_err(|e| {
                                error!("protocol fallback (sync): handle_incoming failed: {}", e);
                                StatusCode::INTERNAL_SERVER_ERROR
                            })?
                            .unwrap_or_else(|| {
                                JsonRpcResponse::success(
                                    serde_json::json!(null),
                                    serde_json::json!({"status":"notification processed"}),
                                )
                            })
                    }
                }
                Err(e) => {
                    error!("Failed to parse JSON body (sync app): {}", e);
                    return Err(StatusCode::BAD_REQUEST);
                }
            }
        } else {
            match self.protocol.handle_incoming(incoming) {
                Ok(Some(response)) => response,
                Ok(None) => JsonRpcResponse::success(
                    json!(null),
                    json!({"status": "notification processed"}),
                ),
                Err(_) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
            }
        };

        let response_body = serde_json::to_string(&response).map_err(|e| {
            error!(
                "Failed to serialize response in Axum handler for agent: {} - {}",
                agent_id, e
            );
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

        debug!(
            "Axum JSON-RPC response serialized for agent: {} (size: {} bytes)",
            agent_id,
            response_body.len()
        );
        trace!("Axum response body: {}", response_body);

        #[cfg(feature = "observability")]
        {
            if let Some(obs) = &self.obs {
                let duration_ms = start_time.elapsed().as_secs_f64() * 1000.0;
                let status = if response.error.is_some() {
                    value::STATUS_ERROR
                } else {
                    value::STATUS_OK
                };
                let outcome = if response.error.is_some() {
                    value::OUTCOME_ERROR
                } else {
                    value::OUTCOME_OK
                };

                if let Some(g) = &span_guard {
                    g.add_attribute(attr::STATUS, status);
                    g.add_attribute(attr::PF_OUTCOME, outcome);
                    g.set_status(if status == value::STATUS_OK {
                        SpanStatus::Ok
                    } else {
                        SpanStatus::Error
                    });
                }

                obs.metric(
                    metric::A2A_REQUESTS_TOTAL,
                    1.0,
                    &[
                        (attr::COMPONENT, "a2a_server"),
                        (attr::OPERATION, method.as_str()),
                        (attr::STATUS, status),
                    ],
                );
                obs.metric(
                    metric::A2A_LATENCY_MS,
                    duration_ms,
                    &[
                        (attr::COMPONENT, "a2a_server"),
                        (attr::OPERATION, method.as_str()),
                        (attr::STATUS, status),
                    ],
                );

                drop(span_guard);

                if let Err(err) = obs.maybe_flush() {
                    warn!("observability:flush_failed error={}", err);
                }

                match prev_ctx {
                    Some(ctx) => set_current_context(ctx),
                    None => clear_current_context(),
                }
            }
        }

        Ok(Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "application/json")
            .header("a2a-version", A2A_PROTOCOL_VERSION)
            .header("x-server", "a2a-http-server")
            .body(Body::from(response_body))
            .unwrap())
    }

    #[cfg(feature = "event-stream")]
    async fn handle_send_streaming_message(
        self: Arc<Self>,
        request: JsonRpcRequest,
        request_headers: std::collections::HashMap<String, String>,
    ) -> Result<Response<Body>, StatusCode> {
        let storage = self
            .task_storage
            .clone()
            .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
        let streaming_port = self
            .streaming_port
            .clone()
            .ok_or(StatusCode::NOT_IMPLEMENTED)?;

        let params = MessageSendParams::from_json(request.params.clone()).map_err(|e| {
            error!("SendStreamingMessage: failed to parse params: {}", e);
            StatusCode::BAD_REQUEST
        })?;
        params.validate().map_err(|e| {
            error!("SendStreamingMessage: params validation failed: {}", e);
            StatusCode::BAD_REQUEST
        })?;

        let created = self
            .protocol
            .handle_incoming(JsonRpcIncoming::Request(request.clone()))
            .map_err(|e| {
                error!("SendStreamingMessage: handle_incoming failed: {}", e);
                StatusCode::INTERNAL_SERVER_ERROR
            })?
            .ok_or_else(|| {
                error!("SendStreamingMessage: handle_incoming returned None");
                StatusCode::INTERNAL_SERVER_ERROR
            })?;

        let task_id = created
            .result
            .as_ref()
            .and_then(|v| {
                serde_json::from_value::<MessageSendResponse>(v.clone())
                    .ok()
                    .and_then(|resp| match resp {
                        MessageSendResponse::Task(task) => Some(task.id),
                        MessageSendResponse::Message(_) => None,
                    })
            })
            .or_else(|| params.message.task_id.clone())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

        if storage.get_task(&task_id).ok().flatten().is_none() {
            let context_id = params
                .message
                .context_id
                .clone()
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
            let mut task = a2a_protocol_core::data::Task::new(context_id);
            task.id = task_id.clone();
            task.add_to_history(params.message.clone());
            task.update_status(a2a_protocol_core::data::TaskState::Working);
            let _ = storage.store_task(task);
        }

        let a2a_events = streaming_port
            .handle_streaming_task(task_id.clone(), params.message.clone(), request_headers)
            .map_err(|e| {
                error!(
                    "handle_streaming_task failed for task_id={}: {}",
                    task_id, e
                );
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
        let sse = a2a_sse_stream(a2a_events);

        let mut response = sse.into_response();
        response
            .headers_mut()
            .insert("a2a-version", A2A_PROTOCOL_VERSION.parse().unwrap());
        response
            .headers_mut()
            .insert("x-server", "a2a-http-server".parse().unwrap());
        Ok(response)
    }

    async fn handle_agent_card(self: Arc<Self>) -> Result<Json<serde_json::Value>, StatusCode> {
        let agent_id = self.agent_id();
        debug!("Handling Axum agent card request for agent: {}", agent_id);

        let agent_card = self.protocol.agent_card();
        let card_value = serde_json::to_value(agent_card).unwrap();

        info!("Agent card served via Axum for agent: {}", agent_id);
        Ok(Json(card_value))
    }

    async fn handle_health(self: Arc<Self>) -> Result<Json<serde_json::Value>, StatusCode> {
        let agent_id = self.agent_id();
        debug!("Handling Axum health check for agent: {}", agent_id);

        let health_data = json!({
            "status": "healthy",
            "server": "a2a-http-server",
            "protocol": A2A_PROTOCOL_VERSION,
            "agent": agent_id
        });

        debug!("Health check completed via Axum for agent: {}", agent_id);
        Ok(Json(health_data))
    }
}

async fn sigterm_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut sigterm = signal(SignalKind::terminate()).expect("install SIGTERM handler");
        let ctrl_c = tokio::signal::ctrl_c();
        tokio::select! {
            _ = sigterm.recv() => {},
            _ = ctrl_c => {},
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
    info!("Received shutdown signal, draining active connections...");
}

#[cfg(feature = "event-stream")]
fn a2a_sse_stream(
    events: impl futures_util::Stream<Item = StreamResponse> + Send + 'static,
) -> axum::response::Sse<
    impl futures_util::Stream<Item = Result<axum::response::sse::Event, std::convert::Infallible>>,
> {
    use async_stream::stream;
    use axum::response::sse::{Event, KeepAlive, Sse};
    use futures_util::StreamExt;
    use std::time::Duration;

    let stream = stream! {
        let mut events = Box::pin(events);
        while let Some(event) = events.next().await {
            let data = serde_json::to_string(&event.to_jsonrpc_data()).unwrap_or_else(|_| "{}".to_string());
            yield Ok::<Event, std::convert::Infallible>(Event::default().event(event.event_name()).data(data));
        }
    };
    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text(":keepalive"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio;

    #[test]
    fn test_native_server_creation() {
        let agent_card = AgentCard::new("test-agent".to_string());
        let server = A2AHttpServer::new_with_a2a_methods(agent_card);
        assert_eq!(server.agent_id(), "test-agent");
        assert!(server.can_serve());
    }

    #[test]
    fn test_native_server_with_storage() {
        let agent_card = AgentCard::new("test-agent".to_string());
        let storage: Arc<dyn TaskStorage> = Arc::new(InMemoryTaskStorage::new());
        let server = A2AHttpServer::new_with_storage(agent_card, storage);
        assert_eq!(server.agent_id(), "test-agent");
    }

    #[tokio::test]
    async fn test_native_server_serve_request() {
        let agent_card = AgentCard::new("test-agent".to_string());
        let server = A2AHttpServer::new_with_a2a_methods(agent_card);

        let headers = HeaderMap::new();
        let body = r#"{"jsonrpc":"2.0","id":"test","method":"Ping","params":null}"#
            .as_bytes()
            .to_vec();

        let (status, _headers, _response_body) = server
            .serve_request("POST", "/jsonrpc", headers, body)
            .await
            .unwrap();
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn test_native_server_health_endpoint() {
        let agent_card = AgentCard::new("test-agent".to_string());
        let server = A2AHttpServer::new_with_a2a_methods(agent_card);

        let headers = HeaderMap::new();
        let body = vec![];

        let (status, _headers, response_body) = server
            .serve_request("GET", "/health", headers, body)
            .await
            .unwrap();
        assert_eq!(status, StatusCode::OK);

        let health_response: serde_json::Value = serde_json::from_slice(&response_body).unwrap();
        assert_eq!(health_response["agent"], "test-agent");
        assert_eq!(health_response["status"], "healthy");
    }

    #[tokio::test]
    async fn test_native_server_agent_card_endpoint() {
        let agent_card = AgentCard::new("test-agent".to_string());
        let server = A2AHttpServer::new_with_a2a_methods(agent_card);

        let mut headers = HeaderMap::new();
        headers.insert("content-type", "application/json".parse().unwrap());
        let body = r#"{"jsonrpc":"2.0","id":"1","method":"GetAgentCard","params":null}"#
            .as_bytes()
            .to_vec();

        let (status, _headers, response_body) = server
            .serve_request("POST", "/jsonrpc", headers, body)
            .await
            .unwrap();
        assert_eq!(status, StatusCode::OK);

        let agent_response: serde_json::Value = serde_json::from_slice(&response_body).unwrap();
        assert_eq!(agent_response["result"]["name"], "test-agent");
    }

    #[tokio::test]
    async fn test_native_server_agent_card_well_known_endpoint() {
        let agent_card = AgentCard::new("test-agent".to_string());
        let server = A2AHttpServer::new_with_a2a_methods(agent_card);

        let headers = HeaderMap::new();
        let body = vec![];

        let (status, _headers, response_body) = server
            .serve_request("GET", "/.well-known/agent-card.json", headers, body)
            .await
            .unwrap();
        assert_eq!(status, StatusCode::OK);

        let agent_response: serde_json::Value = serde_json::from_slice(&response_body).unwrap();
        assert_eq!(agent_response["name"], "test-agent");
    }

    #[tokio::test]
    async fn test_native_server_agent_card_v1_get_endpoint() {
        let agent_card = AgentCard::new("test-agent".to_string());
        let server = A2AHttpServer::new_with_a2a_methods(agent_card);

        let headers = HeaderMap::new();
        let body = vec![];

        let (status, _headers, response_body) = server
            .serve_request("GET", "/v1/agent/card:get", headers, body)
            .await
            .unwrap();
        assert_eq!(status, StatusCode::OK);

        let agent_response: serde_json::Value = serde_json::from_slice(&response_body).unwrap();
        assert_eq!(agent_response["name"], "test-agent");
    }

    #[cfg(feature = "observability")]
    #[tokio::test]
    async fn test_native_server_preserves_w3c_trace_context_for_app_adapter() {
        use a2a_app_ports::{A2AAppPortAsync, AppFuture};
        use a2a_protocol_core::data::{Message, MessageRole};
        use a2a_protocol_core::methods::params::{SendMessageRequest, SendMessageResponse};
        use observability::{TraceContext, get_current_context};

        #[derive(Clone)]
        struct TraceCapturingApp {
            seen: std::sync::Arc<std::sync::Mutex<Option<TraceContext>>>,
        }

        impl A2AAppPortAsync for TraceCapturingApp {
            fn build_agent_card(&self) -> AgentCard {
                AgentCard::new("test-agent".to_string())
            }

            fn handle_send_message_async<'a>(
                &'a self,
                _params: SendMessageRequest,
            ) -> AppFuture<'a> {
                Box::pin(async move {
                    *self.seen.lock().unwrap() = get_current_context();
                    Ok(SendMessageResponse::Message(Message::text(
                        MessageRole::Agent,
                        "ok",
                        "task-1".to_string(),
                    )))
                })
            }
        }

        let seen = std::sync::Arc::new(std::sync::Mutex::new(None));
        let app = TraceCapturingApp { seen: seen.clone() };

        let mut obs_cfg = observability::ObservabilityConfig::default();
        obs_cfg.otel.enabled = true;
        obs_cfg.otel.otlp_endpoint = "http://otel:4317".to_string();
        let obs = observability::Obs::init(obs_cfg).unwrap();

        let server = A2AHttpServer::new_with_a2a_methods(AgentCard::new("test-agent".to_string()))
            .with_app_adapter_async(std::sync::Arc::new(app))
            .with_observability(obs);

        let mut headers = HeaderMap::new();
        headers.insert("content-type", "application/json".parse().unwrap());
        headers.insert(
            "traceparent",
            "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01"
                .parse()
                .unwrap(),
        );

        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": "trace-test",
            "method": crate::method::SEND_MESSAGE,
            "params": {
                "message": {
                    "messageId": "msg-1",
                    "role": "ROLE_USER",
                    "parts": [{"text": "hello"}]
                }
            }
        })
        .to_string()
        .into_bytes();

        let (status, _, _) = server
            .serve_request("POST", "/jsonrpc", headers, body)
            .await
            .unwrap();
        assert_eq!(status, StatusCode::OK);

        let ctx = seen.lock().unwrap().clone().expect("trace context");
        assert_eq!(ctx.trace_id, "0af7651916cd43dd8448eb211c80319c");
        assert_eq!(ctx.parent_span_id.as_deref(), Some("b7ad6b7169203331"));
        assert_ne!(ctx.span_id, "b7ad6b7169203331");
        assert_eq!(ctx.span_id.len(), 16);
    }
}
