//! Integration tests for non-stream JSON-RPC methods on A2A HTTP server.

#![cfg(not(target_arch = "wasm32"))]

use a2a_http_server::{A2AHttpServer, AgentCard};
use axum::http::StatusCode;
use pf_test_harness::a2a_http::{
    call_jsonrpc, jsonrpc_body, message_send_body, tasks_cancel_body, tasks_get_body,
    tasks_list_body,
};
use serde_json::json;

fn build_server() -> axum::Router {
    let card = AgentCard::new("test-jsonrpc-agent".to_string());
    A2AHttpServer::new_with_a2a_methods(card).build_router()
}

#[tokio::test]
async fn test_agent_ping_returns_pong() {
    let router = build_server();
    let capture = call_jsonrpc(router, jsonrpc_body(json!("ping-1"), "Ping", json!({})))
        .await
        .expect("ping request should complete");

    assert_eq!(capture.status, StatusCode::OK);
    let payload = capture.body_json.expect("ping must return JSON body");
    assert_eq!(payload["jsonrpc"], "2.0");
    assert_eq!(payload["id"], "ping-1");
    assert_eq!(payload["result"]["pong"], true);
}

#[tokio::test]
async fn test_message_send_creates_task_and_tasks_get_returns_it() {
    let router = build_server();

    let send_capture = call_jsonrpc(router.clone(), message_send_body("hello from integration"))
        .await
        .expect("SendMessage request should complete");
    assert_eq!(send_capture.status, StatusCode::OK);

    let send_payload = send_capture
        .body_json
        .expect("SendMessage must return JSON body");
    let task_id = send_payload["result"]["task"]["id"]
        .as_str()
        .expect("task id must be present in result.task.id")
        .to_string();
    assert_eq!(
        send_payload["result"]["task"]["status"]["state"],
        "TASK_STATE_WORKING"
    );
    assert!(
        send_payload["result"]["task"].get("history").is_none(),
        "SendMessage should default to compact task payloads"
    );

    let get_capture = call_jsonrpc(router, tasks_get_body(&task_id, true, true))
        .await
        .expect("GetTask request should complete");
    assert_eq!(get_capture.status, StatusCode::OK);

    let get_payload = get_capture
        .body_json
        .expect("GetTask must return JSON body");
    assert_eq!(get_payload["result"]["id"], task_id);
    assert_eq!(
        get_payload["result"]["status"]["state"],
        "TASK_STATE_WORKING"
    );
    assert_eq!(get_payload["result"]["history"][0]["role"], "ROLE_USER");
}

#[tokio::test]
async fn test_tasks_list_includes_previously_created_task() {
    let router = build_server();

    let send_capture = call_jsonrpc(router.clone(), message_send_body("list me"))
        .await
        .expect("SendMessage request should complete");
    let send_payload = send_capture
        .body_json
        .expect("SendMessage must return JSON body");
    let task_id = send_payload["result"]["task"]["id"]
        .as_str()
        .expect("task id must be present in result.task.id")
        .to_string();

    let list_capture = call_jsonrpc(router, tasks_list_body(Some(20), Some(0), None, None))
        .await
        .expect("ListTasks request should complete");
    assert_eq!(list_capture.status, StatusCode::OK);

    let list_payload = list_capture
        .body_json
        .expect("ListTasks must return JSON body");
    let tasks = list_payload["result"]["tasks"]
        .as_array()
        .expect("ListTasks result.tasks must be an array");
    assert!(!tasks.is_empty(), "expected at least one task in list");
    assert!(
        tasks
            .iter()
            .any(|task| task.get("id").and_then(|v| v.as_str()) == Some(task_id.as_str())),
        "ListTasks should include created task_id={task_id}; payload={list_payload}",
    );
}

#[tokio::test]
async fn test_tasks_cancel_sets_canceled_state_and_reason() {
    let router = build_server();

    let send_capture = call_jsonrpc(router.clone(), message_send_body("cancel me"))
        .await
        .expect("SendMessage request should complete");
    let send_payload = send_capture
        .body_json
        .expect("SendMessage must return JSON body");
    let task_id = send_payload["result"]["task"]["id"]
        .as_str()
        .expect("task id must be present in result.task.id")
        .to_string();

    let cancel_capture = call_jsonrpc(
        router.clone(),
        tasks_cancel_body(&task_id, Some("user requested cancellation")),
    )
    .await
    .expect("CancelTask request should complete");
    assert_eq!(cancel_capture.status, StatusCode::OK);
    let cancel_payload = cancel_capture
        .body_json
        .expect("CancelTask must return JSON body");
    assert_eq!(
        cancel_payload["result"]["status"]["state"],
        "TASK_STATE_CANCELED"
    );
    assert_eq!(
        cancel_payload["result"]["metadata"]["cancellation_reason"],
        "user requested cancellation"
    );

    let get_capture = call_jsonrpc(router, tasks_get_body(&task_id, false, false))
        .await
        .expect("GetTask request should complete");
    let get_payload = get_capture
        .body_json
        .expect("GetTask must return JSON body");
    assert_eq!(
        get_payload["result"]["status"]["state"],
        "TASK_STATE_CANCELED"
    );
}

#[tokio::test]
async fn test_message_send_ping_returns_direct_message_shape() {
    let router = build_server();

    let capture = call_jsonrpc(router, message_send_body("ping"))
        .await
        .expect("SendMessage ping should complete");
    assert_eq!(capture.status, StatusCode::OK);
    let payload = capture
        .body_json
        .expect("SendMessage must return JSON body");

    assert_eq!(payload["result"]["message"]["role"], "ROLE_AGENT");
    assert_eq!(payload["result"]["message"]["parts"][0]["text"], "pong");
    assert!(payload["result"]["message"].get("status").is_none());
}

#[tokio::test]
async fn test_invalid_jsonrpc_payload_returns_bad_request() {
    let router = build_server();
    let capture = call_jsonrpc(router, "not-json".to_string())
        .await
        .expect("request should return HTTP response");

    assert_eq!(capture.status, StatusCode::BAD_REQUEST);
    assert!(capture.body_json.is_none());
}
