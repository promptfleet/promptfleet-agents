use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use a2a_http_client::Client as HttpClient;
use agent_core::{ContentPart, TaskPhase};
use agent_sdk::a2a::{
    A2aClient, A2aServer, Message, MessageRole, MessageSendParams, Part, StreamResponse, TaskState,
};
use agent_sdk::agent::{Response, RuntimeArtifact, TaskOpts};
use agent_sdk::{Agent, SdkError};
use criterion::{criterion_group, criterion_main, Criterion};
use futures_util::StreamExt;
use pf_test_harness::a2a_http::{
    call_jsonrpc, collect_send_subscribe_sse, jsonrpc_body, tasks_get_body,
};
use pf_test_harness::a2a_mock::{sse_status, MockA2AServerBuilder};
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio::runtime::{Builder, Runtime};
use tokio::sync::oneshot;

fn runtime() -> Runtime {
    Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("create benchmark runtime")
}

#[derive(Copy, Clone)]
enum TextResponseMode {
    Message,
    Task,
}

#[derive(Copy, Clone)]
enum SkillResponseMode {
    StatusOnlyTask,
    ArtifactTask,
}

fn build_skill_agent(
    name: &str,
    text_mode: TextResponseMode,
    skill_mode: SkillResponseMode,
) -> Agent {
    let mut agent = Agent::new_runtime(name).expect("create benchmark agent");
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
                    .map_err(|error| SdkError::method_execution("bench_skill", &error))?;
                let artifacts = match skill_mode {
                    SkillResponseMode::StatusOnlyTask => Vec::new(),
                    SkillResponseMode::ArtifactTask => vec![RuntimeArtifact::data(
                        format!("{}_result", skill_call.skill_id),
                        result,
                    )],
                };
                return Response::task(
                    TaskOpts {
                        artifacts,
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

            let text = msg_ctx
                .text_content
                .clone()
                .unwrap_or_else(|| "no text".to_string());
            match text_mode {
                TextResponseMode::Message => Response::message_text(
                    format!("echo: {text}"),
                    None,
                    None,
                    task_ctx.as_ref().and_then(|ctx| ctx.context_id.clone()),
                ),
                TextResponseMode::Task => Response::task(
                    TaskOpts {
                        artifacts: vec![],
                        state: Some(TaskPhase::Completed),
                        status_text: Some("message processed".to_string()),
                        history_parts: Some(vec![ContentPart::Text(format!("echo: {text}"))]),
                        task_meta: None,
                    },
                    &msg_ctx,
                    task_ctx,
                ),
            }
        }
    });
    agent
}

fn message_send_skill_body(skill_id: &str, params: Value) -> String {
    let mut data = serde_json::Map::new();
    data.insert("skill".to_string(), Value::String(skill_id.to_string()));
    if let Value::Object(values) = params {
        for (key, value) in values {
            data.insert(key, value);
        }
    }

    jsonrpc_body(
        json!("bench-msg-1"),
        "SendMessage",
        json!({
            "message": {
                "role": "ROLE_USER",
                "parts": [{"data": Value::Object(data)}],
                "messageId": "bench-msg-u1"
            }
        }),
    )
}

fn send_subscribe_skill_body(skill_id: &str, params: Value) -> String {
    let mut data = serde_json::Map::new();
    data.insert("skill".to_string(), Value::String(skill_id.to_string()));
    if let Value::Object(values) = params {
        for (key, value) in values {
            data.insert(key, value);
        }
    }

    jsonrpc_body(
        json!("bench-sub-1"),
        "SendStreamingMessage",
        json!({
            "message": {
                "role": "ROLE_USER",
                "parts": [{"data": Value::Object(data)}],
                "messageId": "bench-stream-u1"
            }
        }),
    )
}

struct ServerHarness {
    endpoint: String,
    _shutdown_tx: oneshot::Sender<()>,
    _server_task: tokio::task::JoinHandle<()>,
}

