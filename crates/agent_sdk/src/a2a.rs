//! A2A adapter surface for composing protocol support around [`crate::Agent`].

#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
use std::sync::Arc;
#[cfg(feature = "a2a-server")]
use std::sync::Arc as StdArc;

#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
use futures_util::{stream, Stream, StreamExt};
use log::info;
#[cfg(feature = "a2a-server")]
use serde_json::Value;

use crate::Agent;
use crate::agent::{RuntimeArtifact, RuntimeResponse, RuntimeTask};
#[cfg(feature = "a2a-server")]
use crate::agent::task_store::{
    ContinuationArtifactRef, ContinuationSnapshot, ContinuationStrategyDescriptor, RuntimeTaskStore,
};

pub use a2a_protocol_core::{
    agent::{AgentCapabilities, AgentCard, AgentInterface, AgentSkill},
    data::{Artifact, Message, MessageRole, Part, Task, TaskState, TaskStatus},
    error::{A2AError, A2AResult},
    methods::params::{MessageSendParams, MessageSendResponse, SendMessageRequest, SendMessageResponse},
    A2A_PROTOCOL_VERSION,
};

#[cfg(feature = "event-stream")]
pub use a2a_protocol_core::streaming::StreamResponse;

#[cfg(feature = "llm-engine")]
use crate::agent::llm_orchestrator::{execute_runtime, LlmInvoker, LlmPolicy, LlmRequestDefaults};
#[cfg(feature = "llm-engine")]
use crate::agent::history_policy::default_runtime as default_history_policy_runtime;

#[cfg(feature = "a2a-server")]
pub use crate::a2a_app::A2aApp;
#[cfg(feature = "a2a-client")]
pub use crate::client::{A2aClient, SendMessageOptions};
#[cfg(feature = "a2a-server")]
pub use crate::server::A2aServer;

pub mod conversions {
    pub use crate::conversions::{
        agent_message_from_a2a, agent_message_to_a2a, content_part_from_a2a, content_part_to_a2a,
        role_from_a2a, role_to_a2a, task_phase_from_a2a, task_phase_to_a2a,
    };
}

#[cfg(all(not(target_arch = "wasm32"), feature = "sub-agents"))]
pub mod sub_agent {
    pub use crate::a2a_sub_agent::{A2aMessageBuilder, A2aStreamEvent, A2aSubAgentAdapter, A2aTraceMapper, DefaultA2aTraceMapper};
}

#[cfg(all(feature = "a2a-server", feature = "agent-observability"))]
pub fn app_with_obs(
    agent: Agent,
    obs_cfg: Option<observability::ObservabilityConfig>,
) -> crate::SdkResult<A2aApp> {
    A2aApp::from_agent_with_obs(agent, obs_cfg)
}

#[cfg(feature = "a2a-server")]
pub fn app(agent: Agent) -> crate::SdkResult<A2aApp> {
    A2aApp::from_agent(agent)
}

#[cfg(feature = "a2a-server")]
pub fn server(agent: Agent) -> crate::SdkResult<A2aServer> {
    A2aServer::with_a2a_methods(agent)
}

#[cfg(feature = "a2a-server")]
struct MirroredTaskStorage {
    storage: StdArc<dyn a2a_protocol_core::services::TaskStorage>,
    runtime_store: StdArc<dyn RuntimeTaskStore>,
}

#[cfg(feature = "a2a-server")]
impl MirroredTaskStorage {
    fn new(
        storage: StdArc<dyn a2a_protocol_core::services::TaskStorage>,
        runtime_store: StdArc<dyn RuntimeTaskStore>,
    ) -> Self {
        Self {
            storage,
            runtime_store,
        }
    }

    fn sync_runtime_snapshot(&self, task: &Task) -> crate::SdkResult<()> {
        let revision = self
            .runtime_store
            .get_task_revision(&task.id)?
            .unwrap_or(0)
            .saturating_add(1);
        self.runtime_store.set_task_revision(&task.id, revision)?;
        let existing = self.runtime_store.get_latest_snapshot(&task.context_id)?;
        let mut snapshot = continuation_snapshot_from_task(task, revision);
        if let Some(existing) = existing.filter(|snapshot| snapshot.task_id == task.id) {
            snapshot.strategy = existing.strategy;
            snapshot.payload = existing.payload;
        }
        self.runtime_store.set_latest_snapshot(
            &task.context_id,
            Some(snapshot),
        )
    }

