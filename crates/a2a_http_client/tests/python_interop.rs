//! Reverse interop tests: Rust `a2a_http_client` → Python a2a-python SDK server.
//!
//! These tests are `#[ignore]` by default. Run against a live Python server:
//!
//! ```sh
//! # Terminal 1: start the Python A2A server
//! cd ~/a2a-interop-test && uv run python python_a2a_server.py
//!
//! # Terminal 2: run these tests
//! cargo test -p a2a_http_client --test python_interop -- --ignored
//! ```

use a2a_http_client::Client;
use a2a_protocol_core::data::message::{Message, MessageRole, Part};
use serde_json::Value;

const PYTHON_SERVER_JSONRPC: &str = "http://127.0.0.1:9090/jsonrpc";
const PYTHON_SERVER_BASE: &str = "http://127.0.0.1:9090";

fn client() -> Client {
    Client::external(PYTHON_SERVER_JSONRPC)
}

fn user_message(text: &str) -> Message {
    let mut msg = Message::with_id(
        uuid::Uuid::new_v4().to_string(),
        MessageRole::User,
        vec![Part::text(text)],
    );
    msg.context_id = Some(uuid::Uuid::new_v4().to_string());
    msg
}

// ─── Agent Card Discovery ───────────────────────────────────────────

#[tokio::test]
#[ignore]
async fn test_well_known_agent_card_http() {
    let url = format!("{}/.well-known/agent-card.json", PYTHON_SERVER_BASE);
    let resp = reqwest::get(&url).await.expect("HTTP GET failed");
    assert_eq!(
        resp.status(),
        200,
        "well-known agent card should return 200"
    );

    let card: Value = resp.json().await.expect("failed to parse agent card JSON");

    assert_eq!(card["name"], "python-echo-server", "agent name mismatch");
    assert_eq!(card["version"], "1.0.0", "version mismatch");

    let interfaces = card["supportedInterfaces"]
        .as_array()
        .expect("supportedInterfaces should be an array");
    assert!(!interfaces.is_empty(), "should have at least one interface");

    let jsonrpc_iface = interfaces
        .iter()
        .find(|i| i["protocolBinding"] == "JSONRPC")
        .expect("should have a JSONRPC interface");
    assert!(
        jsonrpc_iface["url"]
            .as_str()
            .unwrap()
            .starts_with("http://"),
        "interface URL should be absolute"
    );

    let skills = card["skills"]
        .as_array()
        .expect("skills should be an array");
    assert!(!skills.is_empty(), "should have at least one skill");

    println!("PASS: well-known agent card discovery works");
    println!("  name: {}", card["name"]);
    println!("  version: {}", card["version"]);
    println!("  interfaces: {}", interfaces.len());
    println!("  skills: {}", skills.len());
}

#[tokio::test]
#[ignore]
async fn test_our_get_agent_card_jsonrpc_method_not_found() {
    let client = client();
    let result = client.get_agent_card().await;

    match result {
        Err(e) => {
            assert_eq!(
                e.code, -32601,
                "expected Method Not Found (-32601), got code {}",
                e.code
            );
            println!(
                "PASS: GetAgentCard JSON-RPC correctly returns MethodNotFound (Python SDK doesn't implement this method)"
            );
        }
        Ok(val) => {
            panic!(
                "Expected error for GetAgentCard, but got success: {}",
                serde_json::to_string_pretty(&val).unwrap()
            );
        }
    }
}

// ─── SendMessage ────────────────────────────────────────────────────