impl ServerHarness {
    async fn spawn(
        agent_name: &str,
        text_mode: TextResponseMode,
        skill_mode: SkillResponseMode,
    ) -> Self {
        let router =
            A2aServer::with_a2a_methods(build_skill_agent(agent_name, text_mode, skill_mode))
                .expect("create A2A server")
                .build_router();

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind benchmark listener");
        let addr = listener.local_addr().expect("server addr");
        let endpoint = format!("http://{addr}/jsonrpc");
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
        let server_task = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(async move {
                    let _ = shutdown_rx.await;
                })
                .await
                .expect("serve benchmark router");
        });

        Self {
            endpoint,
            _shutdown_tx: shutdown_tx,
            _server_task: server_task,
        }
    }
}

fn make_streaming_events() -> Vec<StreamResponse> {
    vec![
        sse_status(TaskState::Working, Some("planning")),
        sse_status(TaskState::Working, Some("executing")),
        sse_status(TaskState::Completed, Some("done")),
    ]
}

fn extract_task_id(value: &Value) -> Option<&str> {
    value.get("id").and_then(Value::as_str).or_else(|| {
        value
            .get("task")
            .and_then(|task| task.get("id"))
            .and_then(Value::as_str)
    })
}

fn bench_in_process_router(c: &mut Criterion) {
    let rt = runtime();
    let status_router = A2aServer::with_a2a_methods(build_skill_agent(
        "a2a-inprocess-agent",
        TextResponseMode::Task,
        SkillResponseMode::StatusOnlyTask,
    ))
    .expect("create in-process A2A server")
    .build_router();
    let artifact_router = A2aServer::with_a2a_methods(build_skill_agent(
        "a2a-inprocess-agent-artifact",
        TextResponseMode::Task,
        SkillResponseMode::ArtifactTask,
    ))
    .expect("create in-process artifact A2A server")
    .build_router();

    let create_task_body = message_send_skill_body("lookup", json!({"query": "warmup"}));
    let task_capture = rt
        .block_on(call_jsonrpc(status_router.clone(), create_task_body))
        .expect("create benchmark task");
    let task_id = task_capture
        .body_json
        .as_ref()
        .and_then(|body| body.get("result"))
        .and_then(extract_task_id)
        .expect("task id from send message")
        .to_string();

    let send_body = message_send_skill_body("lookup", json!({"query": "roundtrip"}));
    let get_body = tasks_get_body(&task_id, true, true);
    let stream_body = send_subscribe_skill_body("lookup", json!({"query": "stream"}));

    let mut group = c.benchmark_group("a2a_router_in_process");
    group.bench_function("message_send_skill_status_only", |b| {
        b.to_async(&rt).iter(|| async {
            call_jsonrpc(status_router.clone(), send_body.clone())
                .await
                .expect("in-process SendMessage");
        });
    });
    group.bench_function("message_send_skill_artifact", |b| {
        b.to_async(&rt).iter(|| async {
            call_jsonrpc(artifact_router.clone(), send_body.clone())
                .await
                .expect("in-process SendMessage artifact");
        });
    });
    group.bench_function("tasks_get_existing", |b| {
        b.to_async(&rt).iter(|| async {
            call_jsonrpc(status_router.clone(), get_body.clone())
                .await
                .expect("in-process GetTask");
        });
    });
    group.bench_function("send_subscribe_skill", |b| {
        b.to_async(&rt).iter(|| async {
            collect_send_subscribe_sse(status_router.clone(), stream_body.clone())
                .await
                .expect("in-process SendStreamingMessage");
        });
    });
    group.finish();
}

