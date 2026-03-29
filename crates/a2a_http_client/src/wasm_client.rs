//! WASM A2A HTTP Client using Spin SDK

use a2a_protocol_core::{A2A_PROTOCOL_VERSION, data::message::Message, data::task::Task};
use anyhow::Result;
use protocol_transport_core::{
    JSONRPC_VERSION, JsonRpcError, JsonRpcId, JsonRpcRequest, JsonRpcResponse,
};
use serde_json::{Value, json};
use std::collections::HashMap;
use thiserror::Error;

#[cfg(feature = "observability")]
use observability::{
    ObsHandle, SpanStatus, TraceContext, W3CTraceContext, WORKLOAD_PREFIX, attr,
    clear_current_context, cluster_name, current_namespace, current_service_name,
    get_current_context, metric, set_current_context, span, target_id_from_peer, value,
    workload_id,
};

#[cfg(feature = "observability")]
use web_time::Instant;

/// Client errors
#[derive(Error, Debug)]
pub enum ClientError {
    #[error("Network error: {0}")]
    Network(String),
    #[error("Serialization error: {0}")]
    Serialization(String),
    #[error("JSON-RPC error: {0}")]
    JsonRpc(String),
    #[error("Validation error: {0}")]
    Validation(String),
}

/// **RPC Error type** - Compatible with core client interface
#[derive(Error, Debug)]
#[error("RPC Error [{code}]: {message}")]
pub struct RpcError {
    pub code: i32,
    pub message: String,
}

impl RpcError {
    pub fn internal_error(message: &str) -> Self {
        Self {
            code: -32603,
            message: message.to_string(),
        }
    }
}

/// **External A2A Client** - WASM implementation using Spin SDK
pub struct Client {
    url: String,
    headers: HashMap<String, String>,
    #[cfg(feature = "observability")]
    obs: Option<observability::Obs>,
}

impl Client {
    /// Create client for external agent (matches core Client::internal())
    pub fn external(url: impl Into<String>) -> Self {
        let mut headers = HashMap::new();
        headers.insert("content-type".to_string(), "application/json".to_string());
        headers.insert("a2a-version".to_string(), A2A_PROTOCOL_VERSION.to_string());

        Self {
            url: url.into(),
            headers,
            #[cfg(feature = "observability")]
            obs: None,
        }
    }

    /// Add custom header
    pub fn with_header(mut self, key: String, value: String) -> Self {
        self.headers.insert(key, value);
        self
    }

    /// Attach an observability handle (happy-path facade).
    #[cfg(feature = "observability")]
    pub fn with_observability(mut self, obs: observability::Obs) -> Self {
        self.obs = Some(obs);
        self
    }

    /// Get URL (for testing)
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Check if header exists (for testing)
    pub fn has_header(&self, key: &str) -> bool {
        self.headers.contains_key(key)
    }

