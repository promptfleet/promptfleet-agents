//! PromptFleetExecutor — bridges UMAO orchestration to real A2A agents.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

use serde_json::Value;
use tracing::{debug, warn};

use agent_sdk::a2a::A2aClient;
use umao_core::error::UmaoError;
use umao_core::graph::ir::CfbType;
use umao_executor::traits::AsyncNodeExecutor;
use umao_executor::types::{NodeOutcome, NodeOutput};

/// Dispatches UMAO graph nodes to real A2A agents via `agent_sdk`.
///
/// Delegate nodes are routed to the agent matching the node's
/// `target_agent` trait. Non-delegate CFB types (Select, Aggregate,
/// Monitor, FlowControl) use deterministic responses from `umao_core`.
pub struct PromptFleetExecutor {
    agents: HashMap<String, String>,
}

impl PromptFleetExecutor {
    pub fn new() -> Self {
        Self {
            agents: HashMap::new(),
        }
    }

    /// Register an agent by name → endpoint URL.
    pub fn register(&mut self, name: &str, endpoint: &str) {
        self.agents.insert(name.to_string(), endpoint.to_string());
    }

    /// Build from an iterator of (name, endpoint) pairs.
    pub fn from_agents(agents: impl IntoIterator<Item = (String, String)>) -> Self {
        Self {
            agents: agents.into_iter().collect(),
        }
    }
}

impl Default for PromptFleetExecutor {
    fn default() -> Self {
        Self::new()
    }
}

impl AsyncNodeExecutor for PromptFleetExecutor {
    fn execute_node<'a>(
        &'a self,
        node_id: &'a str,
        cfb_type: CfbType,
        inputs: &'a HashMap<String, Value>,
        _system_prompt: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<NodeOutput, UmaoError>> + Send + 'a>> {
        Box::pin(async move {
            match cfb_type {
                CfbType::Delegate => self.execute_delegate(node_id, inputs).await,
                CfbType::Select => {
                    let data = umao_core::deterministic::deterministic_monitor_response(
                        &serde_json::to_string(inputs).unwrap_or_default(),
                    );
                    Ok(NodeOutput::completed(data, 0.0))
                }
                CfbType::Aggregate => {
                    let mut sections = Vec::new();
                    for (port, value) in inputs {
                        if port.starts_with("__") {
                            continue;
                        }
                        let text = match value.as_str() {
                            Some(s) => s.to_string(),
                            None => serde_json::to_string(value).unwrap_or_default(),
                        };
                        sections.push(format!("[{port}] {text}"));
                    }
                    let combined = if sections.is_empty() {
                        "No inputs to aggregate.".to_string()
                    } else {
                        format!(
                            "## Aggregated Results ({} sources)\n\n{}",
                            sections.len(),
                            sections.join("\n\n")
                        )
                    };
                    Ok(NodeOutput::completed(serde_json::json!(combined), 0.0))
                }
                CfbType::Monitor => {
                    let data = umao_core::deterministic::deterministic_monitor_response(
                        &serde_json::to_string(inputs).unwrap_or_default(),
                    );
                    Ok(NodeOutput::completed(data, 0.0))
                }
                CfbType::FlowControl => {
                    let data = umao_core::deterministic::deterministic_flow_control_response(
                        &serde_json::to_string(inputs).unwrap_or_default(),
                    );
                    Ok(NodeOutput::completed(data, 0.0))
                }
            }
        })
    }
}

