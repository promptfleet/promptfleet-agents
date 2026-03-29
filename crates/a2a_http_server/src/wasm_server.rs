//! WASM A2A HTTP Server using Spin SDK

use a2a_protocol_core::{
    A2A_PROTOCOL_VERSION, A2AProtocol, AgentCard,
    services::{InMemoryTaskStorage, TaskStorage},
};
use anyhow::Result;
use log::{debug, error, info, trace, warn};
use protocol_transport_core::{JSONRPC_VERSION, JsonRpcIncoming, JsonRpcResponse};
use serde_json::json;
use spin_sdk::http::{Method, Request as SpinRequest, Response as SpinResponse};
use std::sync::Arc;
#[cfg(feature = "observability")]
use web_time::Instant;

#[cfg(feature = "observability")]
use observability::{
    ObsHandle, SpanStatus, TraceContext, W3CTraceContext, attr, clear_current_context,
    get_current_context, metric, set_current_context, span, value, with_context_future,
};

const STATUS_OK: &str = "ok";

/// **A2A HTTP Server** - WASM implementation using Spin SDK
///
/// Wraps an A2AProtocol instance to provide HTTP transport.
/// No global state, pure dependency injection.
pub struct A2AHttpServer {
    protocol: A2AProtocol,
    app: Option<std::sync::Arc<dyn a2a_app_ports::A2AAppPort>>,
    app_async: Option<std::sync::Arc<dyn a2a_app_ports::A2AAppPortAsync>>,
    task_storage: Option<Arc<dyn TaskStorage>>,
    #[cfg(feature = "observability")]
    obs: Option<observability::Obs>,
}

impl A2AHttpServer {
    /// Create new HTTP server with protocol instance
    pub fn new(protocol: A2AProtocol) -> Self {
        debug!(
            "Creating A2A HTTP server with agent_id: {}",
            protocol.agent_card().name
        );
        Self {
            protocol,
            app: None,
            app_async: None,
            task_storage: None,
            #[cfg(feature = "observability")]
            obs: None,
        }
    }

    /// **Recommended Constructor**: Create HTTP server with full A2A standard methods
    pub fn new_with_a2a_methods(agent_card: AgentCard) -> Self {
        let agent_id = agent_card.name.clone();
        debug!(
            "Creating A2A HTTP server with standard methods for agent: {}",
            agent_id
        );

        let mut protocol = A2AProtocol::new(agent_card);

        let storage: Arc<dyn TaskStorage> = Arc::new(InMemoryTaskStorage::new());
        debug!(
            "Initialized in-memory task storage for agent: {} storage_ptr={:p}",
            agent_id,
            Arc::as_ptr(&storage)
        );

        protocol.register_a2a_methods(Some(storage.clone()));
        info!("Registered A2A standard methods for agent: {}", agent_id);

        Self {
            protocol,
            app: None,
            app_async: None,
            task_storage: Some(storage),
            #[cfg(feature = "observability")]
            obs: None,
        }
    }

    /// Attach application adapter (SDK) for delegation of selected methods
    pub fn with_app_adapter(mut self, app: std::sync::Arc<dyn a2a_app_ports::A2AAppPort>) -> Self {
        debug!(
            "Attaching sync app adapter for agent: {}",
            self.protocol.agent_card().name
        );
        self.app = Some(app);
        self
    }

    /// Attach async application adapter (SDK) for delegation of selected methods
    pub fn with_app_adapter_async(
        mut self,
        app: std::sync::Arc<dyn a2a_app_ports::A2AAppPortAsync>,
    ) -> Self {
        debug!(
            "Attaching async app adapter for agent: {}",
            self.protocol.agent_card().name
        );
        self.app_async = Some(app);
        self
    }

    /// Attach an observability handle (happy-path facade).
    #[cfg(feature = "observability")]
    pub fn with_observability(mut self, obs: observability::Obs) -> Self {
        info!("observability:attached to a2a_http_server");
        self.obs = Some(obs);
        self
    }