    /// **UNIVERSAL CALL** - Same interface as core Client::call()
    pub async fn call(&self, method: &str, params: Value) -> Result<Value, RpcError> {
        #[cfg(feature = "observability")]
        let (span_guard, start_time, prev_ctx, injected, peer, target_id, source_id) = {
            let prev = get_current_context();

            let parent = prev
                .clone()
                .map(|ctx| W3CTraceContext {
                    trace_id: ctx.trace_id.clone(),
                    parent_id: ctx.span_id.clone(),
                    trace_flags: if ctx.sampled {
                        "01".to_string()
                    } else {
                        "00".to_string()
                    },
                    trace_state: None,
                })
                .unwrap_or_else(W3CTraceContext::new_root);

            let peer = peer_service_from_url(&self.url);
            let target_id = target_id_from_peer(&peer);
            let source_id = workload_id(
                &cluster_name(),
                &current_namespace(),
                &current_service_name(),
            );
            let kind = if target_id.starts_with(WORKLOAD_PREFIX) {
                value::KIND_A2A
            } else {
                value::KIND_EXTERNAL
            };

            let span_guard = self.obs.as_ref().and_then(|obs| {
                if let Some(otel) = obs.otel_plugin() {
                    Some(otel.start_span_with_w3c_context(
                        span::A2A_CLIENT,
                        &parent,
                        &[
                            (attr::COMPONENT, "a2a_client"),
                            (attr::OPERATION, method),
                            (attr::PEER_SERVICE, peer.as_str()),
                            (attr::RPC_SYSTEM, value::RPC_SYSTEM_JSONRPC),
                            (attr::RPC_METHOD, method),
                            (attr::PF_SOURCE_WORKLOAD, source_id.as_str()),
                            (attr::PF_TARGET_WORKLOAD, target_id.as_str()),
                            (attr::PF_KIND, kind),
                        ],
                    ))
                } else {
                    Some(obs.span(
                        span::A2A_CLIENT,
                        &[
                            (attr::COMPONENT, "a2a_client"),
                            (attr::OPERATION, method),
                            (attr::PEER_SERVICE, peer.as_str()),
                            (attr::RPC_SYSTEM, value::RPC_SYSTEM_JSONRPC),
                            (attr::RPC_METHOD, method),
                            (attr::PF_SOURCE_WORKLOAD, source_id.as_str()),
                            (attr::PF_TARGET_WORKLOAD, target_id.as_str()),
                            (attr::PF_KIND, kind),
                        ],
                    ))
                }
            });

            let injected = span_guard.as_ref().map(|g| {
                let child = W3CTraceContext {
                    trace_id: parent.trace_id.clone(),
                    parent_id: g.span_id().to_string(),
                    trace_flags: parent.trace_flags.clone(),
                    trace_state: None,
                };
                let mut out = HashMap::<String, String>::new();
                observability::Obs::inject_context(&mut out, &child);
                out
            });

            if let Some(g) = &span_guard {
                set_current_context(TraceContext {
                    trace_id: parent.trace_id.clone(),
                    span_id: g.span_id().to_string(),
                    parent_span_id: Some(parent.parent_id.clone()),
                    sampled: parent.is_sampled(),
                });
            }

            (
                span_guard,
                Instant::now(),
                prev,
                injected,
                peer,
                target_id,
                source_id,
            )
        };

        let request = JsonRpcRequest {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: json!(uuid::Uuid::new_v4().to_string()),
            method: method.to_string(),
            params,
        };

        let injected_headers: Option<&HashMap<String, String>> = {
            #[cfg(feature = "observability")]
            {
                injected.as_ref()
            }
            #[cfg(not(feature = "observability"))]
            {
                None
            }
        };

        let out: Result<Value, RpcError> = async {
            let response = self
                .send_request(request, injected_headers)
                .await
                .map_err(|e| RpcError::internal_error(&e.to_string()))?;

            // Check for JSON-RPC errors
            if let Some(error) = response.error {
                return Err(RpcError {
                    code: error.code as i32,
                    message: error.message,
                });
            }

            response
                .result
                .ok_or_else(|| RpcError::internal_error("Invalid response: no result or error"))
        }
        .await;

        #[cfg(feature = "observability")]
        {
            if let Some(obs) = &self.obs {
                let duration_ms = start_time.elapsed().as_secs_f64() * 1000.0;
                let status = if out.is_ok() {
                    value::STATUS_OK
                } else {
                    value::STATUS_ERROR
                };
                let outcome = if out.is_ok() {
                    value::OUTCOME_OK
                } else {
                    value::OUTCOME_ERROR
                };

                if let Some(g) = &span_guard {
                    g.add_attribute(attr::STATUS, status);
                    g.add_attribute(attr::PF_OUTCOME, outcome);
                    g.add_attribute(attr::PEER_SERVICE, peer.as_str());
                    g.add_attribute(attr::PF_SOURCE_WORKLOAD, source_id.as_str());
                    g.add_attribute(attr::PF_TARGET_WORKLOAD, target_id.as_str());
                    g.add_attribute(attr::RPC_SYSTEM, value::RPC_SYSTEM_JSONRPC);
                    g.add_attribute(attr::RPC_METHOD, method);
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
                        (attr::COMPONENT, "a2a_client"),
                        (attr::OPERATION, method),
                        (attr::STATUS, status),
                    ],
                );
                obs.metric(
                    metric::A2A_LATENCY_MS,
                    duration_ms,
                    &[
                        (attr::COMPONENT, "a2a_client"),
                        (attr::OPERATION, method),
                        (attr::STATUS, status),
                    ],
                );

                match prev_ctx {
                    Some(ctx) => set_current_context(ctx),
                    None => clear_current_context(),
                }
            }
        }

