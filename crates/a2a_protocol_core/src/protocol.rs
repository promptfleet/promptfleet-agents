//! A2A v1.0 Protocol Handler

use crate::{
    A2AError, A2AMethodRegistry, A2AResult, A2ATransport, AgentCard, MethodMetadata,
    jsonrpc_error_codes,
};
use protocol_transport_core::{
    JsonRpcIncoming, JsonRpcNotification, JsonRpcRequest, JsonRpcResponse,
};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;

pub struct A2AProtocol {
    agent_card: AgentCard,
    registry: A2AMethodRegistry,
}

impl A2AProtocol {
    pub fn new(agent_card: AgentCard) -> Self {
        let mut registry = A2AMethodRegistry::new();
        registry.set_agent_card(agent_card.clone());
        Self {
            agent_card,
            registry,
        }
    }

    pub fn with_registry(agent_card: AgentCard, registry: A2AMethodRegistry) -> Self {
        Self {
            agent_card,
            registry,
        }
    }

    pub fn agent_card(&self) -> &AgentCard {
        &self.agent_card
    }

    pub fn update_agent_card(&mut self, agent_card: AgentCard) {
        self.agent_card = agent_card.clone();
        self.registry.set_agent_card(agent_card);
    }

    pub fn registry(&self) -> &A2AMethodRegistry {
        &self.registry
    }

    pub fn registry_mut(&mut self) -> &mut A2AMethodRegistry {
        &mut self.registry
    }

    pub fn register_method<F>(&mut self, method: &str, description: &str, handler: F)
    where
        F: Fn(JsonRpcRequest) -> A2AResult<JsonRpcResponse> + Send + Sync + 'static,
    {
        self.registry
            .register_method(method, description, Arc::new(handler));
        self.agent_card = self.agent_card.clone().with_capability(method, description);
    }

    pub fn register_notification<F>(&mut self, method: &str, description: &str, handler: F)
    where
        F: Fn(JsonRpcNotification) -> A2AResult<()> + Send + Sync + 'static,
    {
        self.registry
            .register_notification(method, description, Arc::new(handler));
    }

    pub fn handle_request(&self, request: JsonRpcRequest) -> A2AResult<JsonRpcResponse> {
        if !request.is_valid() {
            return Ok(JsonRpcResponse::error(
                request.id,
                jsonrpc_error_codes::INVALID_REQUEST,
                "Invalid JSON-RPC 2.0 request".to_string(),
            ));
        }
        if !self.registry.has_method(&request.method) {
            return Ok(JsonRpcResponse::error(
                request.id,
                jsonrpc_error_codes::METHOD_NOT_FOUND,
                format!("Method '{}' not found", request.method),
            ));
        }
        match self.registry.handle_request(request.clone()) {
            Ok(response) => Ok(response),
            Err(error) => {
                let jsonrpc_error = error.to_jsonrpc_error();
                Ok(JsonRpcResponse::error(
                    request.id,
                    jsonrpc_error.code,
                    jsonrpc_error.message,
                ))
            }
        }
    }

    pub fn handle_notification(&self, notification: JsonRpcNotification) -> A2AResult<()> {
        if !notification.is_valid() {
            return Err(A2AError::protocol_validation_error(
                "Invalid JSON-RPC 2.0 notification",
            ));
        }
        self.registry.handle_notification(notification)
    }

    pub fn handle_incoming(&self, incoming: JsonRpcIncoming) -> A2AResult<Option<JsonRpcResponse>> {
        match incoming {
            JsonRpcIncoming::Request(request) => {
                let response = self.handle_request(request)?;
                Ok(Some(response))
            }
            JsonRpcIncoming::Notification(notification) => {
                self.handle_notification(notification)?;
                Ok(None)
            }
            _ => Err(A2AError::unsupported_operation(
                "This JSON-RPC incoming message variant is not supported by A2AProtocol",
            )),
        }
    }

    pub fn get_capabilities(&self) -> Value {
        json!({
            "name": self.agent_card.name,
            "capabilities": self.agent_card.capabilities,
            "methods": self.registry.list_methods(),
            "notifications": self.registry.list_notifications(),
        })
    }