    fn sync_context_after_removal(&self, context_id: &str) -> crate::SdkResult<()> {
        let latest = self
            .storage
            .get_latest_task_in_context(context_id)
            .map_err(crate::SdkError::from)?
            .map(|task| {
                let revision = self
                    .runtime_store
                    .get_task_revision(&task.id)?
                    .unwrap_or(0);
                Ok::<_, crate::SdkError>(continuation_snapshot_from_task(&task, revision))
            })
            .transpose()?;
        self.runtime_store.set_latest_snapshot(context_id, latest)
    }
}

#[cfg(feature = "a2a-server")]
impl a2a_protocol_core::services::TaskStorage for MirroredTaskStorage {
    fn store_task(&self, task: Task) -> A2AResult<()> {
        let task = self
            .merge_existing_task_state(task)
            .map_err(A2AError::from)?;
        self.storage.store_task(task.clone())?;
        self.sync_runtime_snapshot(&task).map_err(A2AError::from)
    }

    fn get_task(&self, task_id: &str) -> A2AResult<Option<Task>> {
        self.storage.get_task(task_id)
    }

    fn update_task(&self, task: Task) -> A2AResult<()> {
        self.storage.update_task(task.clone())?;
        self.sync_runtime_snapshot(&task).map_err(A2AError::from)
    }

    fn list_tasks(&self) -> A2AResult<Vec<Task>> {
        self.storage.list_tasks()
    }

    fn remove_task(&self, task_id: &str) -> A2AResult<bool> {
        let context_id = self.storage.get_task(task_id)?.map(|task| task.context_id);
        let removed = self.storage.remove_task(task_id)?;
        if removed {
            self.runtime_store
                .delete_task_revision(task_id)
                .map_err(A2AError::from)?;
            if let Some(context_id) = context_id {
                self.sync_context_after_removal(&context_id)
                    .map_err(A2AError::from)?;
            }
        }
        Ok(removed)
    }

    fn task_exists(&self, task_id: &str) -> A2AResult<bool> {
        self.storage.task_exists(task_id)
    }

    fn get_tasks_by_context(&self, context_id: &str) -> A2AResult<Vec<Task>> {
        self.storage.get_tasks_by_context(context_id)
    }

    fn get_latest_task_in_context(&self, context_id: &str) -> A2AResult<Option<Task>> {
        self.storage.get_latest_task_in_context(context_id)
    }

    fn get_context_history(&self, context_id: &str) -> A2AResult<Vec<Message>> {
        self.storage.get_context_history(context_id)
    }

    fn get_or_create_context(
        &self,
        context_id: &str,
    ) -> A2AResult<a2a_protocol_core::services::ConversationContext> {
        self.storage.get_or_create_context(context_id)
    }

    fn update_context_activity(&self, context_id: &str) -> A2AResult<()> {
        self.storage.update_context_activity(context_id)
    }

    fn list_contexts(&self) -> A2AResult<Vec<a2a_protocol_core::services::ConversationContext>> {
        self.storage.list_contexts()
    }
}

#[cfg(feature = "a2a-server")]
impl MirroredTaskStorage {
    fn merge_existing_task_state(&self, mut task: Task) -> crate::SdkResult<Task> {
        let has_new_artifacts = task
            .artifacts
            .as_ref()
            .is_some_and(|artifacts| !artifacts.is_empty());
        if !has_new_artifacts {
            return Ok(task);
        }

        let Some(existing) = self.storage.get_task(&task.id).map_err(crate::SdkError::from)? else {
            return Ok(task);
        };

        task.artifacts = match (existing.artifacts.clone(), task.artifacts.take()) {
            (Some(mut existing_artifacts), Some(mut new_artifacts)) => {
                existing_artifacts.append(&mut new_artifacts);
                Some(existing_artifacts)
            }
            (Some(existing_artifacts), None) => Some(existing_artifacts),
            (None, new_artifacts) => new_artifacts,
        };

        Ok(task)
    }
}

