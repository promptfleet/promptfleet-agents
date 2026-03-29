//! Native A2A HTTP Client using Reqwest

use a2a_protocol_core::{A2A_PROTOCOL_VERSION, data::message::Message, data::task::Task};
use anyhow::Result;
use protocol_transport_core::{
    JSONRPC_VERSION, JsonRpcRequest, JsonRpcResponse, RPC_REQUEST_TIMEOUT, StreamingPolicy,
};
use reqwest;
use serde_json::{Value, json};
use std::collections::HashMap;
use thiserror::Error;
#[cfg(feature = "streaming")]
use {
    a2a_protocol_core::methods::params::SendMessageRequest,
    a2a_protocol_core::streaming::{
        StreamResponse, TaskArtifactUpdateEvent, TaskStatusUpdateEvent,
    },
    futures_util::{Stream, StreamExt},
    protocol_transport_core::IdleTimeoutStream,
    std::pin::Pin,
};

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

/// **External A2A Client** - Native implementation using Reqwest
///
/// Built streaming-first: the internal `reqwest::Client` has only a
/// `connect_timeout` — no total request timeout. RPC (non-streaming) calls
/// add a per-request `.timeout()`. Streaming calls use `first_byte_ms` +
/// `IdleTimeoutStream` instead of wall-clock limits.
pub struct Client {
    url: String,
    headers: HashMap<String, String>,
    http_client: reqwest::Client,
    #[cfg(feature = "streaming")]
    streaming_policy: StreamingPolicy,
    #[cfg(feature = "observability")]
    obs: Option<observability::Obs>,
}

impl Client {
    /// Create client for external agent with default streaming policy.
    pub fn external(url: impl Into<String>) -> Self {
        Self::external_with_policy(url, StreamingPolicy::default())
    }