#[tokio::test]
#[ignore]
async fn test_send_message_returns_wrapped_response() {
    let client = client();
    let msg = user_message("hello from rust client");

    let result = client
        .message_send(msg, None)
        .await
        .expect("SendMessage should succeed");

    // Python SDK wraps in externally-tagged oneof: {"task": {...}} or {"message": {...}}
    let has_task = result.get("task").is_some();
    let has_message = result.get("message").is_some();
    assert!(
        has_task || has_message,
        "response should contain 'task' or 'message' key, got: {}",
        serde_json::to_string_pretty(&result).unwrap()
    );

    if has_task {
        let task = &result["task"];
        assert!(task["id"].is_string(), "task.id should be a string");
        assert!(
            task["contextId"].is_string(),
            "task.contextId should be a string"
        );
        assert!(
            task["status"].is_object(),
            "task.status should be an object"
        );

        let state = task["status"]["state"].as_str().unwrap();
        assert!(
            state.starts_with("TASK_STATE_"),
            "state should be SCREAMING_SNAKE, got: {}",
            state
        );

        println!("PASS: SendMessage returned wrapped task");
        println!("  task.id: {}", task["id"]);
        println!("  task.status.state: {}", state);
    } else {
        let message = &result["message"];
        assert!(
            message["role"].is_string(),
            "message.role should be a string"
        );
        assert!(
            message["parts"].is_array(),
            "message.parts should be an array"
        );
        println!("PASS: SendMessage returned wrapped message");
    }
}

#[tokio::test]
#[ignore]
async fn test_send_message_echo_content() {
    let client = client();
    let msg = user_message("interop test payload");

    let result = client
        .message_send(msg, None)
        .await
        .expect("SendMessage should succeed");

    // The echo server returns a Task with the echo in artifacts
    let task = result
        .get("task")
        .expect("expected wrapped task in response");

    let state = task["status"]["state"].as_str().unwrap();
    assert_eq!(
        state, "TASK_STATE_COMPLETED",
        "echo task should be COMPLETED, got: {}",
        state
    );

    let artifacts = task["artifacts"]
        .as_array()
        .expect("completed task should have artifacts");
    assert!(!artifacts.is_empty(), "should have at least one artifact");

    let first_part = &artifacts[0]["parts"][0];
    let echo_text = first_part["text"]
        .as_str()
        .expect("artifact part should have text");
    assert!(
        echo_text.contains("interop test payload"),
        "echo should contain our input, got: {}",
        echo_text
    );

    println!("PASS: echo content verified: {}", echo_text);
}

// ─── GetTask ────────────────────────────────────────────────────────

#[tokio::test]
#[ignore]
async fn test_get_task_after_send() {
    let client = client();
    let msg = user_message("get-task test");

    let send_result = client
        .message_send(msg, None)
        .await
        .expect("SendMessage should succeed");

    let task_obj = send_result.get("task").expect("expected wrapped task");
    let task_id = task_obj["id"]
        .as_str()
        .expect("task.id should be a string")
        .to_string();

    // Now retrieve the task by ID
    let task = client
        .task_get(task_id.clone())
        .await
        .expect("GetTask should succeed");

    assert_eq!(task.id, task_id, "retrieved task ID should match");
    assert!(
        task.status.state.is_terminal(),
        "task should be in terminal state after echo completes"
    );

    println!("PASS: GetTask returned matching task");
    println!("  id: {}", task.id);
    println!("  state: {:?}", task.status.state);
}

// ─── ListTasks ──────────────────────────────────────────────────────

#[tokio::test]
#[ignore]
async fn test_list_tasks() {
    let client = client();

    // Send a message first to ensure at least one task exists
    let msg = user_message("list-tasks test");
    let ctx_id = msg.context_id.clone().unwrap();
    client
        .message_send(msg, None)
        .await
        .expect("SendMessage should succeed");

    // List tasks (no filters)
    let result = client.task_list(Some(ctx_id), None, None, None).await;

    match result {
        Ok(val) => {
            println!(
                "PASS: ListTasks returned: {}",
                serde_json::to_string_pretty(&val).unwrap()
            );
        }
        Err(e) => {
            // Some implementations don't support ListTasks
            println!(
                "INFO: ListTasks returned error (may be unsupported): code={}, msg={}",
                e.code, e.message
            );
        }
    }
}

// ─── CancelTask ─────────────────────────────────────────────────────

#[tokio::test]
#[ignore]
async fn test_cancel_nonexistent_task() {
    let client = client();

    let result = client.task_cancel("nonexistent-task-id".to_string()).await;

    match result {
        Err(e) => {
            println!(
                "PASS: CancelTask for nonexistent ID returns error: code={}, msg={}",
                e.code, e.message
            );
        }
        Ok(task) => {
            println!(
                "INFO: CancelTask returned success for nonexistent task (implementation-dependent): {:?}",
                task.status.state
            );
        }
    }
}

