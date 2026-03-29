//! Golden wire-format tests for A2A v1.0.0
//!
//! These tests verify that our Rust types serialize to JSON matching the
//! canonical A2A v1.0 wire format: camelCase fields, SCREAMING_SNAKE enums,
//! flat Part structs (no `kind` tag), and correct response wrappers.
//!
//! Run: `cargo test -p a2a_protocol_core --features all-features --test wire_format_v1_golden`

use a2a_protocol_core::{
    A2A_PROTOCOL_VERSION,
    agent::{AgentCapabilities, AgentCard, AgentInterface, AgentSkill},
    data::{
        message::{Message, MessageRole, Part},
        task::{Task, TaskState, TaskStatus},
    },
};
use serde_json::{Value, json};

fn normalize(val: &Value, skip: &[&str]) -> Value {
    match val {
        Value::Object(map) => {
            let mut out = serde_json::Map::new();
            for (k, v) in map {
                if skip.contains(&k.as_str()) {
                    continue;
                }
                out.insert(k.clone(), normalize(v, skip));
            }
            Value::Object(out)
        }
        Value::Array(arr) => Value::Array(arr.iter().map(|v| normalize(v, skip)).collect()),
        other => other.clone(),
    }
}

#[test]
fn protocol_version_is_1_0() {
    assert_eq!(A2A_PROTOCOL_VERSION, "1.0");
}

#[test]
fn message_role_screaming_snake() {
    assert_eq!(json!(MessageRole::User), json!("ROLE_USER"));
    assert_eq!(json!(MessageRole::Agent), json!("ROLE_AGENT"));
    assert_eq!(json!(MessageRole::Unspecified), json!("ROLE_UNSPECIFIED"));
}

#[test]
fn task_state_screaming_snake() {
    assert_eq!(json!(TaskState::Submitted), json!("TASK_STATE_SUBMITTED"));
    assert_eq!(json!(TaskState::Working), json!("TASK_STATE_WORKING"));
    assert_eq!(json!(TaskState::Completed), json!("TASK_STATE_COMPLETED"));
    assert_eq!(json!(TaskState::Failed), json!("TASK_STATE_FAILED"));
    assert_eq!(json!(TaskState::Canceled), json!("TASK_STATE_CANCELED"));
    assert_eq!(json!(TaskState::Rejected), json!("TASK_STATE_REJECTED"));
    assert_eq!(
        json!(TaskState::InputRequired),
        json!("TASK_STATE_INPUT_REQUIRED")
    );
    assert_eq!(
        json!(TaskState::AuthRequired),
        json!("TASK_STATE_AUTH_REQUIRED")
    );
    assert_eq!(
        json!(TaskState::Unspecified),
        json!("TASK_STATE_UNSPECIFIED")
    );
}

#[test]
fn part_flat_struct_no_kind_tag() {
    let text_part = Part::text("hello");
    let json = serde_json::to_value(&text_part).unwrap();

    assert!(
        json.get("kind").is_none(),
        "v1.0 Part must not have a 'kind' field"
    );
    assert_eq!(json["text"], "hello");

    let data_part = Part::data(json!({"key": "value"}));
    let json = serde_json::to_value(&data_part).unwrap();
    assert!(json.get("kind").is_none());
    assert_eq!(json["data"]["key"], "value");
}

#[test]
fn message_camel_case_fields() {
    let msg = Message::text(MessageRole::User, "hi", "task-1".to_string());
    let json = serde_json::to_value(&msg).unwrap();

    assert!(
        json.get("messageId").is_some(),
        "field must be camelCase: messageId"
    );
    assert!(
        json.get("taskId").is_some(),
        "field must be camelCase: taskId"
    );
    assert!(
        json.get("contextId").is_none() || json.get("contextId").unwrap().is_null() == false,
        "contextId should be absent when None (skip_serializing_if)"
    );
    assert_eq!(json["role"], "ROLE_USER");
    assert_eq!(json["parts"][0]["text"], "hi");
    assert!(
        json.get("kind").is_none(),
        "v1.0 Message must not have a 'kind' field"
    );
}

