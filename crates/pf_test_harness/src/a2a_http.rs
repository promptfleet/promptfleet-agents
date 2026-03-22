use std::time::Duration;

use axum::body::Body;
use http::{HeaderMap, Request, StatusCode};
use serde_json::{json, Value};
use tower::ServiceExt;

use crate::sse::{SseCapture, SseCollector};

#[derive(Debug, Clone)]
pub struct JsonRpcCapture {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body_raw: String,
    pub body_json: Option<Value>,
}

/// Quick convenience: build a `SendStreamingMessage` JSON-RPC body with defaults.
pub fn send_subscribe_body(text: &str) -> String {
    SendSubscribeRequest::new(text).to_json()
}

/// Builder for A2A `SendStreamingMessage` JSON-RPC requests.
pub struct SendSubscribeRequest<'a> {
    text: &'a str,
    task_id: Option<&'a str>,
    message_id: &'a str,
    jsonrpc_id: &'a str,
}

impl<'a> SendSubscribeRequest<'a> {
    pub fn new(text: &'a str) -> Self {
        Self {
            text,
            task_id: None,
            message_id: "msg-u1",
            jsonrpc_id: "sub-1",
        }
    }

    pub fn with_task_id(mut self, task_id: &'a str) -> Self {
        self.task_id = Some(task_id);
        self
    }

    pub fn with_message_id(mut self, message_id: &'a str) -> Self {
        self.message_id = message_id;
        self
    }

    pub fn with_jsonrpc_id(mut self, jsonrpc_id: &'a str) -> Self {
        self.jsonrpc_id = jsonrpc_id;
        self
    }

    pub fn to_json(&self) -> String {
        let mut msg = json!({
            "role": "ROLE_USER",
            "parts": [{"text": self.text}],
            "messageId": self.message_id
        });
        if let Some(tid) = self.task_id {
            msg["taskId"] = json!(tid);
        }
        json!({
            "jsonrpc": "2.0",
            "id": self.jsonrpc_id,
            "method": "SendStreamingMessage",
            "params": { "message": msg }
        })
        .to_string()
    }
}

pub fn jsonrpc_body(id: Value, method: &str, params: Value) -> String {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params
    })
    .to_string()
}

pub fn message_send_body(text: &str) -> String {
    jsonrpc_body(
        json!("msg-1"),
        "SendMessage",
        json!({
            "message": {
                "role": "ROLE_USER",
                "parts": [{"text": text}],
                "messageId": "msg-u1"
            }
        }),
    )
}

pub fn tasks_get_body(task_id: &str, _include_history: bool, _include_artifacts: bool) -> String {
    jsonrpc_body(
        json!("get-1"),
        "GetTask",
        json!({
            "id": task_id
        }),
    )
}

pub fn tasks_cancel_body(task_id: &str, reason: Option<&str>) -> String {
    let mut params = json!({ "id": task_id });
    if let Some(reason) = reason {
        params["metadata"] = json!({ "cancellation_reason": reason });
    }
    jsonrpc_body(json!("cancel-1"), "CancelTask", params)
}

pub fn tasks_list_body(
    limit: Option<usize>,
    _offset: Option<usize>,
    state_filter: Option<&str>,
    context_filter: Option<&str>,
) -> String {
    let mut params = serde_json::Map::new();
    if let Some(limit) = limit {
        params.insert("pageSize".to_string(), Value::from(limit));
    }
    if let Some(state) = state_filter {
        params.insert("status".to_string(), Value::String(state.to_string()));
    }
    if let Some(context) = context_filter {
        params.insert("contextId".to_string(), Value::String(context.to_string()));
    }
    jsonrpc_body(json!("list-1"), "ListTasks", Value::Object(params))
}

pub async fn call_jsonrpc(router: axum::Router, body: String) -> Result<JsonRpcCapture, String> {
    let request = Request::builder()
        .method("POST")
        .uri("/jsonrpc")
        .header("content-type", "application/json")
        .body(Body::from(body))
        .map_err(|e| format!("failed to build request: {e}"))?;

    let response = router
        .oneshot(request)
        .await
        .map_err(|e| format!("router oneshot failed: {e}"))?;

    let (parts, body) = response.into_parts();
    let bytes = http_body_util::BodyExt::collect(body)
        .await
        .map_err(|e| format!("failed to collect JSON-RPC response body: {e}"))?
        .to_bytes();
    let body_raw = String::from_utf8_lossy(&bytes).to_string();
    let body_json = serde_json::from_slice(&bytes).ok();

    Ok(JsonRpcCapture {
        status: parts.status,
        headers: parts.headers,
        body_raw,
        body_json,
    })
}

pub async fn collect_send_subscribe_sse(
    router: axum::Router,
    body: String,
) -> Result<SseCapture, String> {
    let request = Request::builder()
        .method("POST")
        .uri("/jsonrpc")
        .header("content-type", "application/json")
        .body(Body::from(body))
        .map_err(|e| format!("failed to build request: {e}"))?;

    let response = router
        .oneshot(request)
        .await
        .map_err(|e| format!("router oneshot failed: {e}"))?;

    SseCollector::from_response(response)
        .with_timeout(Duration::from_secs(5))
        .collect_all()
        .await
}
