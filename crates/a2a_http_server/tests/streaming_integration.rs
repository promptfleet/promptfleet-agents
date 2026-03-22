//! Integration test: A2A SSE streaming endpoint (SendStreamingMessage).
//!
//! Exercises the full path: JSON-RPC request → handle_send_streaming_message →
//! A2AStreamingAppPort → SSE response with A2A v1.0 wire format.

#![cfg(not(target_arch = "wasm32"))]
#![cfg(feature = "event-stream")]

use a2a_http_server::{A2AHttpServer, A2AStreamingAppPort, AgentCard};
use a2a_protocol_core::{
    data::{Message, MessageRole, Part, TaskState, TaskStatus},
    streaming::{StreamResponse, TaskStatusUpdateEvent},
};
use axum::body::Body;
use http_body_util::BodyExt;
use hyper::Request;
use pf_test_harness::a2a_http::send_subscribe_body;
use pf_test_harness::sse::SseCollector;
use serde_json::json;
use std::{pin::Pin, sync::Arc, time::Duration};
use tower::ServiceExt;

struct MockStreamingPort {
    events: Vec<StreamResponse>,
}

impl A2AStreamingAppPort for MockStreamingPort {
    fn handle_streaming_task(
        &self,
        _task_id: String,
        _message: Message,
        _request_headers: std::collections::HashMap<String, String>,
    ) -> Result<
        Pin<Box<dyn futures_util::Stream<Item = StreamResponse> + Send>>,
        a2a_protocol_core::A2AError,
    > {
        let events = self.events.clone();
        Ok(Box::pin(futures_util::stream::iter(events)))
    }
}

fn build_server(events: Vec<StreamResponse>) -> axum::Router {
    let card = AgentCard::new("test-streaming-agent".to_string());
    let port: Arc<dyn A2AStreamingAppPort> = Arc::new(MockStreamingPort { events });
    let server = A2AHttpServer::new_with_a2a_methods(card).with_streaming_port(port);
    server.build_router()
}

#[tokio::test]
async fn streaming_endpoint_returns_sse_with_correct_wire_format() {
    let mock_events = vec![
        StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
            id: json!("sub-1"),
            task_id: "task-mock".into(),
            context_id: "ctx-mock".into(),
            status: TaskStatus::new(TaskState::Working),
        }),
        StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
            id: json!("sub-1"),
            task_id: "task-mock".into(),
            context_id: "ctx-mock".into(),
            status: TaskStatus {
                state: TaskState::Working,
                message: Some(Message::new(
                    MessageRole::Agent,
                    vec![Part::text("Hello")],
                    "task-mock".into(),
                )),
                timestamp: None,
            },
        }),
        StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
            id: json!("sub-1"),
            task_id: "task-mock".into(),
            context_id: "ctx-mock".into(),
            status: TaskStatus {
                state: TaskState::Completed,
                message: Some(Message::new(
                    MessageRole::Agent,
                    vec![Part::text("Done")],
                    "task-mock".into(),
                )),
                timestamp: None,
            },
        }),
    ];

    let router = build_server(mock_events);
    let body_str = send_subscribe_body("Hello agent");

    let request = Request::builder()
        .method("POST")
        .uri("/jsonrpc")
        .header("content-type", "application/json")
        .body(Body::from(body_str))
        .unwrap();

    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), 200);

    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        content_type.contains("text/event-stream"),
        "Expected SSE content-type, got: {content_type}"
    );

    let capture = SseCollector::from_response(response)
        .with_timeout(Duration::from_secs(5))
        .collect_all()
        .await
        .expect("collect SSE response");
    let sse_events: Vec<(String, serde_json::Value)> = capture
        .frames
        .iter()
        .filter_map(|frame| {
            frame
                .data_json
                .as_ref()
                .map(|json| (frame.event.clone(), json.clone()))
        })
        .collect();

    assert!(
        sse_events.len() >= 3,
        "Expected at least 3 SSE events, got {} — body: {}",
        sse_events.len(),
        capture.raw_body,
    );

    for (event_name, _) in &sse_events {
        assert_eq!(event_name, "statusUpdate");
    }

    let (_, first_data) = &sse_events[0];
    assert_eq!(first_data["jsonrpc"], "2.0");
    assert_eq!(first_data["id"], "sub-1");
    assert_eq!(
        first_data["result"]["statusUpdate"]["status"]["state"],
        "TASK_STATE_WORKING"
    );

    let (_, last_data) = sse_events.last().unwrap();
    assert_eq!(
        last_data["result"]["statusUpdate"]["status"]["state"],
        "TASK_STATE_COMPLETED"
    );
}

#[tokio::test]
async fn streaming_capability_reflected_in_agent_card() {
    let router = build_server(vec![]);

    let request = Request::builder()
        .method("GET")
        .uri("/.well-known/agent-card.json")
        .body(Body::empty())
        .unwrap();

    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), 200);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let card: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(
        card["capabilities"]["streaming"], true,
        "Agent card should reflect streaming capability"
    );
}

#[tokio::test]
async fn streaming_endpoint_not_triggered_without_port() {
    let card = AgentCard::new("no-stream-agent".to_string());
    let server = A2AHttpServer::new_with_a2a_methods(card);
    let router = server.build_router();

    let body_str = send_subscribe_body("test");
    let request = Request::builder()
        .method("POST")
        .uri("/jsonrpc")
        .header("content-type", "application/json")
        .body(Body::from(body_str))
        .unwrap();

    let response = router.oneshot(request).await.unwrap();
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        content_type.contains("application/json"),
        "Without streaming port, should fall through to normal JSON-RPC (got: {content_type})"
    );
}