fn bench_handler_only(c: &mut Criterion) {
    let rt = runtime();
    let status_task_agent = Arc::new(build_skill_agent(
        "a2a-handler-task-agent",
        TextResponseMode::Task,
        SkillResponseMode::StatusOnlyTask,
    ));
    let artifact_task_agent = Arc::new(build_skill_agent(
        "a2a-handler-artifact-task-agent",
        TextResponseMode::Task,
        SkillResponseMode::ArtifactTask,
    ));
    let message_agent = Arc::new(build_skill_agent(
        "a2a-handler-message-agent",
        TextResponseMode::Message,
        SkillResponseMode::StatusOnlyTask,
    ));

    let text_params = MessageSendParams {
        message: Message::new(
            MessageRole::User,
            vec![Part::text("hello handler")],
            "handler-text-ctx".to_string(),
        ),
        tenant: None,
        configuration: None,
        metadata: None,
    };
    let skill_params = MessageSendParams {
        message: Message::new(
            MessageRole::User,
            vec![Part::data(json!({"skill": "lookup", "query": "handler"}))],
            "handler-skill-ctx".to_string(),
        ),
        tenant: None,
        configuration: None,
        metadata: None,
    };

    let mut group = c.benchmark_group("a2a_handler_only");
    group.bench_function("text_message_response", |b| {
        let agent = message_agent.clone();
        b.to_async(&rt).iter(|| {
            let agent = agent.clone();
            let params = text_params.clone();
            async move {
                agent_sdk::a2a::handle_message_send(agent.as_ref(), params)
                    .await
                    .expect("handler text message");
            }
        });
    });
    group.bench_function("text_task_response", |b| {
        let agent = status_task_agent.clone();
        b.to_async(&rt).iter(|| {
            let agent = agent.clone();
            let params = text_params.clone();
            async move {
                agent_sdk::a2a::handle_message_send(agent.as_ref(), params)
                    .await
                    .expect("handler text task");
            }
        });
    });
    group.bench_function("skill_task_response_status_only", |b| {
        let agent = status_task_agent.clone();
        b.to_async(&rt).iter(|| {
            let agent = agent.clone();
            let params = skill_params.clone();
            async move {
                agent_sdk::a2a::handle_message_send(agent.as_ref(), params)
                    .await
                    .expect("handler skill task");
            }
        });
    });
    group.bench_function("skill_task_response_artifact", |b| {
        let agent = artifact_task_agent.clone();
        b.to_async(&rt).iter(|| {
            let agent = agent.clone();
            let params = skill_params.clone();
            async move {
                agent_sdk::a2a::handle_message_send(agent.as_ref(), params)
                    .await
                    .expect("handler skill task");
            }
        });
    });
    group.finish();
}