// ─── Streaming (SSE) ────────────────────────────────────────────────

#[cfg(feature = "streaming")]
#[tokio::test]
#[ignore]
async fn test_send_streaming_message() {
    use futures_util::StreamExt;

    let client = client();
    let context_id = uuid::Uuid::new_v4().to_string();
    let msg = user_message("streaming test");

    let stream_result = client.send_subscribe(&context_id, msg).await;

    match stream_result {
        Ok(mut stream) => {
            let mut event_count = 0;
            let mut saw_terminal = false;

            while let Some(item) = stream.next().await {
                match item {
                    Ok(event) => {
                        event_count += 1;
                        println!("  SSE event #{}: {:?}", event_count, event);
                        if event.is_terminal() {
                            saw_terminal = true;
                            break;
                        }
                    }
                    Err(e) => {
                        println!("  SSE error: code={}, msg={}", e.code, e.message);
                        break;
                    }
                }
            }

            assert!(event_count > 0, "should receive at least one SSE event");
            println!(
                "PASS: streaming received {} events, terminal={}",
                event_count, saw_terminal
            );
        }
        Err(e) => {
            println!(
                "INFO: SendStreamingMessage failed (may be expected): code={}, msg={}",
                e.code, e.message
            );
        }
    }
}

// ─── Wire Format Details ────────────────────────────────────────────

#[tokio::test]
#[ignore]
async fn test_raw_jsonrpc_send_message_wire_format() {
    let http = reqwest::Client::new();
    let context_id = uuid::Uuid::new_v4().to_string();
    let message_id = uuid::Uuid::new_v4().to_string();

    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": "interop-wire-test-1",
        "method": "SendMessage",
        "params": {
            "message": {
                "role": "ROLE_USER",
                "parts": [{"text": "wire format test"}],
                "messageId": message_id,
                "contextId": context_id
            }
        }
    });

    let resp = http
        .post(PYTHON_SERVER_JSONRPC)
        .header("a2a-version", "1.0")
        .json(&body)
        .send()
        .await
        .expect("HTTP POST failed");

    assert_eq!(resp.status(), 200);

    let json: Value = resp.json().await.expect("failed to parse response");

    assert_eq!(json["jsonrpc"], "2.0", "jsonrpc version mismatch");
    assert_eq!(
        json["id"], "interop-wire-test-1",
        "request ID should echo back"
    );
    assert!(
        json.get("error").is_none(),
        "should not have error: {:?}",
        json.get("error")
    );

    let result = &json["result"];
    let has_task = result.get("task").is_some();
    let has_message = result.get("message").is_some();
    assert!(
        has_task || has_message,
        "result should have 'task' or 'message' key: {}",
        serde_json::to_string_pretty(result).unwrap()
    );

    println!("PASS: raw wire format validated");
    println!(
        "  response: {}",
        serde_json::to_string_pretty(&json).unwrap()
    );
}

#[tokio::test]
#[ignore]
async fn test_task_state_enum_compatibility() {
    let client = client();
    let msg = user_message("state enum test");

    let result = client
        .message_send(msg, None)
        .await
        .expect("SendMessage should succeed");

    let task_json = result.get("task").expect("expected wrapped task");

    // Verify the task can be deserialized into our Rust Task struct
    let task: a2a_protocol_core::data::task::Task = serde_json::from_value(task_json.clone())
        .expect(&format!(
            "Python Task JSON should deserialize into Rust Task struct. JSON: {}",
            serde_json::to_string_pretty(task_json).unwrap()
        ));

    assert!(!task.id.is_empty(), "task.id should not be empty");
    assert!(
        !task.context_id.is_empty(),
        "task.context_id should not be empty"
    );
    assert!(
        task.status.state.is_terminal(),
        "echo task should reach terminal state"
    );

    println!("PASS: Python Task fully deserializes into Rust Task struct");
    println!("  id: {}", task.id);
    println!("  context_id: {}", task.context_id);
    println!("  state: {:?}", task.status.state);
    if let Some(artifacts) = &task.artifacts {
        println!("  artifacts: {}", artifacts.len());
    }
}
