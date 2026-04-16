use std::sync::Arc;
use std::{future::Future, pin::Pin};

use serde_json::Value;
use serde_json::json;

use crate::agent::tools::{ToolExecutor, ToolKind, ToolSpec};

use super::adapter::{SharedSubAgentAdapter, SubAgentAdapter};

type ResultTransformFuture = Pin<Box<dyn Future<Output = Result<Value, String>> + Send>>;
type ResultTransformer = Arc<dyn Fn(Value) -> ResultTransformFuture + Send + Sync>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DelegationMode {
    Streaming,
    Synchronous,
}

pub struct SubAgentToolBuilder {
    agent_name: String,
    agent_url: String,
    tool_name: String,
    tool_description: String,
    input_schema: Value,
    delegation_mode: DelegationMode,
    adapter: SharedSubAgentAdapter,
    result_transformer: Option<ResultTransformer>,
    emit_handoff: bool,
    headers: Vec<(String, String)>,
}

impl SubAgentToolBuilder {
    pub fn new(
        agent_name: impl Into<String>,
        agent_url: impl Into<String>,
        tool_name: impl Into<String>,
    ) -> Self {
        Self {
            agent_name: agent_name.into(),
            agent_url: agent_url.into(),
            tool_name: tool_name.into(),
            tool_description: "Delegate work to a remote sub-agent".to_string(),
            input_schema: serde_json::json!({"type":"object","additionalProperties":true}),
            delegation_mode: DelegationMode::Streaming,
            adapter: default_adapter(),
            result_transformer: None,
            emit_handoff: true,
            headers: Vec::new(),
        }
    }

    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.tool_description = description.into();
        self
    }

    pub fn schema(mut self, schema: Value) -> Self {
        self.input_schema = schema;
        self
    }

    pub fn delegation_mode(mut self, mode: DelegationMode) -> Self {
        self.delegation_mode = mode;
        self
    }

    pub fn adapter(mut self, adapter: Arc<dyn SubAgentAdapter>) -> Self {
        self.adapter = adapter;
        self
    }

    pub fn result_transformer(mut self, transformer: ResultTransformer) -> Self {
        self.result_transformer = Some(transformer);
        self
    }

    pub fn emit_handoff(mut self, enabled: bool) -> Self {
        self.emit_handoff = enabled;
        self
    }

    pub fn header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((key.into(), value.into()));
        self
    }

    pub fn build(self) -> ToolSpec {
        let agent_name = self.agent_name.clone();
        let agent_url = self.agent_url.clone();
        let mode = self.delegation_mode;
        let adapter = self.adapter.clone();
        let result_transformer = self.result_transformer.clone();
        let emit_handoff = self.emit_handoff;
        let headers = self.headers.clone();

        ToolSpec {
            name: self.tool_name,
            description: Some(self.tool_description),
            parameters: self.input_schema,
            kind: ToolKind::A2aDelegate,
            strict: true,
            parallel_ok: false,
            executor: ToolExecutor::WithContext(Arc::new(move |args, ctx| {
                let agent_name = agent_name.clone();
                let agent_url = agent_url.clone();
                let adapter = adapter.clone();
                let result_transformer = result_transformer.clone();
                let headers = headers.clone();
                let agent_name_for_error = agent_name.clone();
                Box::pin(async move {
                    let result = adapter
                        .execute(
                            mode,
                            agent_name,
                            agent_url,
                            args,
                            headers,
                            emit_handoff,
                            ctx,
                        )
                        .await;
                    match (result, result_transformer) {
                        (Ok(value), Some(transformer)) => {
                            let original = value.clone();
                            transformer(value).await.map_err(|message| {
                                structured_transform_error(
                                    &agent_name_for_error,
                                    &original,
                                    &message,
                                )
                            })
                        }
                        (other, _) => other,
                    }
                })
            })),
        }
    }
}

fn default_adapter() -> SharedSubAgentAdapter {
    #[cfg(feature = "sub-agents")]
    {
        Arc::new(crate::a2a_sub_agent::A2aSubAgentAdapter::default())
    }

    #[cfg(not(feature = "sub-agents"))]
    {
        struct MissingAdapter;
        impl SubAgentAdapter for MissingAdapter {
            fn execute(
                &self,
                _mode: DelegationMode,
                _agent_name: String,
                _agent_url: String,
                _args: Value,
                _headers: Vec<(String, String)>,
                _emit_handoff: bool,
                _ctx: crate::agent::tool_context::ToolContext,
            ) -> super::adapter::SubAgentFuture {
                Box::pin(async { Err("sub-agent adapter not available".to_string()) })
            }
        }
        Arc::new(MissingAdapter)
    }
}

fn structured_transform_error(agent_name: &str, value: &Value, message: &str) -> String {
    let tool_call_id = value
        .get("tool_call_id")
        .and_then(Value::as_str)
        .or_else(|| value.get("delegation_id").and_then(Value::as_str));
    let task_id = value.get("task_id").and_then(Value::as_str);

    json!({
        "source": "sub_agent",
        "error_kind": "result_transform_failed",
        "message": message,
        "agent_name": value
            .get("agent_name")
            .and_then(Value::as_str)
            .unwrap_or(agent_name),
        "task_id": task_id,
        "tool_call_id": tool_call_id,
        "delegation_id": tool_call_id,
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_subagent_tool_spec() {
        let spec = SubAgentToolBuilder::new(
            "planner",
            "http://planner.default.svc.cluster.local:3000/jsonrpc",
            "delegate_planner",
        )
        .description("Delegate planning")
        .schema(serde_json::json!({"type":"object"}))
        .delegation_mode(DelegationMode::Synchronous)
        .build();

        assert_eq!(spec.name, "delegate_planner");
        assert_eq!(spec.kind, ToolKind::A2aDelegate);
        assert!(matches!(spec.executor, ToolExecutor::WithContext(_)));
    }
}