fn bench_sdk_client_roundtrip(c: &mut Criterion) {
    let rt = runtime();
    let message_server = rt.block_on(ServerHarness::spawn(
        "a2a-sdk-client-message-agent",
        TextResponseMode::Message,
        SkillResponseMode::StatusOnlyTask,
    ));
    let task_server = rt.block_on(ServerHarness::spawn(
        "a2a-sdk-client-task-agent",
        TextResponseMode::Task,
        SkillResponseMode::StatusOnlyTask,
    ));
    let skill_status_server = rt.block_on(ServerHarness::spawn(
        "a2a-sdk-client-skill-status-agent",
        TextResponseMode::Task,
        SkillResponseMode::StatusOnlyTask,
    ));
    let skill_artifact_server = rt.block_on(ServerHarness::spawn(
        "a2a-sdk-client-skill-artifact-agent",
        TextResponseMode::Task,
        SkillResponseMode::ArtifactTask,
    ));
    let message_client =
        A2aClient::direct(&message_server.endpoint).expect("create SDK A2A message client");
    let task_client = A2aClient::direct(&task_server.endpoint).expect("create SDK A2A task client");
    let skill_status_client =
        A2aClient::direct(&skill_status_server.endpoint).expect("create SDK status skill client");
    let skill_artifact_client = A2aClient::direct(&skill_artifact_server.endpoint)
        .expect("create SDK artifact skill client");
    let skill_message_status = Message {
        role: MessageRole::User,
        parts: vec![Part::data(json!({"skill": "lookup", "query": "sdk"}))],
        message_id: "sdk-msg-status".to_string(),
        task_id: None,
        context_id: Some("sdk-skill-status-ctx".to_string()),
        metadata: None,
        extensions: None,
        reference_task_ids: None,
    };
    let skill_message_artifact = Message {
        role: MessageRole::User,
        parts: vec![Part::data(json!({"skill": "lookup", "query": "sdk"}))],
        message_id: "sdk-msg-artifact".to_string(),
        task_id: None,
        context_id: Some("sdk-skill-artifact-ctx".to_string()),
        metadata: None,
        extensions: None,
        reference_task_ids: None,
    };
    let explicit_skill_task_id = "sdk-explicit-skill-task".to_string();
    let explicit_skill_context_id = "sdk-explicit-skill-ctx".to_string();

    let task_seed = rt
        .block_on(task_client.send_message("seed task", Some("seed-ctx".to_string())))
        .expect("seed task");
    let seeded_task_id = extract_task_id(&task_seed)
        .expect("seed task id")
        .to_string();
    let message_counter = Arc::new(AtomicU64::new(0));
    let task_endpoint = task_server.endpoint.clone();

    let mut group = c.benchmark_group("a2a_sdk_client_roundtrip");
    group.bench_function("send_message_text_message_response", |b| {
        b.to_async(&rt).iter(|| async {
            message_client
                .send_message("hello roundtrip", Some("bench-ctx".to_string()))
                .await
                .expect("sdk send_message message response");
        });
    });
    group.bench_function("send_message_text_task_response", |b| {
        b.to_async(&rt).iter(|| async {
            task_client
                .send_message("hello roundtrip", Some("bench-ctx".to_string()))
                .await
                .expect("sdk send_message task response");
        });
    });
    group.bench_function("message_send_skill_status_only", |b| {
        b.to_async(&rt).iter(|| async {
            skill_status_client
                .message_send(skill_message_status.clone(), None)
                .await
                .expect("sdk message_send status-only");
        });
    });
    group.bench_function("message_send_skill_artifact", |b| {
        b.to_async(&rt).iter(|| async {
            skill_artifact_client
                .message_send(skill_message_artifact.clone(), None)
                .await
                .expect("sdk message_send artifact");
        });
    });
    group.bench_function("message_send_skill_artifact_existing_task_id", |b| {
        let skill_artifact_client = &skill_artifact_client;
        let explicit_skill_task_id = explicit_skill_task_id.clone();
        let explicit_skill_context_id = explicit_skill_context_id.clone();
        let counter = message_counter.clone();
        b.to_async(&rt).iter(|| {
            let seq = counter.fetch_add(1, Ordering::Relaxed);
            let skill_artifact_client = skill_artifact_client;
            let explicit_skill_task_id = explicit_skill_task_id.clone();
            let explicit_skill_context_id = explicit_skill_context_id.clone();
            async move {
                let message = Message {
                    role: MessageRole::User,
                    parts: vec![Part::data(
                        json!({"skill": "lookup", "query": format!("explicit-{seq}")}),
                    )],
                    message_id: format!("sdk-msg-artifact-existing-{seq}"),
                    task_id: Some(explicit_skill_task_id),
                    context_id: Some(explicit_skill_context_id),
                    metadata: None,
                    extensions: None,
                    reference_task_ids: None,
                };
                skill_artifact_client
                    .message_send(message, None)
                    .await
                    .expect("sdk message_send artifact existing task");
            }
        });
    });
    group.bench_function("get_task_existing", |b| {
        b.to_async(&rt).iter(|| async {
            task_client
                .get_task(&seeded_task_id)
                .await
                .expect("sdk get_task");
        });
    });
    group.finish();

    let mut shape_group = c.benchmark_group("a2a_sdk_client_message_shapes");
    let prebuilt_task_client =
        A2aClient::direct(&task_endpoint).expect("create prebuilt task-shape client");
    let prebuilt_text_task = Message {
        role: MessageRole::User,
        parts: vec![Part::text("prebuilt task text")],
        message_id: "prebuilt-task-msg".to_string(),
        task_id: Some("prebuilt-task-shape".to_string()),
        context_id: Some("prebuilt-task-ctx".to_string()),
        metadata: None,
        extensions: None,
        reference_task_ids: None,
    };
    shape_group.bench_function("message_send_prebuilt_text_task_shape", |b| {
        b.to_async(&rt).iter(|| async {
            prebuilt_task_client
                .message_send(prebuilt_text_task.clone(), None)
                .await
                .expect("sdk prebuilt text task shape");
        });
    });
    shape_group.bench_function("message_send_construct_text_new_task_shape", |b| {
        let task_client =
            A2aClient::direct(&task_endpoint).expect("create constructed new-task client");
        let counter = message_counter.clone();
        b.to_async(&rt).iter(|| {
            let seq = counter.fetch_add(1, Ordering::Relaxed);
            let task_client = &task_client;
            async move {
                let message = Message {
                    role: MessageRole::User,
                    parts: vec![Part::text("constructed task text")],
                    message_id: format!("constructed-task-msg-{seq}"),
                    task_id: Some(format!("constructed-task-{seq}")),
                    context_id: Some(format!("constructed-task-ctx-{seq}")),
                    metadata: None,
                    extensions: None,
                    reference_task_ids: None,
                };
                task_client
                    .message_send(message, None)
                    .await
                    .expect("sdk constructed text task shape");
            }
        });
    });
    shape_group.bench_function("message_send_construct_text_existing_task", |b| {
        let task_client =
            A2aClient::direct(&task_endpoint).expect("create constructed existing-task client");
        let counter = message_counter.clone();
        let seeded_task_id = seeded_task_id.clone();
        b.to_async(&rt).iter(|| {
            let seq = counter.fetch_add(1, Ordering::Relaxed);
            let seeded_task_id = seeded_task_id.clone();
            let task_client = &task_client;
            async move {
                let message = Message {
                    role: MessageRole::User,
                    parts: vec![Part::text("existing task text")],
                    message_id: format!("existing-task-msg-{seq}"),
                    task_id: Some(seeded_task_id),
                    context_id: Some("seed-ctx".to_string()),
                    metadata: None,
                    extensions: None,
                    reference_task_ids: None,
                };
                task_client
                    .message_send(message, None)
                    .await
                    .expect("sdk existing task shape");
            }
        });
    });
    shape_group.bench_function("message_send_construct_text_context_only", |b| {
        let task_client =
            A2aClient::direct(&task_endpoint).expect("create constructed context-only client");
        let counter = message_counter.clone();
        b.to_async(&rt).iter(|| {
            let seq = counter.fetch_add(1, Ordering::Relaxed);
            let task_client = &task_client;
            async move {
                let message = Message {
                    role: MessageRole::User,
                    parts: vec![Part::text("context only text")],
                    message_id: format!("context-only-msg-{seq}"),
                    task_id: None,
                    context_id: Some(format!("context-only-{seq}")),
                    metadata: None,
                    extensions: None,
                    reference_task_ids: None,
                };
                task_client
                    .message_send(message, None)
                    .await
                    .expect("sdk context-only shape");
            }
        });
    });
    shape_group.finish();
}

