//! Integration tests for sync/async app adapter delegation seams.

#![cfg(not(target_arch = "wasm32"))]

use std::sync::Arc;

use a2a_app_ports::{A2AAppPort, A2AAppPortAsync, AppFuture};
use a2a_http_server::{A2AHttpServer, AgentCard};
use a2a_protocol_core::{
    A2AError, A2AResult,
    data::{Message, MessageRole, Task, TaskState},
    methods::params::{SendMessageRequest, SendMessageResponse},
};
use axum::http::StatusCode;
use pf_test_harness::a2a_http::{call_jsonrpc, jsonrpc_body, message_send_body, tasks_get_body};
use serde_json::json;

struct SyncMessageAdapter {
    card: AgentCard,
    reply_text: String,
}

impl A2AAppPort for SyncMessageAdapter {
    fn build_agent_card(&self) -> AgentCard {
        self.card.clone()
    }

    fn handle_send_message(&self, _params: SendMessageRequest) -> A2AResult<SendMessageResponse> {
        Ok(SendMessageResponse::Message(Message::text(
            MessageRole::Agent,
            self.reply_text.clone(),
            "sync-message-task".to_string(),
        )))
    }
}

struct SyncTaskAdapter {
    card: AgentCard,
    task_id: String,
}

impl A2AAppPort for SyncTaskAdapter {
    fn build_agent_card(&self) -> AgentCard {
        self.card.clone()
    }

    fn handle_send_message(&self, params: SendMessageRequest) -> A2AResult<SendMessageResponse> {
        let mut task = Task::with_id(self.task_id.clone(), "adapter-context".to_string());
        task.add_to_history(params.message);
        task.update_status(TaskState::Working);
        Ok(SendMessageResponse::Task(task))
    }
}

struct SyncFailAdapter {
    card: AgentCard,
}

impl A2AAppPort for SyncFailAdapter {
    fn build_agent_card(&self) -> AgentCard {
        self.card.clone()
    }

    fn handle_send_message(&self, _params: SendMessageRequest) -> A2AResult<SendMessageResponse> {
        Err(A2AError::internal("sync adapter failure"))
    }
}

struct AsyncMessageAdapter {
    card: AgentCard,
    reply_text: String,
}

impl A2AAppPortAsync for AsyncMessageAdapter {
    fn build_agent_card(&self) -> AgentCard {
        self.card.clone()
    }

    fn handle_send_message_async<'a>(&'a self, _params: SendMessageRequest) -> AppFuture<'a> {
        let reply_text = self.reply_text.clone();
        Box::pin(async move {
            Ok(SendMessageResponse::Message(Message::text(
                MessageRole::Agent,
                reply_text,
                "async-message-task".to_string(),
            )))
        })
    }
}

struct AsyncFailAdapter {
    card: AgentCard,
}

impl A2AAppPortAsync for AsyncFailAdapter {
    fn build_agent_card(&self) -> AgentCard {
        self.card.clone()
    }

    fn handle_send_message_async<'a>(&'a self, _params: SendMessageRequest) -> AppFuture<'a> {
        Box::pin(async { Err(A2AError::internal("async adapter failure")) })
    }
}

#[tokio::test]
async fn test_sync_adapter_message_send_delegates_to_adapter_response() {
    let card = AgentCard::new("sync-adapter-agent".to_string());
    let router = A2AHttpServer::new_with_a2a_methods(card.clone())
        .with_app_adapter(Arc::new(SyncMessageAdapter {
            card,
            reply_text: "sync adapter reply".to_string(),
        }))
        .build_router();

    let capture = call_jsonrpc(router, message_send_body("client hello"))
        .await
        .expect("SendMessage should succeed");
    assert_eq!(capture.status, StatusCode::OK);

    let payload = capture
        .body_json
        .expect("SendMessage must return JSON body");
    assert_eq!(payload["result"]["message"]["role"], "ROLE_AGENT");
    assert_eq!(
        payload["result"]["message"]["parts"][0]["text"],
        "sync adapter reply"
    );
}

