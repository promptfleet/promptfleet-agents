//! Core Agent Implementation Module (Refactored)
//!
//! This module provides the main Agent struct and core functionality
//! following clean architecture principles and hexagonal architecture patterns.
//!
//! **Key Architectural Changes:**
//! - Extracted HTTP serving to http_integration module
//! - Extracted LLM processing to llm_integration module
//! - Extracted response building to response_builders module
//! - Extracted message handler management to message_handlers module
//! - Extracted task management to task_manager module
//! - Core now focuses on Agent lifecycle and coordination

use log::{debug, error, info};
use serde_json::Value;
use std::sync::Arc;

use a2a_protocol_core::services::TaskStorage;

use super::{
    config::AgentConfig,
    history_policy::{build_history_policy_runtime, HistoryPolicyRuntime},
    message::{MessageContext, SkillExecutor, TaskContext},
    message_handlers::MessageHandlerManager,
    response::RuntimeResponse,
    skill::{SkillEntryBuilder, SkillRegistry},
    task_manager::TaskManager,
};

use crate::{
    error::{SdkError, SdkResult},
    services::ServiceContainer,
};

#[cfg(all(feature = "llm-engine", not(target_arch = "wasm32")))]
#[derive(Clone)]
struct StreamRuntimeConfig {
    llm: Arc<dyn super::llm_orchestrator::LlmStreamInvoker>,
    model: String,
    tools: super::tools::ToolRegistry,
    policy: super::llm_orchestrator::LlmPolicy,
    system_message: Option<String>,
    request_defaults: Option<super::llm_orchestrator::LlmRequestDefaults>,
}

/// Core Agent implementation
///
/// The `Agent` struct is the main runtime entry point for registering skills,
/// wiring handlers, and dispatching runtime messages. Protocol surfaces such as
/// A2A and AG-UI compose around this type via adapter modules.
pub struct Agent {
    /// Agent configuration
    config: AgentConfig,

    /// Service container for dependency injection
    services: ServiceContainer,

    /// Skill registry for managing skills and notifications
    skill_registry: SkillRegistry,

    /// Message handler manager for coordinating different handler types
    message_handler_manager: MessageHandlerManager,

    /// Task manager for conversation continuity
    task_manager: TaskManager,

    history_policy_runtime: Arc<dyn HistoryPolicyRuntime>,

    #[cfg(all(feature = "llm-engine", not(target_arch = "wasm32")))]
    stream_runtime: Option<StreamRuntimeConfig>,
}

impl Agent {
    /// Create a new runtime-first agent.
    pub fn new_runtime(name: &str) -> SdkResult<Self> {
        debug!("Creating new runtime agent with name: {}", name);

        let mut config = AgentConfig::default();
        config.name = name.to_string();

        Self::new_with_config(config)
    }

    /// Create a new agent (alias of `new_runtime`)
    pub fn new(name: &str) -> SdkResult<Self> {
        Self::new_runtime(name)
    }

    /// Create agent with custom configuration
    pub fn new_with_config(config: AgentConfig) -> SdkResult<Self> {
        debug!(
            "Creating agent with config: name={}, max_message_size={}, stateless_methods={}",
            config.name, config.max_message_size, config.stateless_methods
        );

        if config.name.is_empty() {
            error!("Agent creation failed: name cannot be empty");
            return Err(SdkError::invalid_input("Agent name cannot be empty"));
        }

        if config.max_message_size == 0 {
            error!("Agent creation failed: max_message_size must be greater than zero");
            return Err(SdkError::invalid_input(
                "Max message size must be greater than zero",
            ));
        }

        let storage_prefix = config
            .storage_prefix
            .clone()
            .unwrap_or_else(|| format!("local:dev:{}", config.name));
        debug!("Agent storage prefix: {}", storage_prefix);
        let runtime_task_store =
            super::task_store::build_default_runtime_task_store(storage_prefix)?;
        #[cfg(feature = "context-window")]
        let history_policy_runtime = build_history_policy_runtime(
            config.name.clone(),
            config.history_policy.as_ref(),
            None,
            None,
        );
        #[cfg(not(feature = "context-window"))]
        let history_policy_runtime =
            build_history_policy_runtime(config.name.clone(), config.history_policy.as_ref());

        let services = ServiceContainer::new();
        let skill_registry = SkillRegistry::new();
        let task_manager = TaskManager::new(runtime_task_store);

        let mut message_handler_manager =
            MessageHandlerManager::new(Arc::new(skill_registry.clone()));

        message_handler_manager.initialize_default_handler();

        let agent = Self {
            config,
            services,
            skill_registry,
            message_handler_manager,
            task_manager,
            history_policy_runtime,
            #[cfg(all(feature = "llm-engine", not(target_arch = "wasm32")))]
            stream_runtime: None,
        };

        info!("Agent '{}' successfully created", agent.config.name,);

        Ok(agent)
    }

