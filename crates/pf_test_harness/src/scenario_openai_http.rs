//! OpenAI Chat Completions **wire** fixtures + local HTTP mock for testing [`llm_client::LlmClient`]
//! (and any HTTP client) against [`crate::scenario::LlmScenario`] turns.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::http::StatusCode;
use axum::http::header::CONTENT_TYPE;
use axum::routing::post;
use llm_client::LlmResponse;
use llm_client::stream::StreamEvent;
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

use crate::scenario::{LlmScenario, fold_stream_events_to_llm_response};

#[derive(Clone)]
struct MockState {
    turns: Arc<Mutex<VecDeque<Vec<StreamEvent>>>>,
}

impl MockState {
    fn pop_turn(&self) -> Vec<StreamEvent> {
        self.turns
            .lock()
            .expect("mock lock poisoned")
            .pop_front()
            .unwrap_or_default()
    }
}

/// Turn a folded [`LlmResponse`] into a minimal non-streaming OpenAI Chat Completions JSON body.
pub fn openai_chat_completion_json(resp: &LlmResponse) -> Value {
    let choice = resp
        .choices
        .first()
        .expect("scenario response must have one choice");
    let mut message = json!({
        "role": choice.message.role,
    });
    if let Some(obj) = message.as_object_mut() {
        match &choice.message.content {
            Some(c) => {
                obj.insert("content".into(), Value::String(c.clone()));
            }
            None => {
                obj.insert("content".into(), Value::Null);
            }
        }
        if let Some(tcs) = &choice.message.tool_calls {
            let arr: Vec<Value> = tcs
                .iter()
                .map(|tc| {
                    let args = serde_json::to_string(&tc.arguments).unwrap_or_else(|_| "{}".into());
                    json!({
                        "id": tc.id,
                        "type": "function",
                        "function": {
                            "name": tc.name,
                            "arguments": args,
                        }
                    })
                })
                .collect();
            obj.insert("tool_calls".into(), Value::Array(arr));
        }
    }

    let mut out = json!({
        "id": resp.id.as_deref().unwrap_or("mockcmpl"),
        "object": "chat.completion",
        "model": resp.model.as_deref().unwrap_or("gpt-4"),
        "choices": [{
            "index": 0,
            "message": message,
            "finish_reason": choice.finish_reason,
        }],
    });
    if let Some(u) = &resp.usage {
        out.as_object_mut()
            .expect("object")
            .insert("usage".into(), serde_json::to_value(u).unwrap_or(json!({})));
    }
    out
}

/// Encode one scenario turn as OpenAI-style **streaming** `data:` lines + trailing `data: [DONE]`.
pub fn openai_chat_sse_lines(turn: &[StreamEvent]) -> Result<String, String> {
    let mut out = String::new();
    let mut id = "mockcmpl".to_string();
    let mut model = "gpt-4".to_string();

    for event in turn {
        match event {
            StreamEvent::StreamStart { id: sid, model: sm } => {
                if let Some(s) = sid {
                    id = s.clone();
                }
                if let Some(m) = sm {
                    model = m.clone();
                }
                let line = json!({
                    "id": id,
                    "model": model,
                    "choices": [{
                        "index": 0,
                        "delta": { "role": "assistant" },
                        "finish_reason": null
                    }]
                });
                append_sse_data(&mut out, &line);
            }
            StreamEvent::ContentDelta { delta } => {
                let line = json!({
                    "id": id,
                    "model": model,
                    "choices": [{
                        "index": 0,
                        "delta": { "content": delta },
                        "finish_reason": null
                    }]
                });
                append_sse_data(&mut out, &line);
            }
            StreamEvent::ReasoningDelta { delta } => {
                let line = json!({
                    "id": id,
                    "model": model,
                    "choices": [{
                        "index": 0,
                        "delta": { "reasoning_content": delta },
                        "finish_reason": null
                    }]
                });
                append_sse_data(&mut out, &line);
            }
            StreamEvent::ToolCallStart {
                index,
                id: call_id,
                name,
            } => {
                let line = json!({
                    "id": id,
                    "model": model,
                    "choices": [{
                        "index": 0,
                        "delta": {
                            "tool_calls": [{
                                "index": index,
                                "id": call_id,
                                "type": "function",
                                "function": { "name": name, "arguments": "" }
                            }]
                        },
                        "finish_reason": null
                    }]
                });
                append_sse_data(&mut out, &line);
            }
            StreamEvent::ToolCallDelta {
                index,
                arguments_delta,
            } => {
                let line = json!({
                    "id": id,
                    "model": model,
                    "choices": [{
                        "index": 0,
                        "delta": {
                            "tool_calls": [{
                                "index": index,
                                "function": { "arguments": arguments_delta }
                            }]
                        },
                        "finish_reason": null
                    }]
                });
                append_sse_data(&mut out, &line);
            }
            StreamEvent::Done {
                finish_reason,
                usage,
            } => {
                let choice = json!({
                    "index": 0,
                    "delta": {},
                    "finish_reason": finish_reason,
                });
                let mut line = json!({
                    "id": id,
                    "model": model,
                    "choices": [choice],
                });
                if let Some(u) = usage {
                    line.as_object_mut()
                        .expect("object")
                        .insert("usage".into(), serde_json::to_value(u).unwrap_or(json!({})));
                }
                append_sse_data(&mut out, &line);
            }
            StreamEvent::Error { message } => return Err(message.clone()),
        }
    }
    out.push_str("data: [DONE]\n\n");
    Ok(out)
}

fn append_sse_data(out: &mut String, value: &Value) {
    out.push_str("data: ");
    out.push_str(&value.to_string());
    out.push_str("\n\n");
}

async fn chat_completions(
    State(state): State<MockState>,
    body: axum::body::Bytes,
) -> Result<axum::response::Response, StatusCode> {
    let payload: Value = serde_json::from_slice(&body).map_err(|_| StatusCode::BAD_REQUEST)?;
    let stream = payload
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let turn = state.pop_turn();
    if turn.is_empty() {
        return Err(StatusCode::NOT_FOUND);
    }

    if stream {
        let sse = openai_chat_sse_lines(&turn).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        axum::response::Response::builder()
            .status(StatusCode::OK)
            .header(CONTENT_TYPE, "text/event-stream; charset=utf-8")
            .body(Body::from(sse))
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
    } else {
        let resp = fold_stream_events_to_llm_response(&turn)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let json = openai_chat_completion_json(&resp);
        axum::response::Response::builder()
            .status(StatusCode::OK)
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(json.to_string()))
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
    }
}

/// Local `/v1/chat/completions` mock: each request consumes one scenario turn (non-stream or stream).
#[derive(Clone)]
pub struct OpenAiScenarioMock {
    state: MockState,
}

impl OpenAiScenarioMock {
    pub fn new(scenario: LlmScenario) -> Self {
        let turns: VecDeque<Vec<StreamEvent>> = scenario.turns().iter().cloned().collect();
        Self {
            state: MockState {
                turns: Arc::new(Mutex::new(turns)),
            },
        }
    }

    /// `POST` base URL for [`llm_client::client::LlmClientBuilder::base_url`], e.g. `http://127.0.0.1:PORT` (no trailing path).
    pub async fn spawn(self) -> (String, JoinHandle<()>) {
        let app = Router::new()
            .route("/v1/chat/completions", post(chat_completions))
            .with_state(self.state);

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock listener");
        let addr = listener.local_addr().expect("local_addr");
        let handle = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("mock serve");
        });
        (format!("http://{addr}"), handle)
    }
}
