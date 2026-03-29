#![cfg(all(
    not(target_arch = "wasm32"),
    feature = "a2a-client",
    feature = "a2a-server"
))]

use a2a_protocol_core::data::{Message, MessageRole, Part};
use agent_core::{ContentPart, TaskPhase};
use agent_sdk::{
    Agent, SdkError,
    a2a::A2aClient,
    a2a::A2aServer,
    agent::{Response, RuntimeArtifact, TaskOpts},
};
use serde_json::{Value, json};
use tokio::net::TcpListener;

fn extract_task(value: &Value) -> &Value {
    value.get("task").unwrap_or(value)
}

fn task_artifact_count(value: &Value) -> usize {
    extract_task(value)
        .get("artifacts")
        .and_then(Value::as_array)
        .map(|artifacts| artifacts.len())
        .unwrap_or(0)
}

fn extract_task_id(value: &Value) -> Option<String> {
    extract_task(value)
        .get("id")
        .and_then(Value::as_str)
        .map(ToString::to_string)
}

async fn spawn_sdk_skill_server() -> (
    String,
    tokio::task::JoinHandle<()>,
    tokio::sync::oneshot::Sender<()>,
) {
    let mut agent = Agent::new_runtime("artifact-roundtrip-agent").expect("agent");
    agent
        .skill("lookup", |params| async move {
            Ok(json!({
                "payload": params,
                "summary": "lookup complete",
            }))
        })
        .register()
        .expect("register lookup skill");
    let skill_registry = agent.skill_registry().clone();
    agent.set_message_handler(move |msg_ctx, task_ctx| {
        let skill_registry = skill_registry.clone();
        async move {
            if let Some(skill_call) = &msg_ctx.skill_call {
                let result = skill_registry
                    .execute_skill(&skill_call.skill_id, &skill_call.parameters)
                    .await
                    .map_err(|error| SdkError::method_execution("test_skill", &error))?;
                return Response::task(
                    TaskOpts {
                        artifacts: vec![RuntimeArtifact::data(
                            format!("{}_result", skill_call.skill_id),
                            result,
                        )],
                        state: Some(TaskPhase::Completed),
                        status_text: Some(format!("skill {} complete", skill_call.skill_id)),
                        history_parts: Some(vec![ContentPart::Text(format!(
                            "skill {} executed",
                            skill_call.skill_id
                        ))]),
                        task_meta: None,
                    },
                    &msg_ctx,
                    task_ctx,
                );
            }

            Response::message_text("no skill", None, None, None)
        }
    });

    let router = A2aServer::with_a2a_methods(agent)
        .expect("sdk server")
        .build_router();

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind listener");
    let addr = listener.local_addr().expect("listener addr");
    let endpoint = format!("http://{addr}/jsonrpc");
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let handle = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx.await;
            })
            .await
            .expect("serve sdk skill router");
    });

    (endpoint, handle, shutdown_tx)
}

#[tokio::test]
async fn test_message_send_reused_task_returns_delta_artifacts_while_get_task_returns_full_artifacts()
 {
    let (endpoint, handle, shutdown_tx) = spawn_sdk_skill_server().await;
    let client = A2aClient::direct(&endpoint).expect("create direct client");

    let requested_task_id = "explicit-artifact-task".to_string();
    let context_id = "explicit-artifact-ctx".to_string();

    let first = client
        .message_send(
            Message {
                role: MessageRole::User,
                parts: vec![Part::data(json!({"skill": "lookup", "query": "first"}))],
                message_id: "msg-artifact-1".to_string(),
                task_id: Some(requested_task_id.clone()),
                context_id: Some(context_id.clone()),
                metadata: None,
                extensions: None,
                reference_task_ids: None,
            },
            None,
        )
        .await
        .expect("first message_send should succeed");
    assert_eq!(
        task_artifact_count(&first),
        1,
        "first send should return one delta artifact"
    );
    let task_id = extract_task_id(&first).expect("first send should return task id");

    let second = client
        .message_send(
            Message {
                role: MessageRole::User,
                parts: vec![Part::data(json!({"skill": "lookup", "query": "second"}))],
                message_id: "msg-artifact-2".to_string(),
                task_id: Some(task_id.clone()),
                context_id: Some(context_id.clone()),
                metadata: None,
                extensions: None,
                reference_task_ids: None,
            },
            None,
        )
        .await
        .expect("second message_send should succeed");
    assert_eq!(
        extract_task_id(&second).as_deref(),
        Some(task_id.as_str()),
        "explicit reused task updates should continue the same canonical task"
    );
    assert_eq!(
        task_artifact_count(&second),
        1,
        "reused task send should stay delta-shaped and not return prior artifacts"
    );

    let task = client
        .get_task(&task_id)
        .await
        .expect("get_task should return full canonical task");
    assert_eq!(
        task.artifacts.as_ref().map(|artifacts| artifacts.len()),
        Some(2),
        "canonical get_task should include both accumulated artifacts"
    );

    let _ = shutdown_tx.send(());
    let _ = handle.await;
}
