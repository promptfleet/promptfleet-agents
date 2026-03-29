//! Integration tests for SubAgentTool delegation using MockA2AServer.
//!
//! Run: cargo test -p agent_sdk --features sub-agents --test sub_agent_delegation

#![cfg(not(target_arch = "wasm32"))]
#![cfg(feature = "sub-agents")]

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use a2a_protocol_core::data::TaskState;
use agent_core::{AgentMessage, ContentPart, Role};
use agent_sdk::a2a::sub_agent::{
    A2aStreamEvent, A2aSubAgentAdapter, A2aTraceMapper, DefaultA2aTraceMapper,
};
use agent_sdk::agent::llm_orchestrator::{
    LlmPolicy, run_tools_loop_agnostic, run_tools_loop_agnostic_with_cancel,
};
use agent_sdk::agent::tool_context::ToolContext;
use agent_sdk::agent::tools::{ToolExecutor, ToolRegistry, ToolSpec};
use agent_sdk::agent::trace::AgentTraceEvent;
use agent_sdk::sub_agent::{DelegationMode, SubAgentContext, SubAgentToolBuilder};
use futures_util::StreamExt;
use pf_test_harness::a2a_mock::{MockA2AServerBuilder, sse_status};
use pf_test_harness::scenario::LlmScenario;
use serde_json::json;
use tokio::sync::Notify;
use tokio::time::{Duration, timeout};

/// Helper: execute a ToolSpec's WithContext executor with a capturing ToolContext.
async fn run_tool(
    spec: &ToolSpec,
    args: serde_json::Value,
) -> (Result<serde_json::Value, String>, Vec<AgentTraceEvent>) {
    let events = Arc::new(std::sync::Mutex::new(Vec::new()));
    let events_clone = events.clone();
    let sink: Arc<dyn Fn(AgentTraceEvent) + Send + Sync> = Arc::new(move |event| {
        events_clone.lock().unwrap().push(event);
    });
    let ctx = ToolContext::new(sink, Arc::new(AtomicBool::new(false)));

    let result = match &spec.executor {
        ToolExecutor::WithContext(f) => f(args, ctx).await,
        ToolExecutor::Simple(f) => f(args).await,
    };

    let captured = events.lock().unwrap().clone();
    (result, captured)
}

// =====================================================================
// Streaming delegation
// =====================================================================

#[tokio::test]
async fn streaming_delegation_forwards_events() {
    let server = MockA2AServerBuilder::new()
        .sse_events(vec![
            sse_status(TaskState::Working, Some("planning...")),
            sse_status(TaskState::Working, Some("almost done")),
            sse_status(TaskState::Completed, Some("final answer")),
        ])
        .spawn()
        .await;

    let spec = SubAgentToolBuilder::new("planner", server.url(), "delegate_planner")
        .delegation_mode(DelegationMode::Streaming)
        .emit_handoff(false)
        .build();

    let (result, events) = run_tool(&spec, json!({"task": "plan"})).await;

    let result = result.expect("tool should succeed");
    assert_eq!(result["result"], "final answer");

    let progress: Vec<&AgentTraceEvent> = events
        .iter()
        .filter(|e| matches!(e, AgentTraceEvent::ProgressUpdate { .. }))
        .collect();
    assert_eq!(
        progress.len(),
        2,
        "Working events with text should produce ProgressUpdate (Completed is skipped by mapper)"
    );
}