    pub fn get_method_metadata(&self) -> HashMap<String, &MethodMetadata> {
        let mut metadata = HashMap::new();
        for (name, meta) in self.registry.get_all_metadata() {
            metadata.insert(name.clone(), meta);
        }
        metadata
    }

    pub fn validate_request_params(&self, method: &str, params: &Value) -> A2AResult<()> {
        if let Some(metadata) = self.registry.get_method_metadata(method) {
            if metadata.parameters.is_some() && params.is_null() {
                return Err(A2AError::invalid_params(
                    method,
                    "Parameters are required for this method",
                ));
            }
        }
        Ok(())
    }

    /// Register all A2A v1.0 protocol methods.
    pub fn register_a2a_methods(
        &mut self,
        storage: Option<std::sync::Arc<dyn crate::services::TaskStorage>>,
    ) {
        #[cfg(feature = "event-stream")]
        {
            if let Some(ref mut caps) = self.agent_card.capabilities {
                caps.streaming = true;
            }
            self.registry.set_agent_card(self.agent_card.clone());
        }

        // ── Basic agent methods ─────────────────────────────────────
        self.register_method("Ping", "A2A agent ping", |request| {
            #[cfg(feature = "time-stamps")]
            let timestamp = chrono::Utc::now().to_rfc3339();
            #[cfg(not(feature = "time-stamps"))]
            let timestamp = "not-available";

            Ok(JsonRpcResponse::success(
                request.id,
                json!({"pong": true, "timestamp": timestamp}),
            ))
        });

        let agent_card = self.agent_card.clone();
        self.register_method("GetAgentCard", "Get agent card", move |request| {
            Ok(JsonRpcResponse::success(request.id, json!(agent_card)))
        });

        // ── Discovery ───────────────────────────────────────────────
        {
            use crate::methods::discovery::{
                AgentDiscovery, AuthenticatedExtendedCardParams, DefaultAgentDiscovery,
            };

            let agent_card_for_discovery = self.agent_card.clone();
            self.register_method(
                "GetExtendedAgentCard",
                "Get authenticated extended agent card",
                move |request| {
                    let params: AuthenticatedExtendedCardParams =
                        match serde_json::from_value(request.params.clone()) {
                            Ok(p) => p,
                            Err(_) => {
                                return Ok(JsonRpcResponse::error(
                                    request.id,
                                    jsonrpc_error_codes::INVALID_PARAMS,
                                    "Invalid parameters for GetExtendedAgentCard".to_string(),
                                ));
                            }
                        };
                    let discovery = DefaultAgentDiscovery::new(agent_card_for_discovery.clone());
                    match discovery.agent_authenticated_extended_card(params) {
                        Ok(result) => Ok(JsonRpcResponse::success(request.id, json!(result))),
                        Err(e) => {
                            let rpc_err = e.to_jsonrpc_error();
                            Ok(JsonRpcResponse::error(
                                request.id,
                                rpc_err.code,
                                rpc_err.message,
                            ))
                        }
                    }
                },
            );
        }

        // ── Task lifecycle methods (require storage) ────────────────
        if let Some(storage) = storage {
            use crate::methods::{
                messaging::handle_message_send,
                tasks::{handle_tasks_cancel, handle_tasks_get, handle_tasks_list},
            };

            let s = storage.clone();
            self.register_method("SendMessage", "Send message to agent", move |request| {
                handle_message_send(request, s.clone())
            });

            let s = storage.clone();
            self.register_method("GetTask", "Get task state", move |request| {
                handle_tasks_get(request, s.clone())
            });

            let s = storage.clone();
            self.register_method("CancelTask", "Cancel ongoing task", move |request| {
                handle_tasks_cancel(request, s.clone())
            });

            let s = storage.clone();
            self.register_method("ListTasks", "List agent tasks", move |request| {
                handle_tasks_list(request, s.clone())
            });

            #[cfg(feature = "event-stream")]
            {
                use crate::methods::messaging::handle_tasks_send_subscribe;
                let s = storage.clone();
                self.register_method(
                    "SendStreamingMessage",
                    "Send message and subscribe to SSE updates",
                    move |request| handle_tasks_send_subscribe(request, s.clone()),
                );
            }
        }
    }

    pub async fn send_request<T: A2ATransport>(
        &self,
        transport: &T,
        request: JsonRpcRequest,
    ) -> A2AResult<JsonRpcResponse> {
        transport.send_request(request).await
    }