#[cfg(feature = "a2a-server")]
pub(crate) fn server_task_storage(
    agent: &Agent,
) -> crate::SdkResult<std::sync::Arc<dyn a2a_protocol_core::services::TaskStorage>> {
    let runtime_store = agent.runtime_task_store();
    let storage_prefix = agent
        .config()
        .storage_prefix
        .clone()
        .unwrap_or_else(|| format!("local:dev:{}", agent.config().name));

    #[cfg(all(target_arch = "wasm32", feature = "a2a-server"))]
    {
        let protocol_backend: StdArc<dyn a2a_protocol_core::services::TaskStorage> =
            StdArc::new(crate::wasm_kv_task_storage::WasmKvTaskStorage::new(storage_prefix).map_err(
                |e| crate::SdkError::agent_initialization(format!("protocol KV storage init failed: {}", e)),
            )?);
        let storage = StdArc::new(MirroredTaskStorage::new(
            protocol_backend,
            runtime_store,
        ));
        agent.attach_protocol_task_storage(storage.clone())?;
        return Ok(storage);
    }

    #[cfg(all(not(target_arch = "wasm32"), feature = "redis-storage"))]
    {
        let storage_mode = std::env::var("PF_TASK_STORAGE_MODE")
            .unwrap_or_else(|_| "best_effort".to_string())
            .to_ascii_lowercase();
        let require_shared_storage = storage_mode == "required";

        if let Ok(url) = std::env::var("PF_TASK_VALKEY_URL") {
            match crate::redis_task_storage::RedisTaskStorage::new(&url, storage_prefix.clone()) {
                Ok(protocol_backend) => {
                    let protocol_backend: StdArc<dyn a2a_protocol_core::services::TaskStorage> =
                        StdArc::new(protocol_backend);
                    let storage = StdArc::new(MirroredTaskStorage::new(
                        protocol_backend,
                        runtime_store,
                    ));
                    agent.attach_protocol_task_storage(storage.clone())?;
                    return Ok(storage);
                }
                Err(e) => {
                    if require_shared_storage {
                        return Err(crate::SdkError::agent_initialization(format!(
                            "PF_TASK_STORAGE_MODE=required but protocol RedisTaskStorage init failed: {}",
                            e
                        )));
                    }
                    log::warn!(
                        "Protocol RedisTaskStorage init failed ({}), falling back to in-memory",
                        e
                    );
                }
            }
        } else if require_shared_storage {
            return Err(crate::SdkError::agent_initialization(
                "PF_TASK_STORAGE_MODE=required but PF_TASK_VALKEY_URL is not set",
            ));
        }
    }

    #[cfg(all(not(target_arch = "wasm32"), not(feature = "redis-storage")))]
    let _ = &storage_prefix;

    #[cfg(not(target_arch = "wasm32"))]
    {
        let protocol_backend: StdArc<dyn a2a_protocol_core::services::TaskStorage> =
            StdArc::new(a2a_protocol_core::services::InMemoryTaskStorage::new());
        let storage = StdArc::new(MirroredTaskStorage::new(
            protocol_backend,
            runtime_store,
        ));
        agent.attach_protocol_task_storage(storage.clone())?;
        return Ok(storage);
    }
}

/// Build an A2A `AgentCard` from the runtime agent state.
///
/// Produces a v1.0-compliant card with default `supportedInterfaces`,
/// `defaultInputModes`, and `defaultOutputModes`.
pub fn agent_card(agent: &Agent) -> AgentCard {
    let mut card = AgentCard::new(agent.config().name.clone());
    card.description = Some(agent.config().description.clone());
    card.version = Some(agent.config().version.clone());
    card.capabilities = Some(AgentCapabilities {
        streaming: agent.config().streaming,
        push_notifications: false,
        extensions: None,
        extended_agent_card: false,
    });

    let interface_url = match &agent.config().base_url {
        Some(base) => format!("{}/jsonrpc", base.trim_end_matches('/')),
        None => "/jsonrpc".to_string(),
    };
    card.supported_interfaces = Some(vec![AgentInterface {
        url: interface_url,
        protocol_binding: "JSONRPC".to_string(),
        tenant: None,
        protocol_version: Some(A2A_PROTOCOL_VERSION.to_string()),
    }]);
    card.default_input_modes = Some(vec!["text/plain".to_string()]);
    card.default_output_modes = Some(vec!["text/plain".to_string()]);

    for skill_def in agent.skill_registry().get_exposed_skills() {
        card = card.add_skill(skill_def_to_agent_skill(skill_def));
    }

    card
}

pub(crate) fn should_provide_task_context(
    msg_ctx: &crate::agent::MessageContext,
    params: &MessageSendParams,
) -> bool {
    match msg_ctx.message_type {
        crate::agent::MessageType::Data => {
            params.message.context_id.is_some() && !msg_ctx.stateless_mode
        }
        crate::agent::MessageType::Text => !msg_ctx.stateless_mode,
        crate::agent::MessageType::Mixed => true,
        #[cfg(feature = "file-handling")]
        crate::agent::MessageType::File => !msg_ctx.stateless_mode,
    }
}

