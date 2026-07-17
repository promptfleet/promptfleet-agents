#![cfg_attr(not(feature = "context-window"), allow(dead_code, unused_imports))]

use std::collections::HashMap;
use std::sync::Arc;

use agent_core::{AgentMessage, ContentPart, Role, TaskPhase};
use async_trait::async_trait;
use serde_json::Value;

use super::config::{HistoryPolicyConfig, HistoryPolicyMode, HistoryStrategyKind};
use super::message::{ContinuationState, MessageContext, TaskContext};
use super::response::RuntimeResponse;
use crate::agent::task_store::ContinuationStrategyDescriptor;
use crate::error::SdkResult;

pub(crate) struct PreparedHistory {
    pub system_message: Option<String>,
    pub retained_history: Vec<AgentMessage>,
}

pub(crate) struct ContinuationUpdate {
    pub strategy: ContinuationStrategyDescriptor,
    pub payload: Value,
}

#[async_trait]
pub(crate) trait HistoryPolicyRuntime: Send + Sync {
    async fn prepare_turn(
        &self,
        task_ctx: &TaskContext,
        msg_ctx: &MessageContext,
        system_message: Option<&str>,
    ) -> SdkResult<PreparedHistory>;

    async fn complete_turn(
        &self,
        task_ctx: Option<&TaskContext>,
        msg_ctx: &MessageContext,
        response: &RuntimeResponse,
        system_message: Option<&str>,
    ) -> SdkResult<Option<ContinuationUpdate>>;

    fn descriptor(&self) -> ContinuationStrategyDescriptor;
}

pub(crate) struct PassThroughHistoryPolicyRuntime;

#[async_trait]
impl HistoryPolicyRuntime for PassThroughHistoryPolicyRuntime {
    async fn prepare_turn(
        &self,
        task_ctx: &TaskContext,
        _msg_ctx: &MessageContext,
        system_message: Option<&str>,
    ) -> SdkResult<PreparedHistory> {
        Ok(PreparedHistory {
            system_message: system_message.map(ToOwned::to_owned),
            retained_history: task_ctx.runtime_history.clone(),
        })
    }

    async fn complete_turn(
        &self,
        _task_ctx: Option<&TaskContext>,
        _msg_ctx: &MessageContext,
        _response: &RuntimeResponse,
        _system_message: Option<&str>,
    ) -> SdkResult<Option<ContinuationUpdate>> {
        Ok(None)
    }

    fn descriptor(&self) -> ContinuationStrategyDescriptor {
        ContinuationStrategyDescriptor::default()
    }
}

#[cfg(feature = "context-window")]
pub(crate) struct ContextWindowHistoryPolicyRuntime {
    agent_id: String,
    policy: HistoryPolicyConfig,
    summarizer: Option<Arc<dyn llm_context_core::history::Summarizer>>,
    memory: Option<Arc<dyn llm_context_core::LongTermMemory>>,
}

#[cfg(feature = "context-window")]
impl ContextWindowHistoryPolicyRuntime {
    pub(crate) fn new(
        agent_id: String,
        policy: HistoryPolicyConfig,
        summarizer: Option<Arc<dyn llm_context_core::history::Summarizer>>,
        memory: Option<Arc<dyn llm_context_core::LongTermMemory>>,
    ) -> Self {
        Self {
            agent_id,
            policy,
            summarizer,
            memory,
        }
    }

    fn build_manager(&self, task_ctx: Option<&TaskContext>) -> llm_context_core::HistoryManager {
        use llm_context_core::{HistoryManager, HistoryManagerConfig};

        let mut manager = HistoryManager::new(HistoryManagerConfig {
            strategy: map_strategy_kind(&self.policy.strategy),
            enable_summarization: self.policy.enable_summarization,
            enable_long_term_memory: self.policy.enable_long_term_memory,
            recall_top_k: self.policy.recall_top_k,
            memory_token_budget: self.policy.memory_token_budget,
        });

        if let Some(summarizer) = &self.summarizer {
            manager = manager.with_summarizer(summarizer.clone());
        }
        if let Some(memory) = &self.memory {
            manager = manager.with_memory(memory.clone());
        }

        let persisted_summaries = task_ctx
            .and_then(|ctx| ctx.continuation.as_ref())
            .and_then(|continuation| {
                (continuation.strategy_kind == self.descriptor().kind
                    && continuation.strategy_version == self.descriptor().version)
                    .then(|| summaries_from_payload(&continuation.payload))
            })
            .unwrap_or_default();

        manager.with_summaries(persisted_summaries)
    }

