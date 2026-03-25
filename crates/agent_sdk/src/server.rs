//! Agent SDK A2A server implementation.
//!
//! Server implementation that adapts to target architecture:
//! - WASM: Uses Spin SDK for SpinKube deployment
//! - Native: Uses native HTTP server for testing

#[cfg(all(feature = "a2a-server", target_arch = "wasm32"))]
use crate::error::SdkError;
#[cfg(feature = "a2a-server")]
use crate::{error::SdkResult, Agent};
#[cfg(feature = "a2a-server")]
use a2a_http_server::A2AHttpServer;
#[cfg(all(
    feature = "a2a-server",
    feature = "event-stream",
    not(target_arch = "wasm32")
))]
use a2a_protocol_core::methods::params::MessageSendParams;
#[cfg(feature = "a2a-server")]
#[cfg(all(
    feature = "a2a-server",
    feature = "event-stream",
    not(target_arch = "wasm32")
))]
use a2a_protocol_core::streaming::StreamResponse;
#[cfg(all(feature = "a2a-server", not(target_arch = "wasm32")))]
use anyhow::Result;

#[cfg(all(feature = "a2a-server", feature = "agent-observability"))]
use observability;

// Conditional imports based on target AND feature
#[cfg(all(target_arch = "wasm32", feature = "a2a-server"))]
use spin_sdk::http::{IncomingRequest, Response as SpinResponse};

// Native-only imports
#[cfg(all(not(target_arch = "wasm32"), feature = "a2a-server"))]
use axum;
#[cfg(all(
    feature = "a2a-server",
    feature = "event-stream",
    not(target_arch = "wasm32")
))]
use futures_util::StreamExt;

/// **A2aServer** - Clean A2A wrapper around the HTTP server
///
/// Provides a clean interface for serving [`crate::Agent`] over A2A HTTP in
/// both WASM and native environments.
#[cfg(feature = "a2a-server")]
pub struct A2aServer {
    inner: A2AHttpServer,
}

#[cfg(feature = "a2a-server")]
#[cfg(not(target_arch = "wasm32"))]
struct SdkAppAdapter {
    agent: std::sync::Arc<crate::Agent>,
}

#[cfg(feature = "a2a-server")]
#[cfg(not(target_arch = "wasm32"))]
impl a2a_app_ports::A2AAppPort for SdkAppAdapter {
    fn build_agent_card(&self) -> a2a_http_server::AgentCard {
        crate::a2a::agent_card(self.agent.as_ref())
    }

    fn handle_send_message(
        &self,
        params: a2a_protocol_core::methods::params::SendMessageRequest,
    ) -> a2a_protocol_core::A2AResult<a2a_protocol_core::methods::params::SendMessageResponse> {
        tokio::runtime::Handle::current()
            .block_on(crate::a2a::handle_message_send(self.agent.as_ref(), params))
    }
}

#[cfg(feature = "a2a-server")]
#[cfg(not(target_arch = "wasm32"))]
struct SdkAppAdapterAsync {
    agent: std::sync::Arc<crate::Agent>,
}

#[cfg(feature = "a2a-server")]
#[cfg(not(target_arch = "wasm32"))]
impl a2a_app_ports::A2AAppPortAsync for SdkAppAdapterAsync {
    fn build_agent_card(&self) -> a2a_http_server::AgentCard {
        crate::a2a::agent_card(self.agent.as_ref())
    }

    fn handle_send_message_async<'a>(
        &'a self,
        params: a2a_protocol_core::methods::params::SendMessageRequest,
    ) -> a2a_app_ports::AppFuture<'a> {
        Box::pin(crate::a2a::handle_message_send(self.agent.as_ref(), params))
    }
}

#[cfg(all(
    feature = "a2a-server",
    feature = "event-stream",
    not(target_arch = "wasm32")
))]
struct SdkStreamingAdapter {
    agent: std::sync::Arc<crate::Agent>,
}

#[cfg(all(
    feature = "a2a-server",
    feature = "event-stream",
    not(target_arch = "wasm32")
))]
impl a2a_http_server::A2AStreamingAppPort for SdkStreamingAdapter {
    fn handle_streaming_task(
        &self,
        task_id: String,
        message: a2a_protocol_core::data::Message,
        _request_headers: std::collections::HashMap<String, String>,
    ) -> Result<
        std::pin::Pin<Box<dyn futures_util::Stream<Item = StreamResponse> + Send>>,
        a2a_protocol_core::A2AError,
    > {
        let context_id = message
            .context_id
            .clone()
            .unwrap_or_else(|| task_id.clone());
        let params = MessageSendParams {
            message,
            tenant: None,
            configuration: None,
            metadata: None,
        };
        let stream = crate::a2a::task_trace_stream(self.agent.clone(), params);
        let a2a_ctx = crate::streaming::A2aSseContext {
            task_id: task_id.clone(),
            context_id,
            jsonrpc_id: serde_json::Value::String(task_id),
        };
        let mapped = stream.flat_map(move |event| {
            let events = crate::streaming::map_trace_to_stream_response(event, &a2a_ctx);
            futures_util::stream::iter(events)
        });
        Ok(Box::pin(mapped))
    }
}