fn extract_skill_hints(message: &Message) -> std::collections::HashMap<String, serde_json::Value> {
    let mut hints = std::collections::HashMap::new();
    for part in &message.parts {
        if let Some(metadata) = &part.metadata {
            hints.extend(metadata.iter().map(|(k, v)| (k.clone(), v.clone())));
        }
    }
    hints
}

pub(crate) fn message_context_from_params(
    params: &MessageSendParams,
    stateless_mode: bool,
    skill_executor: Option<crate::agent::SkillExecutor>,
) -> crate::agent::MessageContext {
    let runtime_message = crate::conversions::agent_message_from_a2a(params.message.clone());
    crate::agent::MessageContext::from_runtime_message(
        runtime_message,
        extract_skill_hints(&params.message),
        params.metadata.clone().unwrap_or_default(),
        stateless_mode,
        skill_executor,
    )
}

pub(crate) fn runtime_response_into_a2a(
    response: RuntimeResponse,
    history_length: Option<u32>,
) -> A2AResult<MessageSendResponse> {
    match response {
        RuntimeResponse::Message(runtime) => {
            let context_id = runtime
                .context_id
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
            let mut message = crate::conversions::agent_message_to_a2a(runtime.message);
            message.task_id = None;
            message.context_id = Some(context_id);
            message.metadata = runtime.metadata;
            Ok(MessageSendResponse::Message(message))
        }
        RuntimeResponse::Task(runtime) => {
            let status_message = latest_task_status_message(&runtime);
            let mut task = Task::with_id(runtime.task_id.clone(), runtime.context_id.clone());
            let history_to_include = trim_runtime_history(runtime.history, history_length);

            for mut history_message in history_to_include
                .into_iter()
                .map(crate::conversions::agent_message_to_a2a)
            {
                history_message.task_id = Some(runtime.task_id.clone());
                history_message.context_id = Some(runtime.context_id.clone());
                task.add_to_history(history_message);
            }

            for artifact in runtime.artifacts {
                task.add_artifact(runtime_artifact_into_a2a(artifact));
            }

            for (key, value) in runtime.metadata {
                task.set_metadata(key, value);
            }

            task.update_status(crate::conversions::task_phase_to_a2a(&runtime.phase));
            if let Some(mut status_message) = status_message {
                status_message.task_id = Some(runtime.task_id.clone());
                status_message.context_id = Some(runtime.context_id.clone());
                task.update_status_with_message(task.status.state.clone(), status_message);
            }

            Ok(MessageSendResponse::Task(task))
        }
    }
}

fn latest_task_status_message(runtime: &crate::agent::response::RuntimeTask) -> Option<Message> {
    if let Some(status_text) = runtime.status_text.clone() {
        return Some(Message::new(
            MessageRole::Agent,
            vec![Part::text(status_text)],
            runtime.task_id.clone(),
        ));
    }

    runtime
        .history
        .iter()
        .rev()
        .find_map(|message| {
            (message.role == agent_core::Role::Agent)
                .then(|| crate::conversions::agent_message_to_a2a(message.clone()))
        })
}

fn trim_runtime_history(
    history: Vec<agent_core::AgentMessage>,
    history_length: Option<u32>,
) -> Vec<agent_core::AgentMessage> {
    let keep = history_length.unwrap_or(0) as usize;
    if keep == 0 {
        return Vec::new();
    }

    let len = history.len();
    let start = len.saturating_sub(keep);
    history.into_iter().skip(start).collect()
}

#[cfg(feature = "a2a-server")]
fn continuation_snapshot_from_task(task: &Task, source_revision: u64) -> ContinuationSnapshot {
    let latest_status_message = task
        .status
        .message
        .clone()
        .or_else(|| {
            task.history
                .as_ref()
                .and_then(|history| history.last().cloned())
        })
        .map(crate::conversions::agent_message_from_a2a);

    let artifact_refs = task
        .artifacts
        .clone()
        .unwrap_or_default()
        .into_iter()
        .map(|artifact| ContinuationArtifactRef {
            name: artifact.name.unwrap_or_else(|| "artifact".to_string()),
            description: artifact.description,
        })
        .collect();

    ContinuationSnapshot {
        task_id: task.id.clone(),
        context_id: task.context_id.clone(),
        source_revision,
        strategy: ContinuationStrategyDescriptor::default(),
        task_phase: crate::conversions::task_phase_from_a2a(&task.status.state),
        latest_status_message,
        artifact_refs,
        metadata_extract: task.metadata.clone().unwrap_or_default(),
        payload: Value::Null,
        created_at: task.status.timestamp.clone(),
        updated_at: task.status.timestamp.clone(),
    }
}