    pub async fn send_notification<T: A2ATransport>(
        &self,
        transport: &T,
        notification: JsonRpcNotification,
    ) -> A2AResult<()> {
        transport.send_notification(notification).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AgentCard;

    #[test]
    fn test_protocol_creation() {
        let card = AgentCard::new("test-agent").with_capability("ping", "Ping");
        let protocol = A2AProtocol::new(card);
        assert_eq!(protocol.agent_card().name, "test-agent");
    }

    #[test]
    fn test_method_registration_and_handling() {
        let card = AgentCard::new("test-agent");
        let mut protocol = A2AProtocol::new(card);
        protocol.register_method("echo", "Echo the input", |request| {
            Ok(JsonRpcResponse::success(request.id, request.params))
        });

        let request = JsonRpcRequest::new(
            json!("req-123"),
            "echo".to_string(),
            json!({"message": "hello"}),
        );
        let response = protocol.handle_request(request).unwrap();
        assert!(response.is_success());
        assert_eq!(response.result.unwrap()["message"], "hello");
    }

    #[test]
    fn test_unknown_method() {
        let card = AgentCard::new("test-agent");
        let protocol = A2AProtocol::new(card);
        let request = JsonRpcRequest::new(json!("req"), "unknown".to_string(), json!({}));
        let response = protocol.handle_request(request).unwrap();
        assert!(response.is_error());
    }

    #[test]
    fn test_standard_methods() {
        let card = AgentCard::new("test-agent");
        let mut protocol = A2AProtocol::new(card);
        protocol.register_a2a_methods(None);

        let ping = JsonRpcRequest::new(json!("r1"), "Ping".to_string(), json!({}));
        let resp = protocol.handle_request(ping).unwrap();
        assert!(resp.is_success());
        assert_eq!(resp.result.unwrap()["pong"], true);
    }

    #[test]
    fn test_get_agent_card_method() {
        let card = AgentCard::new("test-agent").with_capability("test", "Test method");
        let mut protocol = A2AProtocol::new(card);
        protocol.register_a2a_methods(None);

        let req = JsonRpcRequest::new(json!("r2"), "GetAgentCard".to_string(), json!({}));
        let resp = protocol.handle_request(req).unwrap();
        assert!(resp.is_success());
        assert_eq!(resp.result.unwrap()["name"], "test-agent");
    }

    #[test]
    fn test_with_registry_constructor() {
        let card = AgentCard::new("test-agent");
        let mut registry = A2AMethodRegistry::new();
        registry.register_method(
            "test",
            "Test method",
            Arc::new(|request| Ok(JsonRpcResponse::success(request.id, json!({"test": true})))),
        );
        let protocol = A2AProtocol::with_registry(card, registry);
        assert!(protocol.registry().has_method("test"));
    }

    #[test]
    fn test_update_agent_card() {
        let card = AgentCard::new("original");
        let mut protocol = A2AProtocol::new(card);
        let new_card = AgentCard::new("updated");
        protocol.update_agent_card(new_card);
        assert_eq!(protocol.agent_card().name, "updated");
    }

    #[test]
    fn test_validate_request_params() {
        let card = AgentCard::new("test-agent");
        let mut protocol = A2AProtocol::new(card);
        protocol.registry_mut().register_method_with_metadata(
            "test",
            "Test",
            Some(json!({"type": "object"})),
            None,
            Arc::new(|req| Ok(JsonRpcResponse::success(req.id, json!({})))),
        );
        assert!(
            protocol
                .validate_request_params("test", &json!(null))
                .is_err()
        );
        assert!(
            protocol
                .validate_request_params("test", &json!({"a": 1}))
                .is_ok()
        );
    }

    #[tokio::test]
    async fn test_async_transport_methods() {
        use crate::transport::MockTransport;

        let card = AgentCard::new("test");
        let protocol = A2AProtocol::new(card);
        let mock = MockTransport::new().with_response(
            "ping".to_string(),
            JsonRpcResponse::success(json!("r1"), json!({"pong": true})),
        );

        let request = JsonRpcRequest::new(json!("r1"), "ping".to_string(), json!({}));
        let response = protocol.send_request(&mock, request).await;
        assert!(response.is_ok());
    }
}