    // -- Service injection --

    pub fn with_service<T: Send + Sync + 'static>(mut self, service: T) -> Self {
        self.services.register(service);
        self
    }

    pub fn get_service<T: Send + Sync + 'static>(&self) -> Option<std::sync::Arc<T>> {
        self.services.get::<T>()
    }

    pub fn has_service<T: 'static>(&self) -> bool {
        self.services.has::<T>()
    }

    // -- Skill registration (unified API) --

    /// Register a skill via the fluent builder (handler optional — omit for metadata-only).
    pub fn add_skill(&mut self, skill_id: &str) -> SkillEntryBuilder<'_> {
        self.skill_registry.add_skill(skill_id)
    }

    /// Register a skill with a handler (`add_skill(name).handler(handler)`).
    pub fn skill<F, Fut>(&mut self, name: &str, handler: F) -> SkillEntryBuilder<'_>
    where
        F: Fn(Value) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<Value, String>> + Send + 'static,
    {
        self.skill_registry.skill(name, handler)
    }

    /// Register a notification handler.
    pub fn register_notification<F, Fut>(&mut self, name: &str, handler: F) -> SdkResult<()>
    where
        F: Fn(Value) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<(), String>> + Send + 'static,
    {
        self.skill_registry.register_notification(name, handler)
    }

    // -- Accessors --

    pub fn config(&self) -> &AgentConfig {
        &self.config
    }

    pub fn list_skills(&self) -> Vec<String> {
        self.skill_registry.list_skills()
    }

    pub fn list_notifications(&self) -> Vec<String> {
        self.skill_registry.list_notifications()
    }

    /// Get read-only reference to the skill registry.
    pub fn skill_registry(&self) -> &SkillRegistry {
        &self.skill_registry
    }

    // -- Message handler configuration --

    #[cfg(not(target_arch = "wasm32"))]
    pub fn set_message_handler<F, Fut>(&mut self, handler: F)
    where
        F: Fn(MessageContext, Option<TaskContext>) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = SdkResult<RuntimeResponse>> + Send + 'static,
    {
        self.message_handler_manager.set_custom_handler(handler);
    }

    #[cfg(target_arch = "wasm32")]
    pub fn set_message_handler<F, Fut>(&mut self, handler: F)
    where
        F: Fn(MessageContext, Option<TaskContext>) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = SdkResult<RuntimeResponse>> + 'static,
    {
        self.message_handler_manager.set_custom_handler(handler);
    }

    /// Configure the built-in LLM tool loop, streaming runtime (native), and request defaults.
    #[cfg(all(feature = "llm-engine", not(target_arch = "wasm32")))]
    pub fn configure_llm_runtime<I, T>(
        &mut self,
        client: I,
        model: &str,
        tools: T,
        system_message: Option<String>,
        policy: Option<super::llm_orchestrator::LlmPolicy>,
        request_defaults: Option<super::llm_orchestrator::LlmRequestDefaults>,
    ) -> SdkResult<()>
    where
        I: super::llm_orchestrator::IntoLlmInvoker
            + super::llm_orchestrator::IntoLlmStreamInvoker
            + Clone,
        T: super::tools::IntoTools,
    {
        let inv = client.clone().into_invoker();
        let reg = tools.into_tools();
        self.stream_runtime = Some(StreamRuntimeConfig {
            llm: client.into_stream_invoker(),
            model: model.to_string(),
            tools: reg.clone(),
            policy: policy.clone().unwrap_or_default(),
            system_message: system_message.clone(),
            request_defaults: request_defaults.clone(),
        });
        self.message_handler_manager
            .set_llm_tools_handler_configured(
                inv,
                model,
                reg,
                policy,
                system_message,
                request_defaults,
                self.history_policy_runtime.clone(),
            )
    }

    #[cfg(all(feature = "llm-engine", target_arch = "wasm32"))]
    pub fn configure_llm_runtime<I, T>(
        &mut self,
        client: I,
        model: &str,
        tools: T,
        system_message: Option<String>,
        policy: Option<super::llm_orchestrator::LlmPolicy>,
        request_defaults: Option<super::llm_orchestrator::LlmRequestDefaults>,
    ) -> SdkResult<()>
    where
        I: super::llm_orchestrator::IntoLlmInvoker + Clone,
        T: super::tools::IntoTools,
    {
        let inv = client.into_invoker();
        let reg = tools.into_tools();
        self.message_handler_manager
            .set_llm_tools_handler_configured(
                inv,
                model,
                reg,
                policy,
                system_message,
                request_defaults,
                self.history_policy_runtime.clone(),
            )
    }

    // -- Message dispatch --

    /// Unified entrypoint to route a built MessageContext through the active handler.
    pub async fn dispatch_message(
        &self,
        msg_ctx: MessageContext,
        task_ctx: Option<TaskContext>,
    ) -> SdkResult<RuntimeResponse> {
        self.message_handler_manager
            .handle_message(msg_ctx, task_ctx)
            .await
    }

    #[cfg(all(feature = "llm-engine", not(target_arch = "wasm32")))]
    pub fn run_stream(
        &self,
        input: agent_core::AgentMessage,
        history: Option<agent_core::ConversationContext>,
        cancel_flag: Option<Arc<std::sync::atomic::AtomicBool>>,
        request_headers: Option<Arc<std::collections::HashMap<String, String>>>,
    ) -> SdkResult<super::trace::AgentTraceStream> {
        let runtime = self.stream_runtime.as_ref().ok_or_else(|| {
            SdkError::feature_not_enabled(
                "llm streaming runtime not configured; call configure_llm_runtime with a stream-capable client",
            )
        })?;

        Ok(
            super::llm_orchestrator::run_tools_loop_agnostic_with_cancel_and_history_runtime(
                runtime.llm.clone(),
                runtime.model.clone(),
                runtime.tools.clone(),
                runtime.policy.clone(),
                input,
                history,
                runtime.system_message.clone(),
                runtime.request_defaults.clone(),
                cancel_flag,
                request_headers,
                self.history_policy_runtime.clone(),
            ),
        )
    }

    #[cfg(all(feature = "llm-engine", not(target_arch = "wasm32")))]
    pub fn can_run_stream(&self) -> bool {
        self.stream_runtime.is_some()
    }
    pub(crate) fn runtime_skill_executor(&self) -> SkillExecutor {
        SkillExecutor::new(Arc::new(self.skill_registry.clone()))
    }

    pub(crate) async fn get_or_create_task_context(
        &self,
        context_id: &Option<String>,
        reuse_policy: super::task_manager::TaskReusePolicy,
    ) -> SdkResult<TaskContext> {
        self.task_manager
            .get_or_create_task_context(context_id, reuse_policy)
            .await
    }

    pub(crate) fn runtime_task_store(&self) -> Arc<dyn super::task_store::RuntimeTaskStore> {
        self.task_manager.runtime_store()
    }

    pub(crate) fn attach_protocol_task_storage(
        &self,
        storage: Arc<dyn TaskStorage>,
    ) -> SdkResult<()> {
        self.task_manager.attach_canonical_task_storage(storage)
    }

    #[cfg(feature = "context-window")]
    pub fn configure_history_policy_runtime(
        &mut self,
        summarizer: Option<Arc<dyn llm_context_core::history::Summarizer>>,
        memory: Option<Arc<dyn llm_context_core::LongTermMemory>>,
    ) {
        self.history_policy_runtime = build_history_policy_runtime(
            self.config.name.clone(),
            self.config.history_policy.as_ref(),
            summarizer,
            memory,
        );
    }
}