    fn budget(&self, system_message: Option<&str>) -> llm_context_core::budget::ContextBudget {
        llm_context_core::budget::ContextBudget::new(
            self.policy.context_window_tokens,
            self.policy.max_output_tokens,
            system_message,
            &[],
        )
    }

    fn memory_filters(&self, task_ctx: &TaskContext) -> llm_context_core::MemoryFilters {
        llm_context_core::MemoryFilters {
            agent_id: Some(self.agent_id.clone()),
            conversation_id: task_ctx.context_id.clone(),
            ..Default::default()
        }
    }
}

#[cfg(feature = "context-window")]
#[async_trait]
impl HistoryPolicyRuntime for ContextWindowHistoryPolicyRuntime {
    async fn prepare_turn(
        &self,
        task_ctx: &TaskContext,
        msg_ctx: &MessageContext,
        system_message: Option<&str>,
    ) -> SdkResult<PreparedHistory> {
        let history_json = runtime_history_to_provider_json(&task_ctx.runtime_history);
        let current_turn = vec![user_turn_json(msg_ctx)];
        let prepared = self
            .build_manager(Some(task_ctx))
            .prepare_messages(
                &self.budget(system_message),
                system_message,
                &history_json,
                &current_turn,
                &self.memory_filters(task_ctx),
            )
            .await;

        let (system_message, retained_history_json) =
            split_prepared_messages(prepared, &current_turn[0]);

        Ok(PreparedHistory {
            system_message,
            retained_history: retained_history_json
                .iter()
                .filter_map(provider_json_to_agent_message)
                .collect(),
        })
    }

    async fn complete_turn(
        &self,
        task_ctx: Option<&TaskContext>,
        msg_ctx: &MessageContext,
        response: &RuntimeResponse,
        system_message: Option<&str>,
    ) -> SdkResult<Option<ContinuationUpdate>> {
        let Some(task_ctx) = task_ctx else {
            return Ok(None);
        };

        let RuntimeResponse::Task(runtime) = response else {
            return Ok(None);
        };
        if runtime.phase != TaskPhase::Completed {
            return Ok(None);
        }

        let history_json = runtime_history_to_provider_json(&task_ctx.runtime_history);
        let current_turn = vec![user_turn_json(msg_ctx)];
        let prepared = self
            .build_manager(Some(task_ctx))
            .prepare_messages(
                &self.budget(system_message),
                system_message,
                &history_json,
                &current_turn,
                &self.memory_filters(task_ctx),
            )
            .await;
        let (_, retained_history_json) = split_prepared_messages(prepared, &current_turn[0]);
        let evicted = diff_evicted_messages(&history_json, &retained_history_json);

        let mut manager = self.build_manager(Some(task_ctx));
        manager
            .on_turn_complete(
                &evicted,
                &self.agent_id,
                None,
                task_ctx.context_id.as_deref(),
            )
            .await;

        let payload = payload_from_summaries(manager.summaries());

        Ok(Some(ContinuationUpdate {
            strategy: self.descriptor(),
            payload,
        }))
    }

    fn descriptor(&self) -> ContinuationStrategyDescriptor {
        ContinuationStrategyDescriptor {
            kind: history_strategy_name(&self.policy.strategy).to_string(),
            version: 1,
            composition: None,
        }
    }
}

pub(crate) fn default_runtime() -> Arc<dyn HistoryPolicyRuntime> {
    Arc::new(PassThroughHistoryPolicyRuntime)
}

#[cfg(feature = "context-window")]
pub(crate) fn build_history_policy_runtime(
    agent_id: String,
    policy: Option<&HistoryPolicyConfig>,
    summarizer: Option<Arc<dyn llm_context_core::history::Summarizer>>,
    memory: Option<Arc<dyn llm_context_core::LongTermMemory>>,
) -> Arc<dyn HistoryPolicyRuntime> {
    match policy {
        Some(policy) if policy.mode == HistoryPolicyMode::HistoryManager => Arc::new(
            ContextWindowHistoryPolicyRuntime::new(agent_id, policy.clone(), summarizer, memory),
        ),
        _ => default_runtime(),
    }
}

#[cfg(not(feature = "context-window"))]
pub(crate) fn build_history_policy_runtime(
    _agent_id: String,
    _policy: Option<&HistoryPolicyConfig>,
) -> Arc<dyn HistoryPolicyRuntime> {
    default_runtime()
}

fn provider_message_to_text(message: &AgentMessage) -> Option<String> {
    let mut text_content = String::new();
    for part in &message.parts {
        if let ContentPart::Text(text) = part {
            if !text_content.is_empty() {
                text_content.push('\n');
            }
            text_content.push_str(text);
        }
    }
    (!text_content.is_empty()).then_some(text_content)
}