#[tokio::test]
async fn test_sync_adapter_task_response_is_persisted_for_tasks_get() {
    let card = AgentCard::new("sync-task-adapter-agent".to_string());
    let router = A2AHttpServer::new_with_a2a_methods(card.clone())
        .with_app_adapter(Arc::new(SyncTaskAdapter {
            card,
            task_id: "adapter-task-1".to_string(),
        }))
        .build_router();

    let send_capture = call_jsonrpc(router.clone(), message_send_body("persist this"))
        .await
        .expect("SendMessage should succeed");
    assert_eq!(send_capture.status, StatusCode::OK);
    let send_payload = send_capture
        .body_json
        .expect("SendMessage should return JSON body");
    assert_eq!(send_payload["result"]["task"]["id"], "adapter-task-1");
    assert_eq!(
        send_payload["result"]["task"]["status"]["state"],
        "TASK_STATE_WORKING"
    );

    let get_capture = call_jsonrpc(router, tasks_get_body("adapter-task-1", true, false))
        .await
        .expect("GetTask should succeed");
    assert_eq!(get_capture.status, StatusCode::OK);
    let get_payload = get_capture
        .body_json
        .expect("GetTask should return JSON body");
    assert_eq!(get_payload["result"]["id"], "adapter-task-1");
    assert_eq!(
        get_payload["result"]["history"][0]["parts"][0]["text"],
        "persist this"
    );
}

#[tokio::test]
async fn test_sync_adapter_agent_card_get_uses_adapter_card() {
    let delegated_card = AgentCard::new("Delegated Agent".to_string());

    let router = A2AHttpServer::new_with_a2a_methods(AgentCard::new("base-agent".to_string()))
        .with_app_adapter(Arc::new(SyncMessageAdapter {
            card: delegated_card,
            reply_text: "unused".to_string(),
        }))
        .build_router();

    let capture = call_jsonrpc(
        router,
        jsonrpc_body(json!("card-1"), "GetAgentCard", json!({})),
    )
    .await
    .expect("GetAgentCard should succeed");
    assert_eq!(capture.status, StatusCode::OK);
    let payload = capture
        .body_json
        .expect("GetAgentCard must return JSON body");

    assert_eq!(payload["result"]["name"], "Delegated Agent");
}

#[tokio::test]
async fn test_async_adapter_takes_precedence_over_sync_adapter() {
    let base_card = AgentCard::new("adapter-precedence-agent".to_string());
    let sync_card = AgentCard::new("sync-agent".to_string());
    let async_card = AgentCard::new("async-agent".to_string());
    let router = A2AHttpServer::new_with_a2a_methods(base_card)
        .with_app_adapter(Arc::new(SyncMessageAdapter {
            card: sync_card,
            reply_text: "sync path".to_string(),
        }))
        .with_app_adapter_async(Arc::new(AsyncMessageAdapter {
            card: async_card,
            reply_text: "async path".to_string(),
        }))
        .build_router();

    let capture = call_jsonrpc(router, message_send_body("who handles this"))
        .await
        .expect("SendMessage should succeed");
    assert_eq!(capture.status, StatusCode::OK);
    let payload = capture
        .body_json
        .expect("SendMessage should return JSON body");
    assert_eq!(
        payload["result"]["message"]["parts"][0]["text"],
        "async path"
    );
}

#[tokio::test]
async fn test_sync_adapter_failure_maps_to_jsonrpc_error() {
    let card = AgentCard::new("sync-fail-agent".to_string());
    let router = A2AHttpServer::new_with_a2a_methods(card.clone())
        .with_app_adapter(Arc::new(SyncFailAdapter { card }))
        .build_router();

    let capture = call_jsonrpc(router, message_send_body("will fail"))
        .await
        .expect("request should return HTTP response");
    assert_eq!(capture.status, StatusCode::OK);
    let payload = capture.body_json.expect("JSON-RPC error must have a body");
    assert!(
        payload.get("error").is_some(),
        "response must contain JSON-RPC error"
    );
    assert!(
        payload["error"]["message"]
            .as_str()
            .unwrap()
            .contains("Internal")
    );
}

#[tokio::test]
async fn test_async_adapter_failure_maps_to_jsonrpc_error() {
    let card = AgentCard::new("async-fail-agent".to_string());
    let router = A2AHttpServer::new_with_a2a_methods(card.clone())
        .with_app_adapter_async(Arc::new(AsyncFailAdapter { card }))
        .build_router();

    let capture = call_jsonrpc(router, message_send_body("will fail"))
        .await
        .expect("request should return HTTP response");
    assert_eq!(capture.status, StatusCode::OK);
    let payload = capture.body_json.expect("JSON-RPC error must have a body");
    assert!(
        payload.get("error").is_some(),
        "response must contain JSON-RPC error"
    );
    assert!(
        payload["error"]["message"]
            .as_str()
            .unwrap()
            .contains("Internal")
    );
}