impl PromptFleetExecutor {
    async fn execute_delegate(
        &self,
        node_id: &str,
        inputs: &HashMap<String, Value>,
    ) -> Result<NodeOutput, UmaoError> {
        let agent_name = inputs
            .get("__node_traits")
            .and_then(|t| t.get("target_agent"))
            .and_then(|v| v.as_str())
            .unwrap_or(node_id);

        let endpoint = self.agents.get(agent_name).ok_or_else(|| {
            UmaoError::ExecutionError(format!(
                "No endpoint registered for agent '{}' (node {})",
                agent_name, node_id
            ))
        })?;

        debug!(node_id, agent_name, endpoint, "Delegating to A2A agent");

        let client = A2aClient::new(endpoint).map_err(|e| {
            UmaoError::ExecutionError(format!("Failed to create A2A client: {}", e))
        })?;

        let payload = serde_json::to_string(inputs).unwrap_or_default();
        let response = client.send_message(&payload, None).await.map_err(|e| {
            warn!(node_id, agent_name, error = %e, "Delegate call failed");
            UmaoError::ExecutionError(format!("Agent '{}' call failed: {}", agent_name, e))
        })?;

        let outcome = if response.get("input_required").is_some() {
            let question = response["input_required"]["question"]
                .as_str()
                .unwrap_or("Agent needs more input")
                .to_string();
            let continuation = response["input_required"]["continuation_id"]
                .as_str()
                .unwrap_or("")
                .to_string();
            NodeOutcome::InputRequired {
                agent_question: question,
                continuation_id: continuation,
            }
        } else {
            NodeOutcome::Completed
        };

        Ok(NodeOutput {
            data: response,
            budget_used: 0.0,
            outcome,
            idempotency_key: None,
            memoized: false,
            cache_key: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_and_lookup() {
        let mut exec = PromptFleetExecutor::new();
        exec.register("agent-a", "http://localhost:3001");
        exec.register("agent-b", "http://localhost:3002");
        assert_eq!(exec.agents.len(), 2);
        assert_eq!(exec.agents["agent-a"], "http://localhost:3001");
    }

    #[test]
    fn from_agents_builds_map() {
        let exec = PromptFleetExecutor::from_agents(vec![
            ("a".into(), "http://a:3000".into()),
            ("b".into(), "http://b:3000".into()),
        ]);
        assert_eq!(exec.agents.len(), 2);
    }

    #[tokio::test]
    async fn select_node_returns_deterministic() {
        let exec = PromptFleetExecutor::new();
        let inputs = HashMap::new();
        let result = exec
            .execute_node("sel_1", CfbType::Select, &inputs, "")
            .await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().outcome, NodeOutcome::Completed);
    }

    #[tokio::test]
    async fn delegate_without_endpoint_fails() {
        let exec = PromptFleetExecutor::new();
        let mut inputs = HashMap::new();
        inputs.insert(
            "__node_traits".into(),
            serde_json::json!({"target_agent": "unknown-agent"}),
        );
        let result = exec
            .execute_node("del_1", CfbType::Delegate, &inputs, "")
            .await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("No endpoint registered"));
    }

    /// Start a minimal A2A-compatible HTTP server that handles SendMessage JSON-RPC.
    /// Returns the port the server is listening on.
    async fn start_mock_agent(name: &'static str) -> u16 {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    break;
                };
                let name = name;
                tokio::spawn(async move {
                    use tokio::io::{AsyncReadExt, AsyncWriteExt};

                    let mut stream = stream;
                    let mut buf = vec![0u8; 16384];
                    let n = stream.read(&mut buf).await.unwrap_or(0);
                    let request = String::from_utf8_lossy(&buf[..n]);

                    if let Some(body_start) = request.find("\r\n\r\n") {
                        let body = &request[body_start + 4..];
                        if let Ok(rpc) = serde_json::from_str::<serde_json::Value>(body) {
                            let id = rpc.get("id").cloned().unwrap_or(serde_json::json!(null));
                            let method = rpc.get("method").and_then(|v| v.as_str()).unwrap_or("");

                            let result = if method == "SendMessage" || method == "message/send" {
                                let text = rpc
                                    .pointer("/params/message/parts/0/text")
                                    .and_then(|t| t.as_str())
                                    .unwrap_or("no input");

                                let agent_response = if name == "researcher" {
                                    format!(
                                        "Research on input: found 3 key patterns, \
                                         hybrid approach leads with 87% accuracy. \
                                         Input digest: {} chars.",
                                        text.len()
                                    )
                                } else {
                                    format!(
                                        "Synthesis of upstream data: \
                                         recommend hybrid approach for production. \
                                         Upstream size: {} chars.",
                                        text.len()
                                    )
                                };

                                serde_json::json!({
                                    "jsonrpc": "2.0",
                                    "id": id,
                                    "result": {
                                        "id": format!("task-{name}-001"),
                                        "contextId": "ctx-001",
                                        "status": {
                                            "state": "completed",
                                            "message": {
                                                "role": "agent",
                                                "parts": [{"text": agent_response}]
                                            }
                                        }
                                    }
                                })
                            } else {
                                serde_json::json!({
                                    "jsonrpc": "2.0",
                                    "id": id,
                                    "error": {"code": -32601, "message": "Method not found"}
                                })
                            };

                            let body = serde_json::to_string(&result).unwrap();
                            let response = format!(
                                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                                body.len(),
                                body
                            );
                            let _ = stream.write_all(response.as_bytes()).await;
                        }
                    }
                });
            }
        });

        port
    }

    #[tokio::test]
    async fn delegate_calls_real_agent_server() {
        let port = start_mock_agent("researcher").await;

        let mut exec = PromptFleetExecutor::new();
        exec.register("researcher", &format!("http://127.0.0.1:{port}"));

        let mut inputs = HashMap::new();
        inputs.insert(
            "__node_traits".into(),
            serde_json::json!({"target_agent": "researcher"}),
        );
        inputs.insert("task_spec".into(), serde_json::json!("Analyze AI safety"));

        let result = exec
            .execute_node("DEL_1", CfbType::Delegate, &inputs, "")
            .await;

        assert!(result.is_ok(), "Delegate call failed: {:?}", result.err());
        let output = result.unwrap();
        assert_eq!(output.outcome, NodeOutcome::Completed);
        let data_str = serde_json::to_string(&output.data).unwrap();
        assert!(
            data_str.contains("hybrid approach"),
            "Response should contain meaningful research content: {data_str}"
        );
    }

    #[tokio::test]
    async fn full_sequential_graph_orchestration() {
        use std::sync::Arc;
        use umao_core::events::UmaoEvent;
        use umao_core::graph::ir::GraphIR;
        use umao_executor::orchestrator;
        use umao_executor::types::ExecutionStatus;

        let researcher_port = start_mock_agent("researcher").await;
        let synthesizer_port = start_mock_agent("synthesizer").await;

        let mut exec = PromptFleetExecutor::new();
        exec.register("researcher", &format!("http://127.0.0.1:{researcher_port}"));
        exec.register(
            "synthesizer",
            &format!("http://127.0.0.1:{synthesizer_port}"),
        );

        let graph_json = serde_json::json!({
            "graph_id": "integration-test-sequential",
            "version": 1,
            "nodes": [
                {
                    "fb_id": "DELEGATE_researcher",
                    "cfb_type": "Delegate",
                    "ports": {
                        "inputs": ["task_spec"],
                        "outputs": ["task_results"]
                    },
                    "guard": { "retry": 1, "budget_usd": 1.0, "timeout_ms": 10000 },
                    "traits": {
                        "target_agent": "researcher",
                        "task_prompt": "Research the topic."
                    }
                },
                {
                    "fb_id": "DELEGATE_synthesizer",
                    "cfb_type": "Delegate",
                    "ports": {
                        "inputs": ["task_spec"],
                        "outputs": ["task_results"]
                    },
                    "guard": { "retry": 1, "budget_usd": 1.0, "timeout_ms": 10000 },
                    "traits": {
                        "target_agent": "synthesizer",
                        "task_prompt": "Synthesize findings."
                    }
                }
            ],
            "edges": [
                {
                    "from": { "fb_id": "DELEGATE_researcher", "port": "task_results" },
                    "to": { "fb_id": "DELEGATE_synthesizer", "port": "task_spec" },
                    "kind": "depends_on"
                }
            ],
            "meta": {
                "spec_version": "umao-1.0",
                "created_by": "integration-test",
                "description": "Sequential: researcher -> synthesizer"
            },
            "outputs": []
        });

        let graph: GraphIR = serde_json::from_value(graph_json).unwrap();

        let events: Arc<std::sync::Mutex<Vec<String>>> =
            Arc::new(std::sync::Mutex::new(Vec::new()));
        let events_clone = events.clone();
        let event_sink: Option<umao_core::events::EventSink> =
            Some(Arc::new(move |event: UmaoEvent| {
                let label = match &event {
                    UmaoEvent::NodeStateChanged {
                        fb_id, new_state, ..
                    } => {
                        format!("state:{fb_id}={new_state}")
                    }
                    UmaoEvent::NodeCompleted { fb_id, .. } => format!("completed:{fb_id}"),
                    UmaoEvent::ExecutionComplete { .. } => "execution_complete".to_string(),
                    UmaoEvent::NodeFailed { fb_id, error, .. } => {
                        format!("failed:{fb_id}={error}")
                    }
                    _ => format!("{event:?}"),
                };
                events_clone.lock().unwrap().push(label);
            }));

        let result = orchestrator::execute_async(&graph, &exec, &event_sink).await;

        assert!(result.is_ok(), "Orchestration failed: {:?}", result.err());
        let exec_result = result.unwrap();
        assert_eq!(
            exec_result.status,
            ExecutionStatus::Completed,
            "Expected Completed, got {:?}. Trace: {:?}",
            exec_result.status,
            exec_result.trace
        );
        assert_eq!(exec_result.trace.len(), 2, "Should have 2 step records");
        assert!(
            exec_result.trace[0].node_id == "DELEGATE_researcher",
            "First step should be researcher"
        );
        assert!(
            exec_result.trace[1].node_id == "DELEGATE_synthesizer",
            "Second step should be synthesizer"
        );

        let captured = events.lock().unwrap();
        assert!(
            captured
                .iter()
                .any(|e| e.contains("completed:DELEGATE_researcher")),
            "Should have researcher completed event. Events: {captured:?}"
        );
        assert!(
            captured
                .iter()
                .any(|e| e.contains("completed:DELEGATE_synthesizer")),
            "Should have synthesizer completed event. Events: {captured:?}"
        );
        assert!(
            captured.iter().any(|e| e == "execution_complete"),
            "Should have execution_complete event. Events: {captured:?}"
        );
    }
}