#[cfg(all(target_arch = "wasm32", feature = "a2a-server"))]
struct WasmSdkAppAdapterAsync {
    agent: std::sync::Arc<crate::Agent>,
}

#[cfg(all(target_arch = "wasm32", feature = "a2a-server"))]
impl a2a_app_ports::A2AAppPortAsync for WasmSdkAppAdapterAsync {
    fn build_agent_card(&self) -> a2a_http_server::AgentCard {
        crate::a2a::agent_card(self.agent.as_ref())
    }

    fn handle_send_message_async<'a>(
        &'a self,
        params: a2a_protocol_core::methods::params::SendMessageRequest,
    ) -> a2a_app_ports::AppFuture<'a> {
        Box::pin(crate::a2a::handle_message_send(self.agent.as_ref(), params))
    }
}

#[cfg(feature = "a2a-server")]
impl A2aServer {
    /// Create a new server with A2A standard methods
    pub fn with_a2a_methods(agent: Agent) -> SdkResult<Self> {
        Self::from_shared_agent(std::sync::Arc::new(agent))
    }

    /// Create a new server with A2A standard methods around a shared runtime.
    pub fn from_shared_agent(agent: std::sync::Arc<Agent>) -> SdkResult<Self> {
        let agent_card = crate::a2a::agent_card(agent.as_ref());

        #[cfg(feature = "agent-observability")]
        let obs = agent
            .get_service::<observability::Obs>()
            .map(|o| (*o).clone());

        #[cfg(not(target_arch = "wasm32"))]
        let agent_arc = agent;
        #[cfg(target_arch = "wasm32")]
        let agent_wasm = agent;

        let inner = {
            // Use the adapter-owned protocol storage handle to ensure shared persistence
            #[cfg(not(target_arch = "wasm32"))]
            let server = {
                let storage = crate::a2a::server_task_storage(agent_arc.as_ref())?;
                a2a_http_server::A2AHttpServer::new_with_storage(agent_card, storage)
            };
            #[cfg(target_arch = "wasm32")]
            let server = {
                let storage = crate::a2a::server_task_storage(&agent_wasm)?;
                a2a_http_server::A2AHttpServer::new_with_storage(agent_card, storage)
            };

            #[cfg(not(target_arch = "wasm32"))]
            {
                let adapter = SdkAppAdapter {
                    agent: agent_arc.clone(),
                };
                let adapter_async = SdkAppAdapterAsync {
                    agent: agent_arc.clone(),
                };
                let server = server
                    .with_app_adapter(std::sync::Arc::new(adapter))
                    .with_app_adapter_async(std::sync::Arc::new(adapter_async));
                #[cfg(feature = "event-stream")]
                let server = server.with_streaming_port(std::sync::Arc::new(SdkStreamingAdapter {
                    agent: agent_arc.clone(),
                }));

                #[cfg(feature = "agent-observability")]
                let server = if let Some(obs) = obs.clone() {
                    server.with_observability(obs)
                } else {
                    server
                };

                server
            }
            #[cfg(target_arch = "wasm32")]
            {
                // Attach async adapter on WASM
                let adapter_async = WasmSdkAppAdapterAsync { agent: agent_wasm };
                let server = server.with_app_adapter_async(std::sync::Arc::new(adapter_async));

                #[cfg(feature = "agent-observability")]
                let server = if let Some(obs) = obs.clone() {
                    server.with_observability(obs)
                } else {
                    server
                };

                server
            }
        };

        Ok(Self { inner })
    }

    /// Get agent ID (for testing)
    pub fn agent_id(&self) -> &str {
        self.inner.agent_id()
    }

    /// Check if server can serve (for testing)
    pub fn can_serve(&self) -> bool {
        self.inner.can_serve()
    }

    // WASM-specific methods
    #[cfg(target_arch = "wasm32")]
    pub fn serve_request(&self, request: IncomingRequest) -> SdkResult<SpinResponse> {
        let spin_request = self.convert_spin_request(request)?;
        let response = self.inner.serve_request(spin_request).map_err(|e| {
            SdkError::method_execution("handle_request", format!("Request handling failed: {}", e))
        })?;
        Ok(response)
    }

    #[cfg(all(target_arch = "wasm32", feature = "a2a-server"))]
    pub fn serve_request_unified(
        &self,
        request: spin_sdk::http::Request,
    ) -> SdkResult<SpinResponse> {
        let response = self.inner.serve_request(request).map_err(|e| {
            SdkError::method_execution("handle_request", format!("Request handling failed: {}", e))
        })?;
        Ok(response)
    }

