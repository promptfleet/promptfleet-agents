#![cfg(all(
    not(target_arch = "wasm32"),
    feature = "event-stream",
    feature = "test-support"
))]

use agent_sdk::agent::tools::{ToolExecutor, ToolKind, ToolRegistry, ToolSpec};
use agent_sdk::agent::trace::AgentTraceEvent;
use agent_sdk::streaming::{
    AgUiDriverConfig, AgUiStreamDriver, IoEventContext, ag_ui_sse_response_with_summary,
};
use futures::StreamExt;
use pf_test_harness::pipeline::{InvokerMode, MapperKind, TestPipeline};
use pf_test_harness::scenario::LlmScenario;
use std::sync::Arc;

fn test_tools() -> ToolRegistry {
    let mut tools = ToolRegistry::new();
    tools.register(ToolSpec {
        name: "search".to_string(),
        description: Some("Search test tool".to_string()),
        parameters: serde_json::json!({"type":"object"}),
        kind: ToolKind::Function,
        strict: false,
        parallel_ok: false,
        executor: ToolExecutor::Simple(Arc::new(|args| {
            Box::pin(async move { Ok(serde_json::json!({"ok": args})) })
        })),
    });
    tools
}

#[tokio::test]
async fn test_tool_call_produces_expected_ag_ui_sequence() {
    let result = TestPipeline::new()
        .with_scenario(LlmScenario::tool_call_then_text(
            "search",
            serde_json::json!({"q": "test"}),
            "done",
        ))
        .with_tools(test_tools())
        .with_mapper(MapperKind::AgentIo)
        .with_invoker_mode(InvokerMode::Streaming)
        .build()
        .expect("pipeline build")
        .run()
        .await
        .expect("pipeline run");

    let types: Vec<&str> = result
        .mapped_events()
        .iter()
        .map(|e| e.wire_type())
        .collect();
    assert_eq!(
        types,
        vec![
            "STEP_STARTED",
            "TOOL_CALL_START",
            "TOOL_CALL_ARGS",
            "TOOL_CALL_END",
            "TOOL_CALL_RESULT",
            "STEP_FINISHED",
            "STEP_STARTED",
            "TEXT_MESSAGE_CONTENT",
            "STEP_FINISHED",
            "RUN_FINISHED",
        ],
    );
}

#[tokio::test]
async fn test_streaming_and_request_response_parity_for_tool_results() {
    let scenario =
        LlmScenario::tool_call_then_text("search", serde_json::json!({"q": "parity"}), "ok");
    let tools = test_tools();

    let streaming = TestPipeline::new()
        .with_scenario(scenario.clone())
        .with_tools(tools.clone())
        .with_invoker_mode(InvokerMode::Streaming)
        .build()
        .expect("streaming build")
        .run()
        .await
        .expect("streaming run");

    let request_response = TestPipeline::new()
        .with_scenario(scenario)
        .with_tools(tools)
        .with_invoker_mode(InvokerMode::RequestResponse)
        .build()
        .expect("request-response build")
        .run()
        .await
        .expect("request-response run");

    let streaming_text = streaming
        .engine_result()
        .as_ref()
        .expect("streaming result")
        .text
        .clone();
    let request_text = request_response
        .engine_result()
        .as_ref()
        .expect("request-response result")
        .text
        .clone();
    assert_eq!(streaming_text, request_text);

    let streaming_tool_results: Vec<(String, serde_json::Value)> = streaming
        .trace_events()
        .iter()
        .filter_map(|event| match event {
            AgentTraceEvent::ToolCallCompleted { id, result, .. } => {
                Some((id.clone(), result.clone()))
            }
            _ => None,
        })
        .collect();
    let request_tool_results: Vec<(String, serde_json::Value)> = request_response
        .trace_events()
        .iter()
        .filter_map(|event| match event {
            AgentTraceEvent::ToolCallCompleted { id, result, .. } => {
                Some((id.clone(), result.clone()))
            }
            _ => None,
        })
        .collect();

    assert_eq!(streaming_tool_results, request_tool_results);
}