#[cfg(feature = "a2a-server")]
fn continuation_snapshot_from_runtime_task(
    runtime: &RuntimeTask,
    source_revision: u64,
) -> ContinuationSnapshot {
    let latest_status_message = runtime
        .status_text
        .clone()
        .map(agent_core::AgentMessage::agent_text)
        .or_else(|| {
            runtime
                .history
                .iter()
                .rev()
                .find(|message| message.role == agent_core::Role::Agent)
                .cloned()
        });

    let artifact_refs = runtime
        .artifacts
        .iter()
        .map(|artifact| ContinuationArtifactRef {
            name: artifact.name.clone(),
            description: artifact.description.clone(),
        })
        .collect();

    ContinuationSnapshot {
        task_id: runtime.task_id.clone(),
        context_id: runtime.context_id.clone(),
        source_revision,
        strategy: ContinuationStrategyDescriptor::default(),
        task_phase: runtime.phase.clone(),
        latest_status_message,
        artifact_refs,
        metadata_extract: runtime.metadata.clone(),
        payload: Value::Null,
        created_at: None,
        updated_at: None,
    }
}

/// Compatibility wrapper that projects runtime LLM execution onto the A2A response surface.
#[cfg(feature = "llm-engine")]
pub async fn execute_a2a(
    llm: std::sync::Arc<dyn LlmInvoker>,
    model: &str,
    tools: &crate::agent::ToolRegistry,
    policy: &LlmPolicy,
    msg_ctx: &crate::agent::MessageContext,
    task_ctx: Option<crate::agent::TaskContext>,
    system_message: Option<&str>,
    request_defaults: Option<&LlmRequestDefaults>,
    skill_context: Option<&crate::agent::skill::SkillContext>,
    skill_summary: Option<&str>,
) -> A2AResult<MessageSendResponse> {
    let history_runtime = default_history_policy_runtime();
    execute_runtime(
        llm,
        model,
        tools,
        policy,
        msg_ctx,
        task_ctx,
        system_message,
        request_defaults,
        skill_context,
        skill_summary,
        history_runtime.as_ref(),
    )
    .await
    .map_err(A2AError::from)
    .and_then(|response| runtime_response_into_a2a(response, Some(0)))
}

fn runtime_artifact_into_a2a(artifact: RuntimeArtifact) -> Artifact {
    let mut a2a_artifact = Artifact::data(artifact.data).with_name(artifact.name);
    if let Some(desc) = artifact.description {
        a2a_artifact = a2a_artifact.with_description(desc);
    }
    a2a_artifact
}

/// A2A adapter: convert protocol-independent `SkillDefinition` to `AgentSkill`.
pub(crate) fn skill_def_to_agent_skill(def: &crate::agent::skill::SkillDefinition) -> AgentSkill {
    AgentSkill {
        id: def.id.clone(),
        name: def.name.clone(),
        description: def.description.clone(),
        input_modes: Some(def.input_modes.clone()),
        output_modes: Some(def.output_modes.clone()),
        examples: def.examples.clone(),
        tags: def.tags.clone(),
        security_requirements: None,
    }
}

/// Adapter entrypoint for `message/send`.
#[cfg(feature = "a2a-server")]
pub async fn handle_message_send(
    agent: &Agent,
    params: MessageSendParams,
) -> A2AResult<MessageSendResponse> {
    let msg_ctx = message_context_from_params(
        &params,
        agent.config().stateless_methods,
        Some(agent.runtime_skill_executor()),
    );

    let task_ctx = if should_provide_task_context(&msg_ctx, &params) {
        let reuse_policy = if msg_ctx.skill_call.is_some() && params.message.task_id.is_none() {
            crate::agent::task_manager::TaskReusePolicy::StartNewTaskAfterTerminal
        } else {
            crate::agent::task_manager::TaskReusePolicy::ReuseTerminalTask
        };
        Some(
            agent
                .get_or_create_task_context(&params.message.context_id, reuse_policy)
                .await
                .map_err(A2AError::from)?,
        )
    } else {
        None
    };

    info!(
        "Processing A2A message/send: type={:?}, message_id={}, context_id={:?}",
        msg_ctx.message_type, params.message.message_id, params.message.context_id
    );

    let history_length = params
        .configuration
        .as_ref()
        .and_then(|cfg| cfg.history_length)
        .or(Some(0));
    let task_ctx_for_update = task_ctx.clone();

    let response = agent
        .dispatch_message(msg_ctx, task_ctx)
        .await
        .map_err(A2AError::from)?;

    persist_continuation_update(agent, task_ctx_for_update.as_ref(), &response)
        .await
        .map_err(A2AError::from)?;

    runtime_response_into_a2a(response, history_length)
}