    /// Create client for external agent with explicit streaming policy.
    pub fn external_with_policy(url: impl Into<String>, policy: StreamingPolicy) -> Self {
        let mut headers = HashMap::new();
        headers.insert("content-type".to_string(), "application/json".to_string());
        headers.insert("a2a-version".to_string(), A2A_PROTOCOL_VERSION.to_string());

        let http_client = reqwest::Client::builder()
            .connect_timeout(policy.connect_timeout())
            .build()
            .expect("failed to build reqwest client");

        Self {
            url: url.into(),
            headers,
            http_client,
            #[cfg(feature = "streaming")]
            streaming_policy: policy,
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

            // Inject the current client span as traceparent for downstream.
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

                // Restore previous context (avoid leaking across calls).
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

    /// **SendStreamingMessage** — Stream A2A v1.0 SSE events.
    #[cfg(feature = "streaming")]
    pub async fn send_subscribe(
        &self,
        context_id: &str,
        mut message: Message,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamResponse, RpcError>> + Send>>, RpcError>
    {
        message.context_id = Some(context_id.to_string());

        let params = SendMessageRequest {
            message,
            tenant: None,
            configuration: None,
            metadata: None,
        };
        let request = JsonRpcRequest {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: json!(uuid::Uuid::new_v4().to_string()),
            method: "SendStreamingMessage".to_string(),
            params: serde_json::to_value(params).map_err(|e| {
                RpcError::internal_error(&format!("serialize params failed: {}", e))
            })?,
        };

        let request_body = serde_json::to_string(&request)
            .map_err(|e| RpcError::internal_error(&format!("serialize request failed: {}", e)))?;

        let mut request_builder = self
            .http_client
            .post(&self.url)
            .header("content-type", "application/json")
            .header("accept", "text/event-stream")
            .header("cache-control", "no-cache")
            .header("a2a-version", A2A_PROTOCOL_VERSION)
            .body(request_body);

        for (key, value) in &self.headers {
            request_builder = request_builder.header(key, value);
        }

        let response = request_builder.send().await.map_err(|e| RpcError {
            code: -32000,
            message: format!("SSE request failed: {}", e),
        })?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(RpcError {
                code: -32000,
                message: format!("SSE HTTP error: {} - {}", status, body),
            });
        }

        let idle_timeout = self.streaming_policy.idle_timeout();
        let bytes_stream = response.bytes_stream();
        let idle_stream = IdleTimeoutStream::new(bytes_stream, idle_timeout);

        let stream = async_stream::try_stream! {
            let mut chunks = Box::pin(idle_stream);
            let mut buffer = String::new();
            let mut event_name: Option<String> = None;
            let mut data_lines: Vec<String> = Vec::new();

            while let Some(chunk) = chunks.next().await {
                let chunk = chunk.map_err(|e| RpcError {
                    code: -32000,
                    message: format!("SSE read error: {}", e),
                })?;
                buffer.push_str(&String::from_utf8_lossy(&chunk));

                while let Some(newline_idx) = buffer.find('\n') {
                    let mut line = buffer[..newline_idx].to_string();
                    buffer.drain(..=newline_idx);
                    if line.ends_with('\r') {
                        line.pop();
                    }

                    if line.is_empty() {
                        if !data_lines.is_empty() {
                            let data = data_lines.join("\n");
                            if let Some(event) = parse_stream_response(event_name.as_deref(), &data)? {
                                let is_terminal = is_terminal_stream_response(&event);
                                yield event;
                                if is_terminal {
                                    return;
                                }
                            }
                            data_lines.clear();
                        }
                        event_name = None;
                        continue;
                    }

                    if let Some(rest) = line.strip_prefix("event:") {
                        event_name = Some(rest.trim().to_string());
                        continue;
                    }
                    if let Some(rest) = line.strip_prefix("data:") {
                        data_lines.push(rest.trim_start().to_string());
                    }
                }
            }

            if !data_lines.is_empty() {
                let data = data_lines.join("\n");
                if let Some(event) = parse_stream_response(event_name.as_deref(), &data)? {
                    yield event;
                }
            }
        };

        Ok(Box::pin(stream))
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
        let agent_url = self.url.replace("/jsonrpc", "/agent");

        let response = self
            .http_client
            .get(&agent_url)
            .send()
            .await
            .map_err(|e| ClientError::Network(format!("Agent card request failed: {}", e)))?;

        if !response.status().is_success() {
            return Err(ClientError::Network(format!(
                "Agent card HTTP error: {}",
                response.status()
            )));
        }

        response
            .text()
            .await
            .map_err(|e| ClientError::Network(format!("Failed to read agent card response: {}", e)))
    }

    /// **INTERNAL**: Send JSON-RPC request using Reqwest
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

    /// **REQWEST HTTP**: Native HTTP implementation with per-request RPC timeout.
    async fn send_http_request(
        &self,
        body: &str,
        _injected: Option<&HashMap<String, String>>,
    ) -> Result<String, ClientError> {
        let mut request_builder = self
            .http_client
            .post(&self.url)
            .timeout(RPC_REQUEST_TIMEOUT)
            .header("content-type", "application/json")
            .header("a2a-version", A2A_PROTOCOL_VERSION)
            .body(body.to_string());

        // Add custom headers
        for (key, value) in &self.headers {
            request_builder = request_builder.header(key, value);
        }

        #[cfg(feature = "observability")]
        if let Some(h) = _injected {
            for (k, v) in h {
                request_builder = request_builder.header(k, v);
            }
        }

        let response = request_builder
            .send()
            .await
            .map_err(|e| ClientError::Network(format!("Reqwest HTTP error: {}", e)))?;

        // Check HTTP status
        if !response.status().is_success() {
            let status = response.status();
            let error_body = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(ClientError::Network(format!(
                "HTTP error: {} - {}",
                status, error_body
            )));
        }

        response
            .text()
            .await
            .map_err(|e| ClientError::Network(format!("Failed to read response: {}", e)))
    }
}

/// Parse a v1.0 SSE event from the JSON-RPC envelope.
///
/// The v1.0 wire format wraps each event in `{ "jsonrpc": "2.0", "id": ..., "result": { <key>: <payload> } }`
/// where `<key>` is one of: `statusUpdate`, `artifactUpdate`, `task`, `message`.
#[cfg(feature = "streaming")]
fn parse_stream_response(
    _event_name: Option<&str>,
    data: &str,
) -> Result<Option<StreamResponse>, RpcError> {
    if data.trim().is_empty() {
        return Ok(None);
    }

    let payload: Value = serde_json::from_str(data).map_err(|e| RpcError {
        code: -32000,
        message: format!("Invalid SSE JSON payload: {}", e),
    })?;
    let id = payload.get("id").cloned().unwrap_or(Value::Null);
    let result = payload.get("result").ok_or_else(|| RpcError {
        code: -32000,
        message: "Invalid SSE payload: missing result".to_string(),
    })?;

    if let Some(status_val) = result.get("statusUpdate") {
        let task_id = status_val
            .get("taskId")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let context_id = status_val
            .get("contextId")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let status: a2a_protocol_core::data::TaskStatus =
            serde_json::from_value(status_val.get("status").cloned().ok_or_else(|| RpcError {
                code: -32000,
                message: "Invalid statusUpdate SSE payload: missing status".to_string(),
            })?)
            .map_err(|e| RpcError {
                code: -32000,
                message: format!("Invalid TaskStatus payload: {}", e),
            })?;

        return Ok(Some(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
            id,
            task_id,
            context_id,
            status,
        })));
    }