#[tokio::test]
async fn test_driver_with_test_pipeline() {
    let result = TestPipeline::new()
        .with_scenario(LlmScenario::tool_call_then_text(
            "search",
            serde_json::json!({"q": "driver"}),
            "done",
        ))
        .with_tools(test_tools())
        .with_mapper(MapperKind::AgentIo)
        .with_invoker_mode(InvokerMode::Streaming)
        .build()
        .expect("pipeline build")
        .run()
        .await
        .expect("pipeline run");

    let io_ctx = IoEventContext {
        thread_id: "thread-test".to_string(),
        run_id: "run-test".to_string(),
        message_id: "msg-test".to_string(),
    };

    let trace_events = result.trace_events().to_vec();
    let trace_stream = futures::stream::iter(trace_events);
    let stream = AgUiStreamDriver::new(AgUiDriverConfig {
        ctx: io_ctx,
        cancel_flag: None,
        enrichers: Vec::new(),
    })
    .drive(trace_stream);

    let types: Vec<&str> = stream
        .collect::<Vec<_>>()
        .await
        .iter()
        .map(|e| e.wire_type())
        .collect();

    assert_eq!(
        types,
        vec![
            "RUN_STARTED",
            "STEP_STARTED",
            "TOOL_CALL_START",
            "TOOL_CALL_ARGS",
            "TOOL_CALL_END",
            "TOOL_CALL_RESULT",
            "STEP_FINISHED",
            "STEP_STARTED",
            "TEXT_MESSAGE_START",
            "TEXT_MESSAGE_CONTENT",
            "STEP_FINISHED",
            "TEXT_MESSAGE_END",
            "RUN_FINISHED",
        ]
    );
}

#[tokio::test]
async fn test_with_summary_handle_via_pipeline() {
    use agent_sdk::streaming::driver::RunStatus;

    let result = TestPipeline::new()
        .with_scenario(LlmScenario::tool_call_then_text(
            "search",
            serde_json::json!({"q": "sse"}),
            "result text",
        ))
        .with_tools(test_tools())
        .with_mapper(MapperKind::AgentIo)
        .with_invoker_mode(InvokerMode::Streaming)
        .build()
        .expect("pipeline build")
        .run()
        .await
        .expect("pipeline run");

    let trace_events = result.trace_events().to_vec();
    let trace_stream = futures::stream::iter(trace_events);

    let config = AgUiDriverConfig {
        ctx: IoEventContext {
            thread_id: "t-sse".into(),
            run_id: "r-sse".into(),
            message_id: "m-sse".into(),
        },
        cancel_flag: None,
        enrichers: Vec::new(),
    };

    let (event_stream, summary_handle) = AgUiStreamDriver::new(config)
        .drive(trace_stream)
        .with_summary_handle();

    let mut pinned = Box::pin(event_stream);
    let mut event_count = 0;
    while pinned.next().await.is_some() {
        event_count += 1;
    }
    assert!(event_count > 0, "should yield AG UI events");

    let summary = summary_handle.get().await.expect("summary should resolve");
    assert_eq!(summary.status, RunStatus::Completed);
    assert!(
        summary.full_text.contains("result text"),
        "expected accumulated text, got: {:?}",
        summary.full_text
    );
    assert_eq!(summary.tool_calls_count, 1);
}

/// Verify that `ag_ui_sse_response_with_summary` compiles and the SSE type
/// is a valid Axum `IntoResponse`. The summary is captured by consuming the
/// SSE stream's underlying event sequence via the body.
#[tokio::test]
async fn test_ag_ui_sse_response_with_summary_type_checks() {
    use axum::response::IntoResponse;

    let events = vec![
        AgentTraceEvent::ContentDelta {
            delta: "hi".to_string(),
        },
        AgentTraceEvent::Completed {
            text: Some("hi".to_string()),
            usage: None,
        },
    ];
    let trace_stream = futures::stream::iter(events);

    let config = AgUiDriverConfig {
        ctx: IoEventContext {
            thread_id: "t".into(),
            run_id: "r".into(),
            message_id: "m".into(),
        },
        cancel_flag: None,
        enrichers: Vec::new(),
    };

    let (sse, _handle) = ag_ui_sse_response_with_summary(config, trace_stream);
    let response = sse.into_response();
    assert_eq!(
        response.headers().get("content-type").unwrap(),
        "text/event-stream"
    );
}
