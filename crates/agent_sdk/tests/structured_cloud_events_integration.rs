#![cfg(all(not(target_arch = "wasm32"), feature = "structured-io"))]

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use agent_sdk::agent::llm_orchestrator::{
    IntoLlmInvoker, IntoLlmStreamInvoker, LlmInvoker, LlmStreamFuture, LlmStreamInvoker,
};
use agent_sdk::agent::tools::ToolRegistry;
use agent_sdk::{AgentBuilder, CloudEventEnvelope, StructuredInput};
use llm_client::{LlmEventStream, LlmRequest, LlmResponse};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Clone)]
struct MockStructuredRuntime {
    responses: Arc<Mutex<VecDeque<Result<LlmResponse, String>>>>,
}

impl MockStructuredRuntime {
    fn new(responses: Vec<Result<LlmResponse, String>>) -> Self {
        Self {
            responses: Arc::new(Mutex::new(responses.into())),
        }
    }
}

impl LlmInvoker for MockStructuredRuntime {
    fn request(
        &self,
        _req: LlmRequest,
    ) -> std::pin::Pin<Box<dyn core::future::Future<Output = Result<LlmResponse, String>> + Send>>
    {
        let next = self
            .responses
            .lock()
            .expect("mock runtime lock poisoned")
            .pop_front()
            .unwrap_or_else(|| Err("no mock response queued".to_string()));
        Box::pin(async move { next })
    }
}

impl LlmStreamInvoker for MockStructuredRuntime {
    fn request_stream(&self, _req: LlmRequest) -> LlmStreamFuture {
        Box::pin(async move {
            let stream: LlmEventStream = Box::pin(futures::stream::empty());
            Ok(stream)
        })
    }
}

impl IntoLlmInvoker for MockStructuredRuntime {
    fn into_invoker(self) -> Arc<dyn LlmInvoker> {
        Arc::new(self)
    }
}

impl IntoLlmStreamInvoker for MockStructuredRuntime {
    fn into_stream_invoker(self) -> Arc<dyn LlmStreamInvoker> {
        Arc::new(self)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
struct AlertSignal {
    alert_id: String,
    severity: String,
    summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
struct AnalysisOutput {
    alert_id: String,
    disposition: String,
    confidence: f32,
}

fn checkpoint_tool_call(args: serde_json::Value) -> LlmResponse {
    serde_json::from_value(json!({
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": null,
                "tool_calls": [{
                    "id": "call_ck",
                    "name": "checkpoint_task",
                    "arguments": args
                }]
            },
            "finish_reason": "tool_calls"
        }]
    }))
    .expect("valid llm fixture")
}

#[tokio::test]
async fn structured_cloud_event_flow_uses_public_api() {
    let runtime = MockStructuredRuntime::new(vec![Ok(checkpoint_tool_call(json!({
        "task_patch": { "state": "completed" },
        "structured_output": {
            "payload": {
                "alert_id": "alert-42",
                "disposition": "escalate",
                "confidence": 0.91
            },
            "text": "Escalate to the on-call analyst"
        },
        "respond": { "kind": "task" }
    })))]);

    let mut agent = AgentBuilder::new("integration-structured-agent")
        .expect("builder")
        .with_structured_output::<AnalysisOutput>("analysis_output", "analysis_output")
        .build()
        .expect("agent");
    agent
        .configure_llm_runtime(
            runtime,
            "mock-structured-model",
            ToolRegistry::new(),
            Some("Produce a typed analysis payload.".to_string()),
            None,
            None,
        )
        .expect("configure llm runtime");

    let incoming = CloudEventEnvelope::new_json(
        "com.example.alert_signal",
        "urn:promptfleet:alerts",
        AlertSignal {
            alert_id: "alert-42".to_string(),
            severity: "critical".to_string(),
            summary: "Unexpected credential use".to_string(),
        },
    )
    .with_subject("alert-42");

    let result = agent
        .run_structured::<_, AnalysisOutput>(
            StructuredInput::from_cloudevent(incoming).expect("structured input"),
        )
        .await
        .expect("structured run");

    assert_eq!(
        result.output,
        AnalysisOutput {
            alert_id: "alert-42".to_string(),
            disposition: "escalate".to_string(),
            confidence: 0.91,
        }
    );
    assert_eq!(result.artifact_name, "analysis_output");
    assert_eq!(
        result.final_text.as_deref(),
        Some("Escalate to the on-call analyst")
    );

    let outgoing = result.into_cloud_event(
        "com.example.analysis_output",
        "urn:promptfleet:analysis-agent",
    );
    assert_eq!(outgoing.event_type, "com.example.analysis_output");
    assert_eq!(outgoing.source, "urn:promptfleet:analysis-agent");
    assert_eq!(outgoing.data.unwrap().alert_id, "alert-42");
}