    if let Some(artifact_val) = result.get("artifactUpdate") {
        let task_id = artifact_val
            .get("taskId")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let context_id = artifact_val
            .get("contextId")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let artifact: a2a_protocol_core::data::Artifact =
            serde_json::from_value(artifact_val.get("artifact").cloned().ok_or_else(|| {
                RpcError {
                    code: -32000,
                    message: "Invalid artifactUpdate SSE payload: missing artifact".to_string(),
                }
            })?)
            .map_err(|e| RpcError {
                code: -32000,
                message: format!("Invalid artifact payload: {}", e),
            })?;
        let append = artifact_val.get("append").and_then(Value::as_bool);
        let last_chunk = artifact_val.get("lastChunk").and_then(Value::as_bool);

        return Ok(Some(StreamResponse::ArtifactUpdate(
            TaskArtifactUpdateEvent {
                id,
                task_id,
                context_id,
                artifact,
                append,
                last_chunk,
            },
        )));
    }

    if let Some(task_val) = result.get("task") {
        let task: a2a_protocol_core::data::task::Task = serde_json::from_value(task_val.clone())
            .map_err(|e| RpcError {
                code: -32000,
                message: format!("Invalid task payload: {}", e),
            })?;
        return Ok(Some(StreamResponse::Task(task)));
    }

    if let Some(msg_val) = result.get("message") {
        let msg: a2a_protocol_core::data::message::Message =
            serde_json::from_value(msg_val.clone()).map_err(|e| RpcError {
                code: -32000,
                message: format!("Invalid message payload: {}", e),
            })?;
        return Ok(Some(StreamResponse::Message(msg)));
    }

    Err(RpcError {
        code: -32000,
        message: "Invalid SSE payload: unrecognised result envelope key".to_string(),
    })
}

/// Check whether a stream response signals stream termination (v1.0).
#[cfg(feature = "streaming")]
fn is_terminal_stream_response(event: &StreamResponse) -> bool {
    event.is_terminal()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_native_client_creation() {
        let client = Client::external("http://localhost:8080/jsonrpc");
        assert_eq!(client.url(), "http://localhost:8080/jsonrpc");
        assert!(client.has_header("content-type"));
        assert!(client.has_header("a2a-version"));
    }

    #[tokio::test]
    async fn test_native_client_with_header() {
        let client = Client::external("http://localhost:8080/jsonrpc")
            .with_header("Authorization".to_string(), "Bearer token".to_string());
        assert!(client.has_header("Authorization"));
    }
}
