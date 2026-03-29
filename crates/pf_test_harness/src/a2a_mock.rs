//! Mock A2A server for testing sub-agent delegation and A2A client code.
//!
//! Spins up a real HTTP server on a random local port that responds to
//! `SendStreamingMessage` with configurable SSE events and `SendMessage`
//! with configurable JSON-RPC results.

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use a2a_protocol_core::data::{Message, MessageRole, Part, TaskState, TaskStatus};
use a2a_protocol_core::streaming::{StreamResponse, TaskStatusUpdateEvent};
use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::http::header;
use axum::response::IntoResponse;
use axum::routing::post;
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio::sync::oneshot;

/// Pre-built SSE event helpers.
pub fn sse_status(state: TaskState, text: Option<&str>) -> StreamResponse {
    let mut status = TaskStatus::new(state);
    status.message = text.map(|t| {
        Message::new(
            MessageRole::Agent,
            vec![Part::text(t.to_string())],
            "mock-task-1".to_string(),
        )
    });
    StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
        id: json!("mock-rpc-1"),
        task_id: "mock-task-1".to_string(),
        context_id: "mock-task-1".to_string(),
        status,
    })
}

/// Configuration for the mock server.
#[derive(Clone)]
struct MockConfig {
    sse_responses: Arc<Mutex<VecDeque<SseResponse>>>,
    sync_response: Value,
    cancel_requests: Arc<Mutex<Vec<Value>>>,
}

#[derive(Clone)]
enum SseResponse {
    Events(Vec<StreamResponse>),
    Raw(String),
}

/// A mock A2A server bound to a random port.
pub struct MockA2AServer {
    url: String,
    shutdown_tx: Option<oneshot::Sender<()>>,
    addr: SocketAddr,
    cancel_requests: Arc<Mutex<Vec<Value>>>,
}

impl MockA2AServer {
    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    pub fn cancel_requests(&self) -> Vec<Value> {
        self.cancel_requests
            .lock()
            .expect("mock cancel request lock")
            .clone()
    }

    /// Shuts down the server. Also happens automatically on drop.
    pub fn shutdown(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }
}

impl Drop for MockA2AServer {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Builder for constructing a MockA2AServer.
pub struct MockA2AServerBuilder {
    sse_responses: Vec<SseResponse>,
    sync_response: Value,
}

impl MockA2AServerBuilder {
    pub fn new() -> Self {
        Self {
            sse_responses: vec![SseResponse::Events(vec![
                sse_status(TaskState::Working, Some("processing...")),
                sse_status(TaskState::Completed, Some("done")),
            ])],
            sync_response: json!({
                "jsonrpc": "2.0",
                "id": "mock-rpc-1",
                "result": {
                    "id": "mock-task-1",
                    "status": {
                        "state": "TASK_STATE_COMPLETED",
                        "message": {
                            "role": "agent",
                            "parts": [{"text": "sync result"}],
                            "messageId": "mock-msg-1"
                        }
                    }
                }
            }),
        }
    }

    /// Set the SSE events to return for `SendStreamingMessage`.
    pub fn sse_events(mut self, events: Vec<StreamResponse>) -> Self {
        self.sse_responses = vec![SseResponse::Events(events)];
        self
    }

    /// Set a raw SSE body to return for `SendStreamingMessage`.
    pub fn raw_sse_body(mut self, body: impl Into<String>) -> Self {
        self.sse_responses = vec![SseResponse::Raw(body.into())];
        self
    }

    /// Return a deterministic sequence of SSE responses across repeated requests.
    pub fn sse_response_sequence(mut self, responses: Vec<String>) -> Self {
        self.sse_responses = responses.into_iter().map(SseResponse::Raw).collect();
        self
    }

    /// Set the JSON-RPC response for `SendMessage`.
    pub fn sync_response(mut self, response: Value) -> Self {
        self.sync_response = response;
        self
    }

    /// Spawn the server on a random port. Returns a `MockA2AServer` with its URL.
    pub async fn spawn(self) -> MockA2AServer {
        let config = Arc::new(MockConfig {
            sse_responses: Arc::new(Mutex::new(self.sse_responses.into())),
            sync_response: self.sync_response,
            cancel_requests: Arc::new(Mutex::new(Vec::new())),
        });

        let app = Router::new()
            .route("/", post(handle_jsonrpc))
            .with_state(config.clone());

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind to random port");
        let addr = listener.local_addr().expect("get local addr");
        let url = format!("http://127.0.0.1:{}", addr.port());

        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

        tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await
                .ok();
        });