fn runtime_history_to_provider_json(history: &[AgentMessage]) -> Vec<Value> {
    history
        .iter()
        .filter_map(|message| {
            provider_message_to_text(message).map(|content| {
                serde_json::json!({
                    "role": role_name(&message.role),
                    "content": content,
                })
            })
        })
        .collect()
}

fn user_turn_json(msg_ctx: &MessageContext) -> Value {
    let user_text = msg_ctx
        .text_content
        .clone()
        .unwrap_or_else(|| msg_ctx.runtime_message.text_content().unwrap_or_default());
    let combined_user = match render_runtime_data_parts(&msg_ctx.runtime_message) {
        Some(dctx) if !dctx.is_empty() => format!("{}\n\nUser: {}", dctx, user_text),
        _ => user_text,
    };
    serde_json::json!({ "role": "user", "content": combined_user })
}

fn render_runtime_data_parts(message: &AgentMessage) -> Option<String> {
    let mut chunks = Vec::new();
    for part in &message.parts {
        if let ContentPart::Data(value) = part {
            if let Ok(json) = serde_json::to_string_pretty(value) {
                chunks.push(json);
            }
        }
    }

    if chunks.is_empty() {
        None
    } else {
        Some(chunks.join("\n"))
    }
}

fn split_prepared_messages(
    mut prepared: Vec<Value>,
    current_turn: &Value,
) -> (Option<String>, Vec<Value>) {
    let system_message = if prepared
        .first()
        .and_then(|value| value.get("role").and_then(Value::as_str))
        == Some("system")
    {
        let system = prepared
            .remove(0)
            .get("content")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        system
    } else {
        None
    };

    if prepared.last() == Some(current_turn) {
        prepared.pop();
    }

    (system_message, prepared)
}

fn provider_json_to_agent_message(message: &Value) -> Option<AgentMessage> {
    let role = match message.get("role").and_then(Value::as_str) {
        Some("user") => Role::User,
        Some("assistant") => Role::Agent,
        Some("system") => Role::System,
        _ => return None,
    };
    let content = message.get("content").and_then(Value::as_str)?;
    Some(AgentMessage::new(
        role,
        vec![ContentPart::Text(content.to_string())],
    ))
}

fn diff_evicted_messages(full_history: &[Value], retained_history: &[Value]) -> Vec<Value> {
    let mut retained_counts: HashMap<String, usize> = HashMap::new();
    for message in retained_history {
        let key = message.to_string();
        *retained_counts.entry(key).or_default() += 1;
    }

    let mut evicted = Vec::new();
    for message in full_history {
        let key = message.to_string();
        match retained_counts.get_mut(&key) {
            Some(count) if *count > 0 => {
                *count -= 1;
            }
            _ => evicted.push(message.clone()),
        }
    }

    evicted
}

#[cfg(feature = "context-window")]
fn map_strategy_kind(kind: &HistoryStrategyKind) -> llm_context_core::ContextStrategyKind {
    match kind {
        HistoryStrategyKind::SlidingWindow => llm_context_core::ContextStrategyKind::SlidingWindow,
        HistoryStrategyKind::SlidingWindowWithSummary => {
            llm_context_core::ContextStrategyKind::SlidingWindowWithSummary
        }
        HistoryStrategyKind::PriorityBased => llm_context_core::ContextStrategyKind::PriorityBased,
    }
}

fn history_strategy_name(kind: &HistoryStrategyKind) -> &'static str {
    match kind {
        HistoryStrategyKind::SlidingWindow => "sliding_window",
        HistoryStrategyKind::SlidingWindowWithSummary => "sliding_window_with_summary",
        HistoryStrategyKind::PriorityBased => "priority_based",
    }
}

fn role_name(role: &Role) -> &'static str {
    match role {
        Role::User => "user",
        Role::Agent => "assistant",
        Role::System => "system",
        Role::Tool => "tool",
    }
}

