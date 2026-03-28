//! PromptFleetExecutor — bridges UMAO orchestration to real A2A agents.

use std::collections::HashMap;
use std::pin::Pin;
use std::future::Future;

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
                    Ok(NodeOutput::completed(serde_json::json!(inputs), 0.0))
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
}
