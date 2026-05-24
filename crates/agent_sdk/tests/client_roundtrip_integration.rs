#![cfg(all(
    not(target_arch = "wasm32"),
    feature = "a2a-client",
    feature = "a2a-server"
))]

use a2a_http_server::{A2AHttpServer, AgentCard};
use agent_sdk::{SdkError, a2a::A2aClient};
use axum::{Json, Router, routing::get};
use serde_json::Value;
use tokio::net::TcpListener;

fn extract_task_id(value: &Value) -> Option<&str> {
    value.get("id").and_then(Value::as_str).or_else(|| {
        value
            .get("task")
            .and_then(|task| task.get("id"))
            .and_then(Value::as_str)
    })
}

fn extract_task_state(value: &Value) -> Option<&str> {
    value
        .get("status")
        .and_then(|status| status.get("state"))
        .and_then(Value::as_str)
        .or_else(|| {
            value
                .get("task")
                .and_then(|task| task.get("status"))
                .and_then(|status| status.get("state"))
                .and_then(Value::as_str)
        })
}

async fn spawn_test_server() -> (String, tokio::task::JoinHandle<()>) {
    let card = AgentCard::new("agent-client-it");
    let card_for_agent_route = card.clone();
    let base_router = A2AHttpServer::new_with_a2a_methods(card).build_router();

    // a2a_http_client::Client::agent_card() currently targets `/agent`.
    let router: Router = base_router.route(
        "/agent",
        get(move || {
            let card = card_for_agent_route.clone();
            async move { Json(serde_json::to_value(card).expect("serialize card")) }
        }),
    );

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind test listener");
    let addr = listener.local_addr().expect("read local addr");
    let endpoint = format!("http://{addr}/jsonrpc");

    let handle = tokio::spawn(async move {
        axum::serve(listener, router)
            .await
            .expect("serve test router");
    });

    (endpoint, handle)
}

#[tokio::test]
async fn test_agent_client_ping_connectivity_and_card() {
    let (endpoint, handle) = spawn_test_server().await;

    let client = A2aClient::direct(&endpoint).expect("create direct client");
    let ping = client.ping().await.expect("ping should succeed");
    assert_eq!(ping["pong"], true);

    let reachable = client
        .check_connectivity()
        .await
        .expect("connectivity check should succeed");
    assert!(reachable, "expected target server to be reachable");

    let card_json = client
        .agent_card()
        .await
        .expect("agent card request should succeed");
    let card: Value = serde_json::from_str(&card_json).expect("agent card should be valid JSON");
    assert_eq!(card["name"], "agent-client-it");

    handle.abort();
}

#[tokio::test]
async fn test_agent_client_message_and_task_lifecycle_roundtrip() {
    let (endpoint, handle) = spawn_test_server().await;
    let client = A2aClient::direct(&endpoint).expect("create direct client");

    let send_result = client
        .send_message("roundtrip hello", Some("ctx-roundtrip".to_string()))
        .await
        .expect("SendMessage should succeed");
    let task_id = extract_task_id(&send_result)
        .expect("SendMessage should return task id")
        .to_string();
    assert_eq!(extract_task_state(&send_result), Some("TASK_STATE_WORKING"));
    assert!(
        send_result.get("history").is_none()
            && send_result
                .get("task")
                .and_then(|task| task.get("history"))
                .is_none(),
        "default send_message should return compact task payloads"
    );

    let task = client
        .get_task(&task_id)
        .await
        .expect("GetTask should succeed");
    assert_eq!(task.id, task_id);
    assert_eq!(task.status.state, agent_sdk::a2a::TaskState::Working);
    assert!(
        task.history.as_ref().is_some_and(|h| !h.is_empty()),
        "expected task history to include original message"
    );

    let bounded = client
        .get_task_with_history(&task_id, Some(1))
        .await
        .expect("bounded GetTask should succeed");
    assert_eq!(
        bounded.history.as_ref().map(|history| history.len()),
        Some(1),
        "expected bounded history length from explicit get_task_with_history"
    );

    let listed = client
        .list_tasks(None, None, Some(20), None)
        .await
        .expect("tasks/list should succeed");
    let tasks = listed["tasks"]
        .as_array()
        .expect("tasks/list should return tasks array");
    assert!(
        tasks
            .iter()
            .any(|t| t.get("id").and_then(|v| v.as_str()) == Some(task_id.as_str())),
        "tasks/list should include previously created task id={task_id}; listed={listed}"
    );

    let canceled = client
        .cancel_task(&task_id)
        .await
        .expect("tasks/cancel should succeed");
    assert_eq!(canceled.status.state, agent_sdk::a2a::TaskState::Canceled);

    handle.abort();
}

#[tokio::test]
async fn test_agent_client_get_task_missing_maps_to_method_execution_error() {
    let (endpoint, handle) = spawn_test_server().await;
    let client = A2aClient::direct(&endpoint).expect("create direct client");

    let err = client
        .get_task("task-does-not-exist")
        .await
        .expect_err("missing task should produce SDK error");

    match err {
        SdkError::MethodExecution { method, details } => {
                assert_eq!(method, "GetTask");
            assert!(
                details.contains("Task not found") || details.contains("RPC error"),
                "unexpected details: {details}"
            );
        }
        other => panic!("expected MethodExecution error, got {other:?}"),
    }

    handle.abort();
}