#[tokio::test]
async fn streaming_delegation_with_handoff_events() {
    let server = MockA2AServerBuilder::new()
        .sse_events(vec![
            sse_status(TaskState::Working, Some("working")),
            sse_status(TaskState::Completed, Some("done")),
        ])
        .spawn()
        .await;

    let spec = SubAgentToolBuilder::new("executor", server.url(), "delegate_exec")
        .delegation_mode(DelegationMode::Streaming)
        .emit_handoff(true)
        .build();

    let (result, events) = run_tool(&spec, json!({})).await;
    assert!(result.is_ok());

    let handoffs: Vec<&AgentTraceEvent> = events
        .iter()
        .filter(|e| matches!(e, AgentTraceEvent::AgentHandoff { .. }))
        .collect();
    assert_eq!(handoffs.len(), 2, "should emit handoff at start and end");

    match &handoffs[0] {
        AgentTraceEvent::AgentHandoff {
            from_agent,
            to_agent,
            ..
        } => {
            assert_eq!(from_agent, "self");
            assert_eq!(to_agent, "executor");
        }
        _ => unreachable!(),
    }
    match &handoffs[1] {
        AgentTraceEvent::AgentHandoff {
            from_agent,
            to_agent,
            ..
        } => {
            assert_eq!(from_agent, "executor");
            assert_eq!(to_agent, "self");
        }
        _ => unreachable!(),
    }
}

#[tokio::test]
async fn streaming_delegation_cancellation() {
    let server = MockA2AServerBuilder::new()
        .sse_events(vec![
            sse_status(TaskState::Working, Some("step 1")),
            sse_status(TaskState::Working, Some("step 2")),
            sse_status(TaskState::Working, Some("step 3")),
            sse_status(TaskState::Working, Some("step 4")),
            sse_status(TaskState::Completed, Some("done")),
        ])
        .spawn()
        .await;

    let spec = SubAgentToolBuilder::new("slow", server.url(), "delegate_slow")
        .delegation_mode(DelegationMode::Streaming)
        .emit_handoff(false)
        .build();

    let event_count = Arc::new(AtomicUsize::new(0));
    let event_count_clone = event_count.clone();
    let cancel_flag = Arc::new(AtomicBool::new(false));
    let cancel_clone = cancel_flag.clone();

    let sink: Arc<dyn Fn(AgentTraceEvent) + Send + Sync> = Arc::new(move |_event| {
        let count = event_count_clone.fetch_add(1, Ordering::Relaxed);
        if count >= 1 {
            cancel_clone.store(true, Ordering::Relaxed);
        }
    });
    let ctx = ToolContext::new(sink, cancel_flag);

    let result = match &spec.executor {
        ToolExecutor::WithContext(f) => f(json!({}), ctx).await,
        _ => panic!("expected WithContext"),
    };

    assert!(result.is_ok());
    let count = event_count.load(Ordering::Relaxed);
    assert!(
        count < 5,
        "cancellation should stop iteration early, got {count} events"
    );
    let cancel_requests = server.cancel_requests();
    assert_eq!(
        cancel_requests.len(),
        1,
        "expected one remote tasks/cancel request"
    );
    assert_eq!(
        cancel_requests[0]
            .get("params")
            .and_then(|params| params.get("id"))
            .and_then(|value| value.as_str()),
        Some("mock-task-1")
    );
}

#[tokio::test]
async fn streaming_delegation_failure() {
    let server = MockA2AServerBuilder::new()
        .sse_events(vec![
            sse_status(TaskState::Working, Some("starting...")),
            sse_status(TaskState::Failed, Some("internal error")),
        ])
        .spawn()
        .await;

    let spec = SubAgentToolBuilder::new("broken", server.url(), "delegate_broken")
        .delegation_mode(DelegationMode::Streaming)
        .emit_handoff(false)
        .build();

    let (result, events) = run_tool(&spec, json!({})).await;
    let err = result.expect_err("tool should return structured Err on sub-agent failure");
    let error_json: serde_json::Value =
        serde_json::from_str(&err).expect("structured sub-agent error JSON");
    assert_eq!(error_json["source"], "sub_agent");
    assert_eq!(error_json["error_kind"], "subagent_failed");
    assert!(
        error_json["message"]
            .as_str()
            .unwrap_or("")
            .contains("internal error"),
        "error should contain the failed message text"
    );

    let progress_count = events
        .iter()
        .filter(|e| matches!(e, AgentTraceEvent::ProgressUpdate { .. }))
        .count();
    assert!(
        progress_count >= 1,
        "should have forwarded at least the Working + Failed events with text"
    );
}