        out
    }

    // ========================================================================
    // STANDARD METHODS
    // ========================================================================

    pub async fn ping(&self) -> Result<Value, RpcError> {
        self.call("Ping", Value::Null).await
    }

    pub async fn get_agent_card(&self) -> Result<Value, RpcError> {
        self.call("GetAgentCard", Value::Null).await
    }

    /// Backward-compat alias for `get_agent_card`.
    pub async fn metadata(&self) -> Result<Value, RpcError> {
        self.get_agent_card().await
    }

    pub async fn run(&self, input: Value) -> Result<Value, RpcError> {
        self.call("run", input).await
    }

    // ========================================================================
    // A2A v1.0 STANDARD METHODS
    // ========================================================================

    /// **SendMessage** — Send message using A2A v1.0 protocol.
    pub async fn message_send(
        &self,
        message: Message,
        metadata: Option<HashMap<String, Value>>,
    ) -> Result<Value, RpcError> {
        let params = json!({
            "message": message,
            "metadata": metadata
        });

        self.call("SendMessage", params).await
    }

    /// **GetTask** — Retrieve task state and artifacts.
    pub async fn task_get(&self, task_id: String) -> Result<Task, RpcError> {
        let params = json!({
            "id": task_id,
        });

        let result = self.call("GetTask", params).await?;
        serde_json::from_value(result)
            .map_err(|e| RpcError::internal_error(&format!("Failed to parse task: {}", e)))
    }

    /// **CancelTask** — Cancel an ongoing task.
    pub async fn task_cancel(&self, task_id: String) -> Result<Task, RpcError> {
        let params = json!({
            "id": task_id,
        });

        let result = self.call("CancelTask", params).await?;
        serde_json::from_value(result)
            .map_err(|e| RpcError::internal_error(&format!("Failed to parse task: {}", e)))
    }

    /// **ListTasks** — List agent tasks with filtering.
    pub async fn task_list(
        &self,
        context_id: Option<String>,
        status: Option<String>,
        page_size: Option<u32>,
        page_token: Option<String>,
    ) -> Result<Value, RpcError> {
        let params = json!({
            "contextId": context_id,
            "status": status,
            "pageSize": page_size,
            "pageToken": page_token
        });

        self.call("ListTasks", params).await
    }

    /// **GetExtendedAgentCard** — Get extended agent information.
    pub async fn get_extended_agent_card(
        &self,
        auth_token: Option<String>,
        scope: Option<Vec<String>>,
        metadata: Option<HashMap<String, Value>>,
    ) -> Result<Value, RpcError> {
        let params = json!({
            "auth_token": auth_token,
            "scope": scope,
            "metadata": metadata
        });

        self.call("GetExtendedAgentCard", params).await
    }

    // ========================================================================
    // HTTP SPECIFIC METHODS
    // ========================================================================

    /// **AGENT CARD**: Get agent discovery info (HTTP GET, not JSON-RPC)
    pub async fn agent_card(&self) -> Result<String, ClientError> {
        use spin_sdk::http::{Method, Request as SpinRequest, Response as SpinResponse};

        let agent_url = self.url.replace("/jsonrpc", "/agent");

        let spin_request = SpinRequest::builder()
            .method(Method::Get)
            .uri(&agent_url)
            .build();

        let spin_response: SpinResponse = spin_sdk::http::send(spin_request)
            .await
            .map_err(|e| ClientError::Network(format!("Agent card request failed: {}", e)))?;

        if *spin_response.status() != 200 {
            return Err(ClientError::Network(format!(
                "Agent card HTTP error: {}",
                spin_response.status()
            )));
        }

        String::from_utf8(spin_response.body().to_vec())
            .map_err(|e| ClientError::Network(format!("Invalid UTF-8 agent card: {}", e)))
    }

    /// **INTERNAL**: Send JSON-RPC request using Spin SDK
    async fn send_request(
        &self,
        request: JsonRpcRequest,
        injected: Option<&HashMap<String, String>>,
    ) -> Result<JsonRpcResponse, ClientError> {
        let request_body = serde_json::to_string(&request)
            .map_err(|e| ClientError::Serialization(e.to_string()))?;

        let response_body = self.send_http_request(&request_body, injected).await?;

        let response: JsonRpcResponse = serde_json::from_str(&response_body)
            .map_err(|e| ClientError::Serialization(e.to_string()))?;

        Ok(response)
    }

    /// **SPIN SDK HTTP**: WASM-native HTTP implementation
    async fn send_http_request(
        &self,
        body: &str,
        _injected: Option<&HashMap<String, String>>,
    ) -> Result<String, ClientError> {
        use spin_sdk::http::{Method, Request as SpinRequest, Response as SpinResponse};

        let mut builder = SpinRequest::builder();
        builder.method(Method::Post);
        builder.uri(&self.url);
        builder.header("content-type", "application/json");
        builder.header("a2a-version", A2A_PROTOCOL_VERSION);

        // Add custom headers
        for (k, v) in &self.headers {
            builder.header(k, v);
        }

        #[cfg(feature = "observability")]
        if let Some(h) = _injected {
            for (k, v) in h {
                builder.header(k, v);
            }
        }

        builder.body(body.to_string());

        let spin_response: SpinResponse = spin_sdk::http::send(builder)
            .await
            .map_err(|e| ClientError::Network(format!("Spin HTTP error: {}", e)))?;

        // Check HTTP status
        if *spin_response.status() != 200 {
            return Err(ClientError::Network(format!(
                "HTTP error: {} - {}",
                spin_response.status(),
                String::from_utf8_lossy(spin_response.body())
            )));
        }

        String::from_utf8(spin_response.body().to_vec())
            .map_err(|e| ClientError::Network(format!("Invalid UTF-8 response: {}", e)))
    }
}

#[cfg(feature = "observability")]
fn peer_service_from_url(url: &str) -> String {
    let without_scheme = url.split("://").nth(1).unwrap_or(url);
    let host_port = without_scheme.split('/').next().unwrap_or(without_scheme);
    if host_port.is_empty() {
        "unknown".to_string()
    } else {
        host_port.to_string()
    }
}

/// **CONVENIENCE**: Check if target is reachable
pub async fn check_connectivity(url: &str) -> bool {
    let client = Client::external(url);
    matches!(client.ping().await, Ok(_))
}