    /// Create HTTP server with custom task storage
    pub fn new_with_storage(agent_card: AgentCard, storage: Arc<dyn TaskStorage>) -> Self {
        let agent_id = agent_card.name.clone();
        debug!(
            "Creating A2A HTTP server with custom storage for agent: {} storage_ptr={:p}",
            agent_id,
            Arc::as_ptr(&storage)
        );

        let mut protocol = A2AProtocol::new(agent_card);
        protocol.register_a2a_methods(Some(storage.clone()));
        info!(
            "Registered A2A methods with custom storage for agent: {}",
            agent_id
        );

        Self {
            protocol,
            app: None,
            app_async: None,
            task_storage: Some(storage),
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

    /// Async variant entry point that can leverage async app port implementations
    pub async fn serve_request_async(&self, req: SpinRequest) -> Result<SpinResponse> {
        let path = req.path().to_string();
        trace!(
            "serve_request_async path={} app_async_present={} app_present={} storage_ptr={}",
            path,
            self.app_async.is_some(),
            self.app.is_some(),
            self.task_storage
                .as_ref()
                .map(|s| format!("{:p}", Arc::as_ptr(s)))
                .unwrap_or_else(|| "<none>".to_string())
        );
        if path == "/jsonrpc" || path == "/" {
            if let Some(app) = &self.app_async {
                #[cfg(feature = "observability")]
                let (mut span_guard, start_time, mut prev_ctx) = {
                    let mut h = std::collections::HashMap::<String, String>::new();
                    for (k, v) in req.headers() {
                        if let Ok(v) = std::str::from_utf8(v.as_bytes()) {
                            h.insert(k.to_string().to_lowercase(), v.to_string());
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

                    let pf_source = observability::target_id_from_peer(peer);
                    let pf_target = observability::workload_id(
                        &observability::cluster_name(),
                        &observability::current_namespace(),
                        &observability::current_service_name(),
                    );

                    let span_guard = self.obs.as_ref().and_then(|obs| {
                        let attrs: [(&str, &str); 8] = [
                            (attr::COMPONENT, "a2a_server"),
                            (attr::OPERATION, "<pending>"),
                            (attr::PEER_SERVICE, peer),
                            (attr::RPC_SYSTEM, "jsonrpc"),
                            (attr::PF_KIND, "a2a"),
                            (attr::PF_SOURCE_WORKLOAD, pf_source.as_str()),
                            (attr::PF_TARGET_WORKLOAD, pf_target.as_str()),
                            (attr::RPC_METHOD, "<pending>"),
                        ];

                        if let Some(otel) = obs.otel_plugin() {
                            Some(otel.start_span_with_w3c_context(
                                span::A2A_SERVER,
                                &parent,
                                &attrs,
                            ))
                        } else {
                            Some(obs.span(span::A2A_SERVER, &attrs))
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

                if req.method() != &Method::Post {
                    return Ok(SpinResponse::builder()
                        .status(405)
                        .header("content-type", "application/json")
                        .header("allow", "POST")
                        .body(serde_json::json!({
                            "jsonrpc": JSONRPC_VERSION,
                            "error": {"code": -32600, "message": "Method not allowed. Use POST for JSON-RPC requests."},
                            "id": null
                        }).to_string())
                        .build());
                }
                let request_str = std::str::from_utf8(req.body())?;
                trace!(
                    "serve_request_async raw_json={} bytes={}",
                    request_str,
                    req.body().len()
                );
                let root_val: serde_json::Value = serde_json::from_str(request_str)?;
                if let Some(m) = root_val.get("method").and_then(|m| m.as_str()) {
                    trace!("serve_request_async parsed method={}", m);
                    #[cfg(feature = "observability")]
                    if let Some(g) = &span_guard {
                        g.add_attribute(attr::OPERATION, m);
                        g.add_attribute(attr::RPC_METHOD, m);
                    }
                    if m == crate::method::SEND_MESSAGE {
                        let params_val = root_val
                            .get("params")
                            .cloned()
                            .unwrap_or(serde_json::Value::Null);
                        let params =
                            a2a_protocol_core::methods::params::MessageSendParams::from_json(
                                params_val,
                            )?;
                        let id = root_val
                            .get("id")
                            .cloned()
                            .unwrap_or(serde_json::Value::Null);
                        let response_future = app.handle_send_message_async(params);
                        #[cfg(feature = "observability")]
                        let response_result = if let Some(current_context) = get_current_context() {
                            with_context_future(current_context, response_future).await
                        } else {
                            response_future.await
                        };
                        #[cfg(not(feature = "observability"))]
                        let response_result = response_future.await;
                        let response = match response_result {
                            Ok(result) => {
                                let result_value =
                                    if let a2a_protocol_core::methods::params::MessageSendResponse::Task(
                                        task,
                                    ) = &result
                                    {
                                        if let Some(storage) = &self.task_storage {
                                            if let Ok(tasks) = storage.list_tasks() {
                                                debug!("adapter.store_task (before) count={}", tasks.len());
                                            }
                                            let _ = storage.store_task(task.clone());
                                            if let Ok(tasks) = storage.list_tasks() {
                                                debug!(
                                                    "adapter.store_task (after) count={} stored_task_id={}",
                                                    tasks.len(),
                                                    task.id
                                                );
                                            }
                                        }
                                        serde_json::to_value(result)?
                                    } else {
                                        serde_json::to_value(result)?
                                    };
                                JsonRpcResponse::success(id, result_value)
                            }
                            Err(e) => {
                                #[cfg(feature = "observability")]
                                {
                                    if let Some(obs) = &self.obs {
                                        let duration_ms =
                                            start_time.elapsed().as_secs_f64() * 1000.0;
                                        let status = value::STATUS_ERROR;
                                        if let Some(g) = &span_guard {
                                            g.add_attribute(attr::STATUS, status);
                                            g.add_attribute(attr::PF_OUTCOME, "error");
                                            g.set_status(SpanStatus::Error);
                                        }
                                        obs.metric(
                                            metric::A2A_REQUESTS_TOTAL,
                                            1.0,
                                            &[
                                                (attr::COMPONENT, "a2a_server"),
                                                (attr::OPERATION, m),
                                                (attr::STATUS, status),
                                            ],
                                        );
                                        obs.metric(
                                            metric::A2A_LATENCY_MS,
                                            duration_ms,
                                            &[
                                                (attr::COMPONENT, "a2a_server"),
                                                (attr::OPERATION, m),
                                                (attr::STATUS, status),
                                            ],
                                        );
                                        drop(span_guard.take());
                                        let _ = obs.maybe_flush();
                                        match prev_ctx.take() {
                                            Some(ctx) => set_current_context(ctx),
                                            None => clear_current_context(),
                                        }
                                    }
                                }
                                let jsonrpc_error = e.to_jsonrpc_error();
                                JsonRpcResponse::error(
                                    id,
                                    jsonrpc_error.code,
                                    jsonrpc_error.message,
                                )
                            }
                        };
                        let body = serde_json::to_string(&response)?;

                        #[cfg(feature = "observability")]
                        {
                            if let Some(obs) = &self.obs {
                                let duration_ms = start_time.elapsed().as_secs_f64() * 1000.0;
                                let status = STATUS_OK;
                                if let Some(g) = &span_guard {
                                    g.add_attribute(attr::STATUS, status);
                                    g.add_attribute(attr::PF_OUTCOME, "ok");
                                    g.set_status(SpanStatus::Ok);
                                }
                                obs.metric(
                                    metric::A2A_REQUESTS_TOTAL,
                                    1.0,
                                    &[
                                        (attr::COMPONENT, "a2a_server"),
                                        (attr::OPERATION, m),
                                        (attr::STATUS, status),
                                    ],
                                );
                                obs.metric(
                                    metric::A2A_LATENCY_MS,
                                    duration_ms,
                                    &[
                                        (attr::COMPONENT, "a2a_server"),
                                        (attr::OPERATION, m),
                                        (attr::STATUS, status),
                                    ],
                                );
                                drop(span_guard.take());
                                let _ = obs.maybe_flush();
                                match prev_ctx.take() {
                                    Some(ctx) => set_current_context(ctx),
                                    None => clear_current_context(),
                                }
                            }
                        }

                        return Ok(SpinResponse::builder()
                            .status(200)
                            .header("content-type", "application/json")
                            .header("a2a-version", A2A_PROTOCOL_VERSION)
                            .header("x-server", "a2a-http-server")
                            .body(body)
                            .build());
                    } else if m == crate::method::GET_AGENT_CARD {
                        let card = app.build_agent_card();
                        let id = root_val
                            .get("id")
                            .cloned()
                            .unwrap_or(serde_json::Value::Null);
                        let response = JsonRpcResponse::success(id, serde_json::to_value(card)?);
                        let body = serde_json::to_string(&response)?;

                        #[cfg(feature = "observability")]
                        {
                            if let Some(obs) = &self.obs {
                                let duration_ms = start_time.elapsed().as_secs_f64() * 1000.0;
                                let status = STATUS_OK;
                                if let Some(g) = &span_guard {
                                    g.add_attribute(attr::STATUS, status);
                                    g.add_attribute(attr::PF_OUTCOME, "ok");
                                    g.set_status(SpanStatus::Ok);
                                }
                                obs.metric(
                                    metric::A2A_REQUESTS_TOTAL,
                                    1.0,
                                    &[
                                        (attr::COMPONENT, "a2a_server"),
                                        (attr::OPERATION, m),
                                        (attr::STATUS, status),
                                    ],
                                );
                                obs.metric(
                                    metric::A2A_LATENCY_MS,
                                    duration_ms,
                                    &[
                                        (attr::COMPONENT, "a2a_server"),
                                        (attr::OPERATION, m),
                                        (attr::STATUS, status),
                                    ],
                                );
                                drop(span_guard.take());
                                let _ = obs.maybe_flush();
                                match prev_ctx.take() {
                                    Some(ctx) => set_current_context(ctx),
                                    None => clear_current_context(),
                                }
                            }
                        }

                        return Ok(SpinResponse::builder()
                            .status(200)
                            .header("content-type", "application/json")
                            .header("a2a-version", A2A_PROTOCOL_VERSION)
                            .header("x-server", "a2a-http-server")
                            .body(body)
                            .build());
                    }
                }
            }
        }
        trace!("serve_request_async falling back to sync serve_request path");
        self.serve_request(req)
    }

    /// **MAIN ENTRY POINT**: Serve HTTP requests with A2A protocol compliance
    pub fn serve_request(&self, req: SpinRequest) -> Result<SpinResponse> {
        let path = req.path().to_string();
        let method = req.method().to_string();
        let agent_id = self.agent_id();

        debug!(
            "Incoming request: {} {} for agent: {} (app_present={}, app_async_present={}, storage_ptr={})",
            method,
            path,
            agent_id,
            self.app.is_some(),
            self.app_async.is_some(),
            self.task_storage
                .as_ref()
                .map(|s| format!("{:p}", Arc::as_ptr(s)))
                .unwrap_or_else(|| "<none>".to_string())
        );
        trace!("Request headers: <omitted>");

        let result = match path.as_str() {
            "/jsonrpc" | "/" => {
                debug!("Routing to JSON-RPC endpoint for agent: {}", agent_id);
                self.serve_jsonrpc(req)
            }

            "/.well-known/agent-card.json" | "/v1/agent/card:get" => {
                debug!(
                    "Routing to agent card alias endpoint for agent: {}",
                    agent_id
                );
                self.serve_agent_card()
            }
            "/health" => {
                debug!("Routing to health endpoint for agent: {}", agent_id);
                self.serve_health()
            }
            _ => {
                warn!("Unknown path requested: {} for agent: {}", path, agent_id);
                self.serve_not_found(&path)
            }
        };

        match &result {
            Ok(response) => {
                info!(
                    "Request completed: {} {} -> {} for agent: {}",
                    method,
                    path,
                    response.status(),
                    agent_id
                );
            }
            Err(e) => {
                error!(
                    "Request failed: {} {} -> {} for agent: {}",
                    method, path, e, agent_id
                );
            }
        }

        result
    }

    /// **JSON-RPC ENDPOINT**: Main protocol endpoint
    fn serve_jsonrpc(&self, req: SpinRequest) -> Result<SpinResponse> {
        let agent_id = self.agent_id();

        if req.method() != &Method::Post {
            warn!(
                "Invalid HTTP method for JSON-RPC: {} (expected POST) for agent: {}",
                req.method(),
                agent_id
            );
            return Ok(SpinResponse::builder()
                .status(405)
                .header("content-type", "application/json")
                .header("allow", "POST")
                .body(
                    json!({
                        "jsonrpc": JSONRPC_VERSION,
                        "error": {
                            "code": -32600,
                            "message": "Method not allowed. Use POST for JSON-RPC requests."
                        },
                        "id": null
                    })
                    .to_string(),
                )
                .build());
        }

        let request_str = std::str::from_utf8(req.body())?;
        info!(
            "jsonrpc_request_received agent={} method={} path={} bytes={}",
            agent_id,
            req.method(),
            req.path(),
            request_str.len()
        );
        if request_str.is_empty() {
            error!("Empty request body for JSON-RPC at agent: {}", agent_id);
            return Ok(SpinResponse::builder()
                .status(400)
                .header("content-type", "application/json")
                .body(
                    serde_json::json!({
                        "jsonrpc": JSONRPC_VERSION,
                        "error": {"code": -32700, "message": "Empty request body"},
                        "id": null
                    })
                    .to_string(),
                )
                .build());
        }
        debug!(
            "Parsing JSON-RPC request for agent: {} (size: {} bytes) app_present={} app_async_present={}",
            agent_id,
            request_str.len(),
            self.app.is_some(),
            self.app_async.is_some()
        );
        trace!("JSON-RPC request body: {}", request_str);

        let root_val: serde_json::Value = match serde_json::from_str(request_str) {
            Ok(v) => v,
            Err(e) => {
                error!(
                    "Failed to parse JSON-RPC request for agent: {} - {}",
                    agent_id, e
                );
                return Err(e.into());
            }
        };
        let method_name = root_val
            .get("method")
            .and_then(|m| m.as_str())
            .unwrap_or("<none>")
            .to_string();
        trace!("JSON-RPC method parsed: {}", method_name);

        #[cfg(feature = "observability")]
        let (mut span_guard, start_time, prev_ctx) = {
            let mut h = std::collections::HashMap::<String, String>::new();
            for (k, v) in req.headers() {
                if let Ok(v) = std::str::from_utf8(v.as_bytes()) {
                    h.insert(k.to_string().to_lowercase(), v.to_string());
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
                            (attr::OPERATION, method_name.as_str()),
                            (attr::PEER_SERVICE, peer),
                        ],
                    ))
                } else {
                    Some(obs.span(
                        span::A2A_SERVER,
                        &[
                            (attr::COMPONENT, "a2a_server"),
                            (attr::OPERATION, method_name.as_str()),
                            (attr::PEER_SERVICE, peer),
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

        let mut delegated_response: Option<(SpinResponse, &'static str)> = None;
        if let Some(app) = &self.app {
            if let Some(method) = root_val.get("method").and_then(|m| m.as_str()) {
                debug!(
                    "Sync app adapter present, considering delegation for method={}",
                    method
                );
                if method == crate::method::SEND_MESSAGE {
                    let params_val = root_val
                        .get("params")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null);
                    let params = a2a_protocol_core::methods::params::MessageSendParams::from_json(
                        params_val,
                    )?;
                    let id = root_val
                        .get("id")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null);
                    let response = match app.handle_send_message(params) {
                        Ok(result) => JsonRpcResponse::success(id, serde_json::to_value(result)?),
                        Err(err) => {
                            let jsonrpc_error = err.to_jsonrpc_error();
                            JsonRpcResponse::error(id, jsonrpc_error.code, jsonrpc_error.message)
                        }
                    };
                    let body = serde_json::to_string(&response)?;
                    debug!("Delegated to sync app adapter for method=SendMessage");
                    delegated_response = Some((
                        SpinResponse::builder()
                            .status(200)
                            .header("content-type", "application/json")
                            .header("a2a-version", A2A_PROTOCOL_VERSION)
                            .header("x-server", "a2a-http-server")
                            .body(body)
                            .build(),
                        STATUS_OK,
                    ));
                } else if method == crate::method::GET_AGENT_CARD {
                    let card = app.build_agent_card();
                    let id = root_val
                        .get("id")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null);
                    let response = JsonRpcResponse::success(id, serde_json::to_value(card)?);
                    let body = serde_json::to_string(&response)?;
                    debug!("Delegated to sync app adapter for method=GetAgentCard");
                    delegated_response = Some((
                        SpinResponse::builder()
                            .status(200)
                            .header("content-type", "application/json")
                            .header("a2a-version", A2A_PROTOCOL_VERSION)
                            .header("x-server", "a2a-http-server")
                            .body(body)
                            .build(),
                        STATUS_OK,
                    ));
                }
            }
        }

        if let Some((resp, delegated_status)) = delegated_response {
            #[cfg(feature = "observability")]
            {
                if let Some(obs) = &self.obs {
                    let duration_ms = start_time.elapsed().as_secs_f64() * 1000.0;
                    let status = delegated_status;

                    if let Some(g) = &span_guard {
                        g.add_attribute(attr::STATUS, status);
                        g.set_status(SpanStatus::Ok);
                    }

                    info!(
                        "observability:emit_metrics method={} status={} otel_plugin={}",
                        method_name,
                        status,
                        obs.otel_plugin().is_some()
                    );
                    obs.metric(
                        metric::A2A_REQUESTS_TOTAL,
                        1.0,
                        &[
                            (attr::COMPONENT, "a2a_server"),
                            (attr::OPERATION, method_name.as_str()),
                            (attr::STATUS, status),
                        ],
                    );
                    obs.metric(
                        metric::A2A_LATENCY_MS,
                        duration_ms,
                        &[
                            (attr::COMPONENT, "a2a_server"),
                            (attr::OPERATION, method_name.as_str()),
                            (attr::STATUS, status),
                        ],
                    );

                    drop(span_guard);

                    if let Err(err) = obs.maybe_flush() {
                        warn!("observability:flush_failed error={}", err);
                    } else {
                        info!("observability:flush_ok method={}", method_name);
                    }

                    match prev_ctx {
                        Some(ctx) => set_current_context(ctx),
                        None => clear_current_context(),
                    }
                }
            }

            return Ok(resp);
        }

        debug!(
            "No sync adapter delegation for method={}, falling back to protocol handler",
            method_name
        );

        let incoming: JsonRpcIncoming = serde_json::from_value(root_val)?;
        let incoming_method = match &incoming {
            JsonRpcIncoming::Request(req) => req.method.clone(),
            JsonRpcIncoming::Notification(notif) => notif.method.clone(),
        };

        debug!(
            "Delegating to A2A protocol instance for agent: {} method: {}",
            agent_id, incoming_method
        );

        let response = match self.protocol.handle_incoming(incoming) {
            Ok(Some(response)) => response,
            Ok(None) => {
                JsonRpcResponse::success(json!(null), json!({"status": "notification processed"}))
            }
            Err(e) => return Err(e.into()),
        };

        let response_body = serde_json::to_string(&response)?;

        #[cfg(feature = "observability")]
        {
            if let Some(obs) = &self.obs {
                let duration_ms = start_time.elapsed().as_secs_f64() * 1000.0;
                let status = if response.error.is_some() {
                    value::STATUS_ERROR
                } else {
                    STATUS_OK
                };

                if let Some(g) = &span_guard {
                    g.add_attribute(attr::STATUS, status);
                    g.set_status(if status == STATUS_OK {
                        SpanStatus::Ok
                    } else {
                        SpanStatus::Error
                    });
                }

                info!(
                    "observability:emit_metrics method={} status={} otel_plugin={}",
                    method_name,
                    status,
                    obs.otel_plugin().is_some()
                );
                obs.metric(
                    metric::A2A_REQUESTS_TOTAL,
                    1.0,
                    &[
                        (attr::COMPONENT, "a2a_server"),
                        (attr::OPERATION, method_name.as_str()),
                        (attr::STATUS, status),
                    ],
                );
                obs.metric(
                    metric::A2A_LATENCY_MS,
                    duration_ms,
                    &[
                        (attr::COMPONENT, "a2a_server"),
                        (attr::OPERATION, method_name.as_str()),
                        (attr::STATUS, status),
                    ],
                );

                drop(span_guard);

                if let Err(err) = obs.maybe_flush() {
                    warn!("observability:flush_failed error={}", err);
                } else {
                    info!("observability:flush_ok method={}", method_name);
                }

                match prev_ctx {
                    Some(ctx) => set_current_context(ctx),
                    None => clear_current_context(),
                }
            }
        }

        debug!(
            "Returning HTTP 200 response for agent: {} method={}",
            agent_id, method_name
        );
        Ok(SpinResponse::builder()
            .status(200)
            .header("content-type", "application/json")
            .header("a2a-version", A2A_PROTOCOL_VERSION)
            .header("x-server", "a2a-http-server")
            .body(response_body)
            .build())
    }

    /// **AGENT CARD ENDPOINT**: A2A protocol discovery
    fn serve_agent_card(&self) -> Result<SpinResponse> {
        let agent_id = self.agent_id();
        debug!("Serving agent card for agent: {}", agent_id);

        let agent_card = self.protocol.agent_card();

        let response_body = match serde_json::to_string(agent_card) {
            Ok(body) => {
                debug!(
                    "Serialized agent card for agent: {} (size: {} bytes)",
                    agent_id,
                    body.len()
                );
                trace!("Agent card: {}", body);
                body
            }
            Err(e) => {
                error!(
                    "Failed to serialize agent card for agent: {} - {}",
                    agent_id, e
                );
                return Err(e.into());
            }
        };

        info!("Agent card served for agent: {}", agent_id);
        Ok(SpinResponse::builder()
            .status(200)
            .header("content-type", "application/json")
            .header("a2a-version", A2A_PROTOCOL_VERSION)
            .body(response_body)
            .build())
    }

    /// **HEALTH ENDPOINT**: Server status
    fn serve_health(&self) -> Result<SpinResponse> {
        let agent_id = self.agent_id();
        debug!("Serving health check for agent: {}", agent_id);

        let health_data = json!({
            "status": "healthy",
            "server": "a2a-http-server",
            "protocol": A2A_PROTOCOL_VERSION,
            "agent": agent_id
        });

        let response_body = match serde_json::to_vec(&health_data) {
            Ok(body) => {
                debug!("Health check completed for agent: {}", agent_id);
                body
            }
            Err(e) => {
                error!(
                    "Failed to serialize health data for agent: {} - {}",
                    agent_id, e
                );
                return Err(e.into());
            }
        };

        Ok(SpinResponse::builder()
            .status(200)
            .header("content-type", "application/json")
            .body(response_body)
            .build())
    }

    /// **NOT FOUND ENDPOINT**: 404 handler
    fn serve_not_found(&self, path: &str) -> Result<SpinResponse> {
        let agent_id = self.agent_id();
        warn!(
            "Serving 404 for unknown path: {} for agent: {}",
            path, agent_id
        );

        let error_response = json!({
            "error": "Not Found",
            "message": format!("Path '{}' not found", path),
            "available_endpoints": ["/jsonrpc", "/health"]
        });

        let response_body = match serde_json::to_string(&error_response) {
            Ok(body) => body,
            Err(e) => {
                error!(
                    "Failed to serialize 404 response for agent: {} - {}",
                    agent_id, e
                );
                return Err(e.into());
            }
        };

        Ok(SpinResponse::builder()
            .status(404)
            .header("content-type", "application/json")
            .body(response_body)
            .build())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wasm_server_creation() {
        let agent_card = AgentCard::new("test-agent".to_string());
        let server = A2AHttpServer::new_with_a2a_methods(agent_card);
        assert_eq!(server.agent_id(), "test-agent");
        assert!(server.can_serve());
    }

    #[test]
    fn test_wasm_server_with_storage() {
        let agent_card = AgentCard::new("test-agent".to_string());
        let storage: Arc<dyn TaskStorage> = Arc::new(InMemoryTaskStorage::new());
        let server = A2AHttpServer::new_with_storage(agent_card, storage);
        assert_eq!(server.agent_id(), "test-agent");
    }

    #[cfg(feature = "observability")]
    #[tokio::test]
    async fn test_wasm_server_preserves_w3c_trace_context_for_app_adapter() {
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

        let req = SpinRequest::builder()
            .method(Method::Post)
            .uri("/jsonrpc")
            .header("content-type", "application/json")
            .header(
                "traceparent",
                "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01",
            )
            .body(
                serde_json::json!({
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
                .to_string(),
            )
            .build();

        let response = server.serve_request_async(req).await.unwrap();
        assert_eq!(*response.status(), 200);

        let ctx = seen.lock().unwrap().clone().expect("trace context");
        assert_eq!(ctx.trace_id, "0af7651916cd43dd8448eb211c80319c");
        assert_eq!(ctx.parent_span_id.as_deref(), Some("b7ad6b7169203331"));
        assert_ne!(ctx.span_id, "b7ad6b7169203331");
        assert_eq!(ctx.span_id.len(), 16);
    }
}