#[tokio::test]
async fn streaming_delegation_allows_tool_loop_to_continue_after_success() {
    let server = MockA2AServerBuilder::new()
        .sse_events(vec![
            sse_status(TaskState::Working, Some("planning...")),
            sse_status(TaskState::Completed, Some("final answer")),
        ])
        .spawn()
        .await;

    let mut tools = ToolRegistry::new();
    tools.register(
        SubAgentToolBuilder::new("planner", server.url(), "delegate_planner")
            .delegation_mode(DelegationMode::Streaming)
            .emit_handoff(true)
            .build(),
    );

    let stream = run_tools_loop_agnostic(
        LlmScenario::new()
            .turn(|t| {
                t.tool_call("delegate_planner", json!({"task":"plan"}))
                    .done("tool_calls")
            })
            .turn(|t| t.content("done").done("stop"))
            .into_stream_invoker(),
        "test-model".to_string(),
        tools,
        LlmPolicy::default(),
        AgentMessage::new(Role::User, vec![ContentPart::Text("plan".to_string())]),
        None,
        None,
        None,
    );

    let events = stream.collect::<Vec<_>>().await;
    assert!(
        events.iter().any(|event| matches!(
            event,
            AgentTraceEvent::ToolCallCompleted { success: true, .. }
        )),
        "expected successful tool completion, got events: {events:#?}"
    );
    assert!(
        events.iter().any(|event| matches!(event, AgentTraceEvent::Completed { text, .. } if text.as_deref() == Some("done"))),
        "expected loop to continue into the follow-up text turn, got events: {events:#?}"
    );
}