#[test]
fn message_with_context() {
    let msg = Message::text(MessageRole::Agent, "reply", "t-1".to_string())
        .with_context("ctx-1".to_string());
    let json = serde_json::to_value(&msg).unwrap();

    assert_eq!(json["contextId"], "ctx-1");
    assert_eq!(json["role"], "ROLE_AGENT");
}

#[test]
fn task_wire_format() {
    let mut task = Task::with_id("task-abc".to_string(), "ctx-xyz".to_string());
    task.update_status(TaskState::Working);
    task.add_to_history(Message::text(
        MessageRole::User,
        "input",
        "task-abc".to_string(),
    ));

    let json = serde_json::to_value(&task).unwrap();
    let norm = normalize(&json, &["timestamp"]);

    assert_eq!(norm["id"], "task-abc");
    assert_eq!(norm["contextId"], "ctx-xyz");
    assert_eq!(norm["status"]["state"], "TASK_STATE_WORKING");
    assert!(norm["history"].is_array());
    assert_eq!(norm["history"][0]["role"], "ROLE_USER");
}

#[test]
fn task_status_camel_case() {
    let status = TaskStatus::new(TaskState::Completed);
    let json = serde_json::to_value(&status).unwrap();

    assert_eq!(json["state"], "TASK_STATE_COMPLETED");
    assert!(
        json.get("kind").is_none(),
        "v1.0 TaskStatus must not have a 'kind' field"
    );
}

#[test]
fn agent_card_v1_fields() {
    let mut card = AgentCard::new("test-agent");
    card.description = Some("Test agent".to_string());
    card.version = Some("1.0.0".to_string());
    card.capabilities = Some(AgentCapabilities {
        streaming: true,
        push_notifications: false,
        extensions: None,
        extended_agent_card: false,
    });
    card.supported_interfaces = Some(vec![AgentInterface {
        url: "/jsonrpc".to_string(),
        protocol_binding: "JSONRPC".to_string(),
        tenant: None,
        protocol_version: Some("1.0".to_string()),
    }]);
    card.default_input_modes = Some(vec!["text/plain".to_string()]);
    card.default_output_modes = Some(vec!["text/plain".to_string()]);
    card = card.add_skill(AgentSkill {
        id: "echo".to_string(),
        name: "echo".to_string(),
        description: "Echoes messages".to_string(),
        tags: None,
        examples: None,
        input_modes: Some(vec!["text/plain".to_string()]),
        output_modes: Some(vec!["text/plain".to_string()]),
        security_requirements: None,
    });

    let json = serde_json::to_value(&card).unwrap();

    assert_eq!(json["name"], "test-agent");
    assert_eq!(json["capabilities"]["streaming"], true);
    assert_eq!(json["supportedInterfaces"][0]["protocolBinding"], "JSONRPC");
    assert_eq!(json["supportedInterfaces"][0]["protocolVersion"], "1.0");
    assert_eq!(json["defaultInputModes"][0], "text/plain");
    assert_eq!(json["defaultOutputModes"][0], "text/plain");
    assert_eq!(json["skills"][0]["id"], "echo");

    assert!(
        json.get("kind").is_none(),
        "AgentCard must not have a 'kind' field"
    );
}

#[test]
fn send_message_response_task_variant() {
    use a2a_protocol_core::methods::params::SendMessageResponse;

    let mut task = Task::with_id("t-1".to_string(), "c-1".to_string());
    task.update_status(TaskState::Completed);
    let resp = SendMessageResponse::Task(task);
    let json = serde_json::to_value(&resp).unwrap();

    assert!(
        json.get("task").is_some(),
        "Task variant must be wrapped under 'task' key"
    );
    assert_eq!(json["task"]["id"], "t-1");
    assert_eq!(json["task"]["contextId"], "c-1");
    assert_eq!(json["task"]["status"]["state"], "TASK_STATE_COMPLETED");
}