        MockA2AServer {
            url,
            shutdown_tx: Some(shutdown_tx),
            addr,
            cancel_requests: Arc::clone(&config.cancel_requests),
        }
    }
}

impl Default for MockA2AServerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

async fn handle_jsonrpc(
    State(config): State<Arc<MockConfig>>,
    axum::Json(body): axum::Json<Value>,
) -> impl IntoResponse {
    let method = body.get("method").and_then(|m| m.as_str()).unwrap_or("");
    let id = body.get("id").cloned().unwrap_or(json!(null));

    match method {
        "SendStreamingMessage" => {
            let response = config
                .sse_responses
                .lock()
                .expect("mock SSE response lock")
                .pop_front();
            let sse_body = match response {
                Some(SseResponse::Raw(body)) => body,
                Some(SseResponse::Events(events)) => encode_sse_events(&events),
                None => encode_sse_events(&[]),
            };

            (
                [
                    (header::CONTENT_TYPE, "text/event-stream"),
                    (header::CACHE_CONTROL, "no-cache"),
                ],
                Body::from(sse_body),
            )
                .into_response()
        }
        "SendMessage" => {
            let response = config.sync_response.clone();
            axum::Json(response).into_response()
        }
        "CancelTask" => {
            config
                .cancel_requests
                .lock()
                .expect("mock cancel request lock")
                .push(body.clone());
            let task_id = body
                .get("params")
                .and_then(|params| params.get("id"))
                .cloned()
                .unwrap_or_else(|| json!("mock-task-1"));
            let response = json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "id": task_id,
                    "status": {
                        "state": "TASK_STATE_CANCELED",
                        "message": {
                            "role": "agent",
                            "parts": [{"text": "cancelled"}],
                            "messageId": "mock-cancel-msg-1"
                        }
                    },
                    "metadata": {
                        "cancellation_reason": body
                            .get("params")
                            .and_then(|params| params.get("reason"))
                            .cloned()
                    }
                }
            });
            axum::Json(response).into_response()
        }
        _ => {
            let error = json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": -32601, "message": format!("method not found: {}", method) }
            });
            axum::Json(error).into_response()
        }
    }
}

fn encode_sse_events(events: &[StreamResponse]) -> String {
    let mut sse_body = String::new();
    for event in events {
        let event_name = event.event_name();
        let data = event.to_jsonrpc_data();
        sse_body.push_str(&format!("event: {}\ndata: {}\n\n", event_name, data));
    }
    sse_body
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mock_server_starts_and_responds() {
        let mut server = MockA2AServerBuilder::new().spawn().await;
        let client = reqwest::Client::new();

        let resp: reqwest::Response = client
            .post(server.url())
            .json(&json!({
                "jsonrpc": "2.0",
                "id": "test-1",
                "method": "SendMessage",
                "params": {}
            }))
            .send()
            .await
            .expect("send request");

        assert!(resp.status().is_success());
        let body: Value = resp.json().await.expect("parse json");
        assert_eq!(body["result"]["status"]["state"], "TASK_STATE_COMPLETED");

        server.shutdown();
    }

    #[tokio::test]
    async fn mock_server_streams_sse() {
        let mut server = MockA2AServerBuilder::new()
            .sse_events(vec![
                sse_status(TaskState::Working, Some("step 1")),
                sse_status(TaskState::Working, Some("step 2")),
                sse_status(TaskState::Completed, Some("done")),
            ])
            .spawn()
            .await;

        let client = reqwest::Client::new();
        let resp: reqwest::Response = client
            .post(server.url())
            .json(&json!({
                "jsonrpc": "2.0",
                "id": "test-2",
                "method": "SendStreamingMessage",
                "params": {}
            }))
            .send()
            .await
            .expect("send request");

        assert!(resp.status().is_success());
        let body: String = resp.text().await.expect("read body");
        assert!(body.contains("event: statusUpdate"));
        assert_eq!(body.matches("event: statusUpdate").count(), 3);

        server.shutdown();
    }
}