#[tokio::test]
async fn dropping_stream_cancels_inflight_tool_execution() {
    let started = Arc::new(Notify::new());
    let observed_cancel = Arc::new(AtomicBool::new(false));

    let mut tools = ToolRegistry::new();
    tools.register(ToolSpec {
        name: "wait_for_cancel".to_string(),
        description: Some("Blocks until cancelled".to_string()),
        parameters: json!({"type":"object"}),
        strict: true,
        parallel_ok: false,
        executor: ToolExecutor::WithContext(Arc::new({
            let started = started.clone();
            let observed_cancel = observed_cancel.clone();
            move |_args, ctx| {
                let started = started.clone();
                let observed_cancel = observed_cancel.clone();
                Box::pin(async move {
                    started.notify_waiters();
                    loop {
                        if ctx.is_cancelled() {
                            observed_cancel.store(true, Ordering::Relaxed);
                            return Ok(json!({"cancelled": true}));
                        }
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                })
            }
        })),
    });

    let cancel_flag = Arc::new(AtomicBool::new(false));
    let started_wait = started.notified();
    let mut stream = run_tools_loop_agnostic_with_cancel(
        LlmScenario::new()
            .turn(|t| t.tool_call("wait_for_cancel", json!({})).done("tool_calls"))
            .into_stream_invoker(),
        "test-model".to_string(),
        tools,
        LlmPolicy::default(),
        AgentMessage::new(Role::User, vec![ContentPart::Text("cancel".to_string())]),
        None,
        None,
        None,
        Some(cancel_flag.clone()),
        None,
    );

    let _first = stream.next().await.expect("first event");
    timeout(Duration::from_secs(1), started_wait)
        .await
        .expect("tool should start before disconnect");

    drop(stream);

    timeout(Duration::from_secs(1), async {
        while !observed_cancel.load(Ordering::Relaxed) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("dropping the stream should cancel in-flight tool execution");
    assert!(cancel_flag.load(Ordering::Relaxed));
}

// =====================================================================
// Synchronous delegation
// =====================================================================

#[tokio::test]
async fn synchronous_delegation_returns_result() {
    let server = MockA2AServerBuilder::new().spawn().await;

    let spec = SubAgentToolBuilder::new("sync-agent", server.url(), "delegate_sync")
        .delegation_mode(DelegationMode::Synchronous)
        .emit_handoff(false)
        .build();

    let (result, events) = run_tool(&spec, json!({"query": "hello"})).await;
    let result = result.expect("sync delegation should succeed");
    assert!(result.get("result").is_some());
    assert!(
        events.is_empty(),
        "sync mode should not emit any mid-execution events"
    );
}

#[tokio::test]
async fn synchronous_delegation_with_handoff() {
    let server = MockA2AServerBuilder::new().spawn().await;

    let spec = SubAgentToolBuilder::new("sync-agent", server.url(), "delegate_sync")
        .delegation_mode(DelegationMode::Synchronous)
        .emit_handoff(true)
        .build();

    let (result, events) = run_tool(&spec, json!({})).await;
    assert!(result.is_ok());

    let handoffs: Vec<&AgentTraceEvent> = events
        .iter()
        .filter(|e| matches!(e, AgentTraceEvent::AgentHandoff { .. }))
        .collect();
    assert_eq!(
        handoffs.len(),
        2,
        "sync mode with handoff should emit start + end handoff events"
    );
}

// =====================================================================
// Builder validation
// =====================================================================

#[tokio::test]
async fn builder_defaults_produce_valid_spec() {
    let spec = SubAgentToolBuilder::new("agent", "http://localhost:9999", "delegate").build();

    assert_eq!(spec.name, "delegate");
    assert!(matches!(spec.executor, ToolExecutor::WithContext(_)));
    assert!(spec.description.is_some());
    assert!(spec.strict);
}

#[tokio::test]
async fn builder_custom_mapper() {
    #[derive(Debug)]
    struct NoopMapper;
    impl A2aTraceMapper for NoopMapper {
        fn map(&self, _: &A2aStreamEvent, _: &SubAgentContext) -> Vec<AgentTraceEvent> {
            Vec::new()
        }
        fn extract_result(&self, event: &A2aStreamEvent) -> Option<String> {
            DefaultA2aTraceMapper.extract_result(event)
        }
    }

    let server = MockA2AServerBuilder::new()
        .sse_events(vec![
            sse_status(TaskState::Working, Some("hidden")),
            sse_status(TaskState::Completed, Some("result")),
        ])
        .spawn()
        .await;

    let spec = SubAgentToolBuilder::new("agent", server.url(), "delegate")
        .adapter(Arc::new(
            A2aSubAgentAdapter::default().trace_mapper(Arc::new(NoopMapper)),
        ))
        .emit_handoff(false)
        .build();

    let (result, events) = run_tool(&spec, json!({})).await;
    assert!(result.is_ok());
    assert!(
        events.is_empty(),
        "NoopMapper should suppress all trace events"
    );
}

#[tokio::test]
async fn streaming_result_transformer_rewrites_final_result() {
    let server = MockA2AServerBuilder::new()
        .sse_events(vec![sse_status(
            TaskState::Completed,
            Some("planner output"),
        )])
        .spawn()
        .await;

    let spec = SubAgentToolBuilder::new("planner", server.url(), "delegate")
        .result_transformer(Arc::new(|mut value| {
            Box::pin(async move {
                value["result"] = json!("stored by transformer");
                value["graph_id"] = json!("graph-123");
                Ok(value)
            })
        }))
        .emit_handoff(false)
        .build();

    let (result, _events) = run_tool(&spec, json!({})).await;
    let result = result.expect("tool should succeed");
    assert_eq!(result["result"], "stored by transformer");
    assert_eq!(result["graph_id"], "graph-123");
}

#[tokio::test]
async fn streaming_result_transformer_failure_returns_structured_error() {
    let server = MockA2AServerBuilder::new()
        .sse_events(vec![sse_status(
            TaskState::Completed,
            Some("planner output"),
        )])
        .spawn()
        .await;

    let spec = SubAgentToolBuilder::new("planner", server.url(), "delegate")
        .result_transformer(Arc::new(|value| {
            let _ = value;
            Box::pin(async { Err("store failed".to_string()) })
        }))
        .emit_handoff(false)
        .build();

    let (result, _events) = run_tool(&spec, json!({})).await;
    let err = result.expect_err("transform should fail");
    let error_json: serde_json::Value =
        serde_json::from_str(&err).expect("structured transform error JSON");
    assert_eq!(error_json["error_kind"], "result_transform_failed");
    assert_eq!(error_json["message"], "store failed");
}