#[test]
fn send_message_response_message_variant() {
    use a2a_protocol_core::methods::params::SendMessageResponse;

    let msg = Message::text(MessageRole::Agent, "reply", "t-1".to_string());
    let resp = SendMessageResponse::Message(msg);
    let json = serde_json::to_value(&resp).unwrap();

    assert!(
        json.get("message").is_some(),
        "Message variant must be wrapped under 'message' key"
    );
    assert_eq!(json["message"]["role"], "ROLE_AGENT");
    assert_eq!(json["message"]["parts"][0]["text"], "reply");
}

#[test]
fn send_message_response_roundtrip() {
    use a2a_protocol_core::methods::params::SendMessageResponse;

    let msg = Message::text(MessageRole::Agent, "hello", "t-1".to_string());
    let original = SendMessageResponse::Message(msg);
    let json_str = serde_json::to_string(&original).unwrap();
    let roundtripped: SendMessageResponse = serde_json::from_str(&json_str).unwrap();
    match roundtripped {
        SendMessageResponse::Message(m) => assert_eq!(m.get_text_content(), "hello"),
        _ => panic!("expected Message variant after roundtrip"),
    }

    let mut task = Task::with_id("t-2".to_string(), "c-2".to_string());
    task.update_status(TaskState::Working);
    let original = SendMessageResponse::Task(task);
    let json_str = serde_json::to_string(&original).unwrap();
    let roundtripped: SendMessageResponse = serde_json::from_str(&json_str).unwrap();
    match roundtripped {
        SendMessageResponse::Task(t) => assert_eq!(t.id, "t-2"),
        _ => panic!("expected Task variant after roundtrip"),
    }
}

#[test]
fn send_message_request_roundtrip() {
    use a2a_protocol_core::methods::params::SendMessageRequest;

    let raw = json!({
        "message": {
            "messageId": "msg-1",
            "role": "ROLE_USER",
            "parts": [{"text": "hello"}]
        }
    });
    let parsed: SendMessageRequest = serde_json::from_value(raw.clone()).unwrap();
    assert_eq!(parsed.message.role, MessageRole::User);
    assert_eq!(parsed.message.get_text_content(), "hello");
    assert!(parsed.tenant.is_none());
    assert!(parsed.configuration.is_none());
}

#[test]
fn send_message_request_with_tenant_and_config() {
    use a2a_protocol_core::methods::params::SendMessageRequest;

    let raw = json!({
        "message": {
            "messageId": "msg-2",
            "role": "ROLE_USER",
            "parts": [{"text": "hi"}]
        },
        "tenant": "acme-corp",
        "configuration": {
            "acceptedOutputModes": ["text/plain"],
            "historyLength": 5
        }
    });
    let parsed: SendMessageRequest = serde_json::from_value(raw).unwrap();
    assert_eq!(parsed.tenant.as_deref(), Some("acme-corp"));
    assert!(parsed.configuration.is_some());
    let cfg = parsed.configuration.unwrap();
    assert_eq!(
        cfg.accepted_output_modes.as_deref(),
        Some(vec!["text/plain".to_string()].as_slice())
    );
    assert_eq!(cfg.history_length, Some(5));
}

#[test]
fn enum_deserialize_from_wire() {
    let role: MessageRole = serde_json::from_str(r#""ROLE_USER""#).unwrap();
    assert_eq!(role, MessageRole::User);

    let state: TaskState = serde_json::from_str(r#""TASK_STATE_COMPLETED""#).unwrap();
    assert_eq!(state, TaskState::Completed);
}

#[test]
fn part_with_metadata_roundtrip() {
    let raw = json!({
        "text": "hello",
        "metadata": {"source": "test"}
    });
    let part: Part = serde_json::from_value(raw).unwrap();
    assert_eq!(part.text.as_deref(), Some("hello"));
    assert_eq!(part.metadata.as_ref().unwrap()["source"], "test");

    let back = serde_json::to_value(&part).unwrap();
    assert_eq!(back["metadata"]["source"], "test");
}