fn summaries_from_payload(payload: &Value) -> Vec<String> {
    payload
        .get("summaries")
        .and_then(Value::as_array)
        .map(|summaries| {
            summaries
                .iter()
                .filter_map(Value::as_str)
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn payload_from_summaries(summaries: &[String]) -> Value {
    if summaries.is_empty() {
        Value::Null
    } else {
        serde_json::json!({ "summaries": summaries })
    }
}

pub(crate) fn continuation_state_from_snapshot(
    snapshot: &crate::agent::task_store::ContinuationSnapshot,
) -> ContinuationState {
    ContinuationState {
        source_revision: snapshot.source_revision,
        strategy_kind: snapshot.strategy.kind.clone(),
        strategy_version: snapshot.strategy.version,
        strategy_composition: snapshot.strategy.composition.clone(),
        payload: snapshot.payload.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::RuntimeResponse;
    use crate::agent::config::HistoryPolicyConfig;
    use crate::agent::response::{RuntimeContinuationUpdate, RuntimeTask};

    fn task_ctx_with_history() -> TaskContext {
        TaskContext {
            task_id: "task-1".to_string(),
            context_id: Some("ctx-1".to_string()),
            runtime_history: vec![
                AgentMessage::user_text("old question with a lot of padding to consume budget"),
                AgentMessage::agent_text("old answer with a lot of padding to consume budget"),
                AgentMessage::user_text("newer question that should survive trimming"),
                AgentMessage::agent_text("newer answer that should survive trimming"),
            ],
            task_phase: TaskPhase::Working,
            artifacts: Vec::new(),
            task_metadata: HashMap::new(),
            created_at: None,
            updated_at: None,
            continuation: None,
        }
    }

    fn user_msg_ctx(text: &str) -> MessageContext {
        MessageContext::from_runtime_message(
            AgentMessage::user_text(text),
            HashMap::new(),
            HashMap::new(),
            false,
            None,
        )
    }

    #[tokio::test]
    async fn pass_through_runtime_preserves_history_and_system_message() {
        let runtime = PassThroughHistoryPolicyRuntime;
        let task_ctx = task_ctx_with_history();
        let prepared = runtime
            .prepare_turn(&task_ctx, &user_msg_ctx("current turn"), Some("system"))
            .await
            .expect("prepare turn should succeed");

        assert_eq!(prepared.system_message.as_deref(), Some("system"));
        assert_eq!(
            prepared.retained_history.len(),
            task_ctx.runtime_history.len()
        );
    }

    #[cfg(feature = "context-window")]
    #[tokio::test]
    async fn history_manager_runtime_trims_history_but_keeps_system_message() {
        let runtime = ContextWindowHistoryPolicyRuntime::new(
            "agent-1".to_string(),
            HistoryPolicyConfig {
                mode: HistoryPolicyMode::HistoryManager,
                strategy: HistoryStrategyKind::SlidingWindow,
                context_window_tokens: 60,
                max_output_tokens: 16,
                ..Default::default()
            },
            None,
            None,
        );
        let task_ctx = task_ctx_with_history();

        let prepared = runtime
            .prepare_turn(
                &task_ctx,
                &user_msg_ctx("current turn"),
                Some("system instructions"),
            )
            .await
            .expect("prepare turn should succeed");

        assert_eq!(
            prepared.system_message.as_deref(),
            Some("system instructions")
        );
        assert!(
            prepared.retained_history.len() < task_ctx.runtime_history.len(),
            "history manager should trim oversized history"
        );
    }

    #[cfg(feature = "context-window")]
    #[tokio::test]
    async fn history_manager_runtime_completed_turn_emits_summary_payload() {
        let runtime = ContextWindowHistoryPolicyRuntime::new(
            "agent-1".to_string(),
            HistoryPolicyConfig {
                mode: HistoryPolicyMode::HistoryManager,
                strategy: HistoryStrategyKind::SlidingWindowWithSummary,
                context_window_tokens: 60,
                max_output_tokens: 16,
                enable_summarization: true,
                ..Default::default()
            },
            Some(Arc::new(
                llm_context_core::history::ExtractiveSnippets::default(),
            )),
            None,
        );
        let task_ctx = task_ctx_with_history();
        let response = RuntimeResponse::Task(RuntimeTask {
            task_id: task_ctx.task_id.clone(),
            context_id: task_ctx.context_id.clone().unwrap(),
            history: task_ctx.runtime_history.clone(),
            artifacts: Vec::new(),
            metadata: HashMap::new(),
            phase: TaskPhase::Completed,
            status_text: Some("done".to_string()),
            continuation_update: Some(RuntimeContinuationUpdate {
                strategy_kind: "unused".to_string(),
                strategy_version: 0,
                strategy_composition: None,
                payload: Value::Null,
            }),
        });

        let update = runtime
            .complete_turn(
                Some(&task_ctx),
                &user_msg_ctx("current turn"),
                &response,
                Some("system instructions"),
            )
            .await
            .expect("complete turn should succeed")
            .expect("completed turn should emit an update");

        assert_eq!(update.strategy.kind, "sliding_window_with_summary");
        assert!(
            update
                .payload
                .get("summaries")
                .and_then(Value::as_array)
                .is_some_and(|summaries| !summaries.is_empty()),
            "summary-capable policy should persist summaries in payload"
        );
    }
}