#[cfg(feature = "a2a-server")]
async fn persist_continuation_update(
    agent: &Agent,
    task_ctx: Option<&crate::agent::TaskContext>,
    response: &RuntimeResponse,
) -> crate::SdkResult<()> {
    let RuntimeResponse::Task(runtime) = response else {
        return Ok(());
    };
    let Some(update) = &runtime.continuation_update else {
        return Ok(());
    };

    let runtime_store = agent.runtime_task_store();
    let source_revision = task_ctx
        .and_then(|ctx| ctx.continuation.as_ref().map(|continuation| continuation.source_revision))
        .or_else(|| runtime_store.get_task_revision(&runtime.task_id).ok().flatten())
        .unwrap_or(0);
    let existing = runtime_store.get_latest_snapshot(&runtime.context_id)?;
    let mut snapshot = continuation_snapshot_from_runtime_task(runtime, source_revision);
    snapshot.strategy = ContinuationStrategyDescriptor {
        kind: update.strategy_kind.clone(),
        version: update.strategy_version,
        composition: update.strategy_composition.clone(),
    };
    snapshot.payload = update.payload.clone();
    if let Some(existing) = existing.filter(|snapshot| snapshot.task_id == runtime.task_id) {
        if snapshot.created_at.is_none() {
            snapshot.created_at = existing.created_at;
        }
    }
    runtime_store.set_latest_snapshot(&runtime.context_id, Some(snapshot))
}

#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
pub use crate::streaming::{a2a_sse_stream, map_trace_to_stream_response, A2aSseContext};

/// Start a protocol-native task trace stream from an [`Agent`] and A2A request.
#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
pub fn task_trace_stream(
    agent: Arc<Agent>,
    params: MessageSendParams,
) -> crate::agent::trace::AgentTraceStream {
    Box::pin(async_stream::stream! {
        use crate::agent::trace::AgentTraceEvent;

        yield AgentTraceEvent::TurnStarted {
            turn: 1,
            response_id: None,
            model: None,
        };

        match handle_message_send(agent.as_ref(), params).await {
            Ok(response) => {
                let text = extract_text_from_message_send_response(&response);
                if let Some(delta) = text.clone() {
                    if !delta.is_empty() {
                        yield AgentTraceEvent::ContentDelta { delta };
                    }
                }
                yield AgentTraceEvent::Completed { text, usage: None };
            }
            Err(err) => {
                yield AgentTraceEvent::Failed {
                    message: err.to_string(),
                };
            }
        }
    })
}

/// Map a runtime trace stream into A2A SSE/task events.
#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
pub fn task_sse_stream(
    trace_stream: crate::agent::trace::AgentTraceStream,
    ctx: A2aSseContext,
) -> std::pin::Pin<Box<dyn Stream<Item = StreamResponse> + Send>> {
    let mapped = trace_stream.flat_map(move |event| {
        let events = crate::streaming::map_trace_to_stream_response(event, &ctx);
        stream::iter(events)
    });
    Box::pin(mapped)
}

#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
fn extract_text_from_message_send_response(response: &MessageSendResponse) -> Option<String> {
    match response {
        MessageSendResponse::Message(message) => {
            let text = message.get_text_content();
            (!text.is_empty()).then_some(text)
        }
        MessageSendResponse::Task(task) => {
            if let Some(message) = &task.status.message {
                let text = message.get_text_content();
                if !text.is_empty() {
                    return Some(text);
                }
            }
            task.history
                .as_ref()
                .and_then(|history| history.last())
                .map(|message| message.get_text_content())
                .filter(|text| !text.is_empty())
        }
    }
}