fn bench_mock_client_paths(c: &mut Criterion) {
    let rt = runtime();
    let mock_server = rt.block_on(
        MockA2AServerBuilder::new()
            .sse_events(make_streaming_events())
            .spawn(),
    );

    let sdk_client = A2aClient::direct(mock_server.url()).expect("create mock SDK client");
    let http_client = HttpClient::external(mock_server.url());
    let skill_message = Message {
        role: MessageRole::User,
        parts: vec![Part::data(json!({"skill": "lookup", "query": "mock"}))],
        message_id: "mock-msg-1".to_string(),
        task_id: None,
        context_id: Some("mock-ctx".to_string()),
        metadata: None,
        extensions: None,
        reference_task_ids: None,
    };

    let streaming_message = Message::new(
        MessageRole::User,
        vec![Part::text("stream me")],
        "mock-stream-ctx".to_string(),
    );

    let mut group = c.benchmark_group("a2a_mock_client_paths");
    group.bench_function("sdk_send_message", |b| {
        b.to_async(&rt).iter(|| async {
            sdk_client
                .send_message("mock hello", Some("mock-text-ctx".to_string()))
                .await
                .expect("mock send_message");
        });
    });
    group.bench_function("sdk_message_send_skill", |b| {
        b.to_async(&rt).iter(|| async {
            sdk_client
                .message_send(skill_message.clone(), None)
                .await
                .expect("mock message_send");
        });
    });
    group.bench_function("transport_send_subscribe", |b| {
        b.to_async(&rt).iter(|| async {
            let mut stream = http_client
                .send_subscribe("mock-task-1", streaming_message.clone())
                .await
                .expect("mock send_subscribe");
            while let Some(event) = stream.next().await {
                let event = event.expect("stream event");
                if event.is_terminal() {
                    break;
                }
            }
        });
    });
    group.finish();
}

fn configure_criterion() -> Criterion {
    Criterion::default()
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(3))
}

criterion_group!(
    name = benches;
    config = configure_criterion();
    targets =
        bench_in_process_router,
        bench_handler_only,
        bench_sdk_client_roundtrip,
        bench_mock_client_paths
);
criterion_main!(benches);
