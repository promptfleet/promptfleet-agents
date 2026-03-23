use std::pin::Pin;
use std::sync::Arc;

use serde_json::Value;

use crate::agent::tool_context::ToolContext;

use super::tool::DelegationMode;

pub type SubAgentFuture = Pin<Box<dyn Future<Output = Result<Value, String>> + Send>>;

#[derive(Debug, Clone)]
pub struct SubAgentContext {
    pub agent_name: String,
    pub task_id: String,
    pub tool_call_id: String,
}

pub trait SubAgentAdapter: Send + Sync {
    fn execute(
        &self,
        mode: DelegationMode,
        agent_name: String,
        agent_url: String,
        args: Value,
        headers: Vec<(String, String)>,
        emit_handoff: bool,
        ctx: ToolContext,
    ) -> SubAgentFuture;
}

pub type SharedSubAgentAdapter = Arc<dyn SubAgentAdapter>;