#[cfg(all(test, feature = "a2a-server"))]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, RwLock};

    use a2a_protocol_core::data::{Artifact, Message, MessageRole, Task, TaskState};
    use a2a_protocol_core::services::{InMemoryTaskStorage, TaskStorage};
    use agent_core::{AgentMessage, ContentPart, Role, TaskPhase};
    use serde_json::json;

    use super::{MirroredTaskStorage, runtime_response_into_a2a};
    use crate::agent::task_store::{
        ContinuationSnapshot, ContinuationStrategyDescriptor, RuntimeTaskStore,
    };
    use crate::agent::{RuntimeArtifact, RuntimeResponse, RuntimeTask};
    use crate::SdkResult;

    struct TestRuntimeTaskStore {
        latest_by_context: RwLock<HashMap<String, ContinuationSnapshot>>,
        revisions_by_task: RwLock<HashMap<String, u64>>,
    }

    impl TestRuntimeTaskStore {
        fn new() -> Self {
            Self {
                latest_by_context: RwLock::new(HashMap::new()),
                revisions_by_task: RwLock::new(HashMap::new()),
            }
        }
    }

    impl RuntimeTaskStore for TestRuntimeTaskStore {
        fn get_latest_snapshot(&self, context_id: &str) -> SdkResult<Option<ContinuationSnapshot>> {
            self.latest_by_context
                .read()
                .map(|contexts| contexts.get(context_id).cloned())
                .map_err(|_| crate::SdkError::method_execution("test_runtime_task_store", "lock poisoned"))
        }

        fn set_latest_snapshot(
            &self,
            context_id: &str,
            snapshot: Option<ContinuationSnapshot>,
        ) -> SdkResult<()> {
            let mut contexts = self.latest_by_context.write().map_err(|_| {
                crate::SdkError::method_execution("test_runtime_task_store", "lock poisoned")
            })?;

            match snapshot {
                Some(snapshot) => {
                    contexts.insert(context_id.to_string(), snapshot);
                }
                None => {
                    contexts.remove(context_id);
                }
            }

            Ok(())
        }

        fn get_task_revision(&self, task_id: &str) -> SdkResult<Option<u64>> {
            self.revisions_by_task
                .read()
                .map(|revisions| revisions.get(task_id).copied())
                .map_err(|_| crate::SdkError::method_execution("test_runtime_task_store", "lock poisoned"))
        }

        fn set_task_revision(&self, task_id: &str, revision: u64) -> SdkResult<()> {
            let mut revisions = self.revisions_by_task.write().map_err(|_| {
                crate::SdkError::method_execution("test_runtime_task_store", "lock poisoned")
            })?;
            revisions.insert(task_id.to_string(), revision);
            Ok(())
        }

        fn delete_task_revision(&self, task_id: &str) -> SdkResult<()> {
            let mut revisions = self.revisions_by_task.write().map_err(|_| {
                crate::SdkError::method_execution("test_runtime_task_store", "lock poisoned")
            })?;
            revisions.remove(task_id);
            Ok(())
        }
    }

    #[test]
    fn test_mirrored_task_storage_syncs_runtime_snapshots() {
        let runtime_store: Arc<dyn RuntimeTaskStore> = Arc::new(TestRuntimeTaskStore::new());
        let storage = MirroredTaskStorage::new(
            Arc::new(InMemoryTaskStorage::new()),
            Arc::clone(&runtime_store),
        );

        let context_id = "ctx-123".to_string();
        let task_id = "task-123".to_string();

        let mut task = Task::with_id(task_id.clone(), context_id.clone());
        task.add_to_history(
            Message::text(MessageRole::User, "hello", task_id.clone()).with_context(context_id.clone()),
        );
        task.add_artifact(
            Artifact::data(json!({"result":"ok"}))
                .with_name("answer")
                .with_description("Structured output"),
        );
        task.set_metadata("source".to_string(), json!("test"));
        task.update_status(TaskState::Completed);

        runtime_store
            .set_latest_snapshot(
                &context_id,
                Some(ContinuationSnapshot {
                    task_id: task_id.clone(),
                    context_id: context_id.clone(),
                    source_revision: 0,
                    strategy: ContinuationStrategyDescriptor {
                        kind: "sliding_window_with_summary".to_string(),
                        version: 1,
                        composition: None,
                    },
                    task_phase: agent_core::TaskPhase::Working,
                    latest_status_message: None,
                    artifact_refs: Vec::new(),
                    metadata_extract: HashMap::new(),
                    payload: json!({"summaries": ["previous summary"]}),
                    created_at: None,
                    updated_at: None,
                }),
            )
            .unwrap();

        storage.store_task(task.clone()).unwrap();

        let snapshot = runtime_store
            .get_latest_snapshot(&context_id)
            .unwrap()
            .expect("runtime snapshot should exist after store");
        assert_eq!(snapshot.task_id, task_id);
        assert_eq!(snapshot.context_id, context_id);
        assert_eq!(snapshot.metadata_extract.get("source"), Some(&json!("test")));
        assert_eq!(snapshot.artifact_refs.len(), 1);
        assert_eq!(snapshot.artifact_refs[0].name, "answer");
        assert_eq!(
            snapshot.artifact_refs[0].description.as_deref(),
            Some("Structured output")
        );
        assert_eq!(snapshot.source_revision, 1);
        assert_eq!(snapshot.strategy.kind, "sliding_window_with_summary");
        assert_eq!(snapshot.payload, json!({"summaries": ["previous summary"]}));

        storage.remove_task(&task.id).unwrap();

        assert!(
            runtime_store
                .get_latest_snapshot(&context_id)
                .unwrap()
                .is_none(),
            "runtime snapshot should clear when the last task is removed"
        );
    }

    #[test]
    fn test_mirrored_task_storage_preserves_existing_artifacts_for_compact_updates() {
        let runtime_store: Arc<dyn RuntimeTaskStore> = Arc::new(TestRuntimeTaskStore::new());
        let backing = Arc::new(InMemoryTaskStorage::new());
        let storage = MirroredTaskStorage::new(backing.clone(), Arc::clone(&runtime_store));

        let context_id = "ctx-artifacts".to_string();
        let task_id = "task-artifacts".to_string();

        let mut initial = Task::with_id(task_id.clone(), context_id.clone());
        initial.add_artifact(
            Artifact::data(json!({"round": 1}))
                .with_name("round_1")
                .with_description("first result"),
        );
        initial.update_status(TaskState::Completed);
        storage.store_task(initial).unwrap();

        let mut compact_update = Task::with_id(task_id.clone(), context_id.clone());
        compact_update.add_artifact(
            Artifact::data(json!({"round": 2}))
                .with_name("round_2")
                .with_description("second result"),
        );
        compact_update.update_status(TaskState::Completed);
        storage.store_task(compact_update).unwrap();

        let stored = backing
            .get_task(&task_id)
            .unwrap()
            .expect("stored task should exist");
        let artifacts = stored.artifacts.expect("stored task should preserve artifacts");
        assert_eq!(artifacts.len(), 2);
        assert_eq!(artifacts[0].name.as_deref(), Some("round_1"));
        assert_eq!(artifacts[1].name.as_deref(), Some("round_2"));
    }

    #[test]
    fn test_runtime_response_into_a2a_omits_history_when_length_zero() {
        let response = RuntimeResponse::Task(RuntimeTask {
            task_id: "task-1".to_string(),
            context_id: "ctx-1".to_string(),
            history: vec![
                AgentMessage::new(Role::User, vec![ContentPart::Text("u1".to_string())]),
                AgentMessage::new(Role::Agent, vec![ContentPart::Text("a1".to_string())]),
            ],
            artifacts: vec![RuntimeArtifact::data("result", json!({"ok": true}))],
            metadata: HashMap::new(),
            phase: TaskPhase::Completed,
            status_text: Some("done".to_string()),
            continuation_update: None,
        });

        let converted = runtime_response_into_a2a(response, Some(0)).expect("conversion should succeed");
        let task = match converted {
            crate::a2a::MessageSendResponse::Task(task) => task,
            other => panic!("expected task response, got {:?}", other),
        };

        assert!(task.history.is_none());
        assert_eq!(task.artifacts.as_ref().map(Vec::len), Some(1));
        assert_eq!(
            task.status.message.as_ref().map(|message| message.get_text_content()),
            Some("done".to_string())
        );
    }

    #[test]
    fn test_runtime_response_into_a2a_keeps_only_requested_history_tail() {
        let response = RuntimeResponse::Task(RuntimeTask {
            task_id: "task-2".to_string(),
            context_id: "ctx-2".to_string(),
            history: vec![
                AgentMessage::new(Role::User, vec![ContentPart::Text("u1".to_string())]),
                AgentMessage::new(Role::Agent, vec![ContentPart::Text("a1".to_string())]),
                AgentMessage::new(Role::User, vec![ContentPart::Text("u2".to_string())]),
                AgentMessage::new(Role::Agent, vec![ContentPart::Text("a2".to_string())]),
            ],
            artifacts: Vec::new(),
            metadata: HashMap::new(),
            phase: TaskPhase::Working,
            status_text: None,
            continuation_update: None,
        });

        let converted = runtime_response_into_a2a(response, Some(2)).expect("conversion should succeed");
        let task = match converted {
            crate::a2a::MessageSendResponse::Task(task) => task,
            other => panic!("expected task response, got {:?}", other),
        };

        let history = task.history.expect("history tail should be present");
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].get_text_content(), "u2");
        assert_eq!(history[1].get_text_content(), "a2");
    }
}