    #[cfg(all(target_arch = "wasm32", feature = "a2a-server"))]
    pub async fn serve_request_unified_async(
        &self,
        request: spin_sdk::http::Request,
    ) -> SdkResult<SpinResponse> {
        let response = self.inner.serve_request_async(request).await.map_err(|e| {
            SdkError::method_execution(
                "handle_request_async",
                format!("Request handling failed: {}", e),
            )
        })?;
        Ok(response)
    }

    #[cfg(all(target_arch = "wasm32", feature = "a2a-server"))]
    pub async fn serve_request_async(&self, request: IncomingRequest) -> SdkResult<SpinResponse> {
        let spin_request = self.convert_spin_request(request)?;
        let response = self
            .inner
            .serve_request_async(spin_request)
            .await
            .map_err(|e| {
                SdkError::method_execution(
                    "handle_request_async",
                    format!("Request handling failed: {}", e),
                )
            })?;
        Ok(response)
    }

    // Native-specific methods
    #[cfg(not(target_arch = "wasm32"))]
    pub fn build_router(self) -> axum::Router {
        self.inner.build_router()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn serve(self, addr: &str) -> Result<()> {
        self.inner.serve(addr).await
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn serve_request_simulation(
        &self,
        method: &str,
        path: &str,
        headers: axum::http::HeaderMap,
        body: Vec<u8>,
    ) -> Result<(axum::http::StatusCode, axum::http::HeaderMap, Vec<u8>)> {
        self.inner.serve_request(method, path, headers, body).await
    }

    // WASM helper methods
    #[cfg(target_arch = "wasm32")]
    fn convert_spin_request(&self, request: IncomingRequest) -> SdkResult<spin_sdk::http::Request> {
        // Extract data from incoming request
        let method = request.method().to_string();
        let path = request.path_with_query().unwrap_or("/".to_string());

        // Convert method string to Method enum
        let method_enum = match method.as_str() {
            "GET" => spin_sdk::http::Method::Get,
            "POST" => spin_sdk::http::Method::Post,
            "PUT" => spin_sdk::http::Method::Put,
            "DELETE" => spin_sdk::http::Method::Delete,
            "HEAD" => spin_sdk::http::Method::Head,
            "OPTIONS" => spin_sdk::http::Method::Options,
            "PATCH" => spin_sdk::http::Method::Patch,
            _ => spin_sdk::http::Method::Get, // Default fallback
        };

        Ok(spin_sdk::http::Request::builder()
            .method(method_enum)
            .uri(path)
            .build())
    }
}

/// **Convenience Functions**

/// Create a server with standard A2A methods (requires server feature)
#[cfg(feature = "a2a-server")]
pub fn create_a2a_server(agent: Agent) -> SdkResult<A2aServer> {
    A2aServer::with_a2a_methods(agent)
}

/// Create a server with standard A2A methods (feature guard)
#[cfg(not(feature = "a2a-server"))]
pub fn create_a2a_server(_agent: Agent) -> SdkResult<A2aServer> {
    Err(SdkError::feature_not_enabled("a2a-server"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "a2a-server")]
    use crate::Agent;

    #[cfg(feature = "a2a-server")]
    #[test]
    fn test_server_creation() {
        let agent = Agent::new_runtime("test-agent").unwrap();

        // Check that the agent name matches what we expect
        assert_eq!(agent.config().name, "test-agent");

        let server = A2aServer::with_a2a_methods(agent).unwrap();

        // AgentCard id is derived from configured agent name
        assert_eq!(server.agent_id(), "test-agent");
        assert!(server.can_serve());
    }

    #[cfg(all(feature = "a2a-server", not(target_arch = "wasm32")))]
    #[tokio::test]
    async fn test_native_server_simulation() {
        let agent = Agent::new_runtime("test-agent").unwrap();
        let server = A2aServer::with_a2a_methods(agent).unwrap();

        let headers = axum::http::HeaderMap::new();
        let body = r#"{"jsonrpc":"2.0","id":"test","method":"Ping","params":null}"#
            .as_bytes()
            .to_vec();

        let (status, _headers, _response_body) = server
            .serve_request_simulation("POST", "/jsonrpc", headers, body)
            .await
            .unwrap();
        assert_eq!(status, axum::http::StatusCode::OK);
    }

    #[cfg(not(feature = "a2a-server"))]
    #[test]
    fn test_server_feature_guard() {
        use crate::Agent;
        let agent = Agent::new_runtime("test-agent").unwrap();
        let result = create_a2a_server(agent);
        assert!(matches!(
            result.unwrap_err(),
            SdkError::FeatureNotEnabled { .. }
        ));
    }
}
