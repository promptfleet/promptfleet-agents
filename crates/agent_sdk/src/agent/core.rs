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
    history_policy::{HistoryPolicyRuntime, build_history_policy_runtime},
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

#[cfg(feature = "llm-engine")]
#[derive(Clone)]
struct RequestRuntimeConfig {
    llm: Arc<dyn super::llm_orchestrator::LlmInvoker>,
    model: String,
    tools: super::tools::ToolRegistry,
    policy: super::llm_orchestrator::LlmPolicy,
    system_message: Option<String>,
    request_defaults: Option<super::llm_orchestrator::LlmRequestDefaults>,
}

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

    #[cfg(feature = "llm-engine")]
    request_runtime: Option<RequestRuntimeConfig>,

    #[cfg(all(feature = "llm-engine", not(target_arch = "wasm32")))]
    stream_runtime: Option<StreamRuntimeConfig>,

    #[cfg(feature = "llm-engine")]
    structured_output_contract: Option<crate::structured::StructuredOutputContract>,
}

#[cfg(feature = "llm-engine")]
pub struct LlmRuntimeConfigurator<'a> {
    agent: &'a mut Agent,
}

#[cfg(feature = "llm-engine")]
impl<'a> LlmRuntimeConfigurator<'a> {
    #[cfg(feature = "structured-io")]
    pub fn with_structured_output_contract(
        self,
        contract: crate::structured::StructuredOutputContract,
    ) -> SdkResult<Self> {
        self.agent.configure_structured_output(contract)?;
        Ok(self)
    }

    #[cfg(feature = "structured-io")]
    pub fn with_structured_output<T>(
        self,
        schema_name: impl Into<String>,
        artifact_name: impl Into<String>,
    ) -> SdkResult<Self>
    where
        T: schemars::JsonSchema,
    {
        self.with_structured_output_contract(
            crate::structured::StructuredOutputContract::from_type::<T>(schema_name, artifact_name),
        )
    }

    pub fn into_agent(self) -> &'a mut Agent {
        self.agent
    }
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
            #[cfg(feature = "llm-engine")]
            request_runtime: None,
            #[cfg(all(feature = "llm-engine", not(target_arch = "wasm32")))]
            stream_runtime: None,
            #[cfg(feature = "llm-engine")]
            structured_output_contract: None,
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
    ) -> SdkResult<LlmRuntimeConfigurator<'_>>
    where
        I: super::llm_orchestrator::IntoLlmInvoker
            + super::llm_orchestrator::IntoLlmStreamInvoker
            + Clone,
        T: super::tools::IntoTools,
    {
        let inv = client.clone().into_invoker();
        let reg = tools.into_tools();
        let resolved_policy = policy.clone().unwrap_or_default();
        self.request_runtime = Some(RequestRuntimeConfig {
            llm: inv.clone(),
            model: model.to_string(),
            tools: reg.clone(),
            policy: resolved_policy.clone(),
            system_message: system_message.clone(),
            request_defaults: request_defaults.clone(),
        });
        self.stream_runtime = Some(StreamRuntimeConfig {
            llm: client.into_stream_invoker(),
            model: model.to_string(),
            tools: reg.clone(),
            policy: resolved_policy.clone(),
            system_message: system_message.clone(),
            request_defaults: request_defaults.clone(),
        });
        self.message_handler_manager
            .set_llm_tools_handler_configured(
                inv,
                model,
                reg,
                Some(resolved_policy),
                system_message,
                request_defaults,
                self.history_policy_runtime.clone(),
            )?;
        Ok(LlmRuntimeConfigurator { agent: self })
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
    ) -> SdkResult<LlmRuntimeConfigurator<'_>>
    where
        I: super::llm_orchestrator::IntoLlmInvoker + Clone,
        T: super::tools::IntoTools,
    {
        let inv = client.into_invoker();
        let reg = tools.into_tools();
        let resolved_policy = policy.clone().unwrap_or_default();
        self.request_runtime = Some(RequestRuntimeConfig {
            llm: inv.clone(),
            model: model.to_string(),
            tools: reg.clone(),
            policy: resolved_policy.clone(),
            system_message: system_message.clone(),
            request_defaults: request_defaults.clone(),
        });
        self.message_handler_manager
            .set_llm_tools_handler_configured(
                inv,
                model,
                reg,
                Some(resolved_policy),
                system_message,
                request_defaults,
                self.history_policy_runtime.clone(),
            )?;
        Ok(LlmRuntimeConfigurator { agent: self })
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

    #[cfg(feature = "structured-io")]
    pub fn configure_structured_output(
        &mut self,
        contract: crate::structured::StructuredOutputContract,
    ) -> SdkResult<()> {
        self.structured_output_contract = Some(contract);
        Ok(())
    }

    #[cfg(feature = "structured-io")]
    pub async fn run_structured<I, O>(
        &self,
        input: crate::structured::StructuredInput<I>,
    ) -> SdkResult<crate::structured::StructuredRunResult<O>>
    where
        I: serde::Serialize,
        O: serde::de::DeserializeOwned + schemars::JsonSchema,
    {
        let contract = self
            .structured_output_contract
            .clone()
            .unwrap_or_else(crate::structured::StructuredOutputContract::for_type::<O>);
        self.run_structured_with_contract(input, contract).await
    }

    #[cfg(feature = "structured-io")]
    pub async fn run_structured_with_contract<I, O>(
        &self,
        input: crate::structured::StructuredInput<I>,
        contract: crate::structured::StructuredOutputContract,
    ) -> SdkResult<crate::structured::StructuredRunResult<O>>
    where
        I: serde::Serialize,
        O: serde::de::DeserializeOwned,
    {
        let runtime = self.request_runtime.as_ref().ok_or_else(|| {
            SdkError::method_execution(
                "run_structured",
                "LLM runtime is not configured; call configure_llm_runtime first",
            )
        })?;

        let mut checkpoint_env = crate::runtime_vars::CheckpointEnv::load_optional();
        if checkpoint_env.mode == crate::runtime_vars::CheckpointMode::StateOnly {
            checkpoint_env.mode = crate::runtime_vars::CheckpointMode::TaskObservable;
        }
        checkpoint_env.allow_message_response = false;

        let mut tools = runtime.tools.clone();
        tools.register(super::checkpoint::checkpoint_tool_spec(
            &checkpoint_env,
            Some(&contract),
        ));

        let mut policy = runtime.policy.clone();
        policy.checkpoint_mode = checkpoint_env.mode;
        policy.checkpoint_allow_message_response = false;
        policy.checkpoint_mirror_internal_state_to_task_meta =
            checkpoint_env.mirror_internal_state_to_task_meta;

        let msg_ctx = input.into_message_context(Some(self.runtime_skill_executor()), true)?;
        let response = super::llm_orchestrator::execute_runtime(
            runtime.llm.clone(),
            &runtime.model,
            &tools,
            &policy,
            &msg_ctx,
            None,
            runtime.system_message.as_deref(),
            runtime.request_defaults.as_ref(),
            None,
            None,
            self.history_policy_runtime.as_ref(),
            Some(&contract),
        )
        .await?;
        let final_text = response.text_content();
        let output = match crate::structured::decode_optional_artifact(
            &response,
            &contract.artifact_name,
        )? {
            Some(output) => output,
            None if !contract.required => {
                serde_json::from_value(serde_json::Value::Null).map_err(|error| {
                    SdkError::method_execution(
                        "run_structured",
                        format!(
                            "structured output artifact '{}' was not produced; use Option<T>, serde_json::Value, or make the contract required: {}",
                            contract.artifact_name, error
                        ),
                    )
                })?
            }
            None => crate::structured::decode_artifact(&response, &contract.artifact_name)?,
        };

        Ok(crate::structured::StructuredRunResult {
            output,
            artifact_name: contract.artifact_name,
            dataschema: contract.dataschema,
            raw_response: response,
            final_text,
        })
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

#[cfg(all(test, feature = "structured-io"))]
mod structured_tests {
    use super::*;
    use crate::structured::{StructuredInput, StructuredOutputContract, StructuredRunResult};
    use llm_client::LlmResponse;
    use schemars::JsonSchema;
    use serde::{Deserialize, Serialize};
    use serde_json::json;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    struct MockRequestInvoker {
        responses: Mutex<VecDeque<Result<LlmResponse, String>>>,
    }

    impl MockRequestInvoker {
        fn new(responses: Vec<Result<LlmResponse, String>>) -> Self {
            Self {
                responses: Mutex::new(responses.into()),
            }
        }
    }

    impl super::super::llm_invoker::LlmInvoker for MockRequestInvoker {
        fn request(
            &self,
            _req: llm_client::LlmRequest,
        ) -> std::pin::Pin<
            Box<dyn core::future::Future<Output = Result<llm_client::LlmResponse, String>> + Send>,
        > {
            let next = self
                .responses
                .lock()
                .expect("mock invoker lock poisoned")
                .pop_front()
                .unwrap_or_else(|| Err("no mock response queued".to_string()));
            Box::pin(async move { next })
        }
    }

    #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
    struct AlertSignal {
        alert_id: String,
        severity: String,
    }

    #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
    struct AnalysisOutput {
        alert_id: String,
        disposition: String,
        confidence: f32,
    }

    fn llm_response(value: serde_json::Value) -> LlmResponse {
        serde_json::from_value(value).expect("llm response fixture")
    }

    fn checkpoint_tool_call(args: serde_json::Value) -> LlmResponse {
        llm_response(json!({
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
    }

    fn configured_agent(responses: Vec<Result<LlmResponse, String>>) -> Agent {
        let mut agent = Agent::new_runtime("structured-agent").expect("agent");
        agent.request_runtime = Some(RequestRuntimeConfig {
            llm: Arc::new(MockRequestInvoker::new(responses)),
            model: "gpt-test".to_string(),
            tools: super::super::tools::ToolRegistry::new(),
            policy: super::super::llm_invoker::LlmPolicy::default(),
            system_message: Some("Return a typed structured output".to_string()),
            request_defaults: None,
        });
        agent
    }

    #[tokio::test]
    async fn run_structured_returns_typed_output_and_configured_artifact_name() {
        let mut agent = configured_agent(vec![Ok(checkpoint_tool_call(json!({
            "task_patch": { "state": "completed" },
            "structured_output": {
                "payload": {
                    "alert_id": "a-1",
                    "disposition": "escalate",
                    "confidence": 0.98
                },
                "text": "Analysis complete"
            },
            "respond": { "kind": "task" }
        })))]);
        agent
            .configure_structured_output(StructuredOutputContract::from_type::<AnalysisOutput>(
                "analysis_output",
                "analysis_output",
            ))
            .expect("configure contract");

        let result: StructuredRunResult<AnalysisOutput> = agent
            .run_structured(StructuredInput::from_payload(AlertSignal {
                alert_id: "a-1".to_string(),
                severity: "critical".to_string(),
            }))
            .await
            .expect("structured run");

        assert_eq!(
            result.output,
            AnalysisOutput {
                alert_id: "a-1".to_string(),
                disposition: "escalate".to_string(),
                confidence: 0.98,
            }
        );
        assert_eq!(result.artifact_name, "analysis_output");
        assert_eq!(
            result.dataschema.as_deref(),
            Some("urn:promptfleet:schema:analysis_output")
        );
        assert_eq!(result.final_text.as_deref(), Some("Analysis complete"));
    }

    #[tokio::test]
    async fn run_structured_rejects_payloads_that_fail_schema_validation() {
        let agent = configured_agent(vec![Ok(checkpoint_tool_call(json!({
            "task_patch": { "state": "completed" },
            "structured_output": {
                "payload": {
                    "alert_id": "a-2",
                    "disposition": "ignore"
                }
            },
            "respond": { "kind": "task" }
        })))]);

        let error = agent
            .run_structured::<_, AnalysisOutput>(StructuredInput::from_payload(AlertSignal {
                alert_id: "a-2".to_string(),
                severity: "low".to_string(),
            }))
            .await
            .expect_err("schema validation should fail");

        assert!(
            error
                .to_string()
                .contains("Structured output validation failed"),
            "unexpected error: {}",
            error
        );
    }

    #[tokio::test]
    async fn run_structured_allows_missing_optional_payload_when_contract_is_not_required() {
        let agent = configured_agent(vec![Ok(checkpoint_tool_call(json!({
            "task_patch": { "state": "working", "status_text": "Need more evidence" },
            "respond": { "kind": "task" }
        })))]);

        let result: StructuredRunResult<Option<AnalysisOutput>> = agent
            .run_structured_with_contract(
                StructuredInput::from_payload(AlertSignal {
                    alert_id: "a-3".to_string(),
                    severity: "medium".to_string(),
                }),
                StructuredOutputContract::from_type::<AnalysisOutput>(
                    "analysis_output",
                    "analysis_output",
                )
                .with_required(false),
            )
            .await
            .expect("optional structured run");

        assert_eq!(result.output, None);
        assert_eq!(result.artifact_name, "analysis_output");
        assert_eq!(
            result.raw_response.text_content(),
            Some("Need more evidence".to_string())
        );
    }

    #[tokio::test]
    async fn run_structured_prefers_validated_artifact_when_names_collide() {
        let agent = configured_agent(vec![Ok(checkpoint_tool_call(json!({
            "task_patch": {
                "state": "completed",
                "artifacts": [{
                    "name": "analysis_output",
                    "json": { "alert_id": "bad", "disposition": 42 }
                }]
            },
            "structured_output": {
                "payload": {
                    "alert_id": "a-4",
                    "disposition": "escalate",
                    "confidence": 0.87
                }
            },
            "respond": { "kind": "task" }
        })))]);

        let result: StructuredRunResult<AnalysisOutput> = agent
            .run_structured_with_contract(
                StructuredInput::from_payload(AlertSignal {
                    alert_id: "a-4".to_string(),
                    severity: "high".to_string(),
                }),
                StructuredOutputContract::from_type::<AnalysisOutput>(
                    "analysis_output",
                    "analysis_output",
                ),
            )
            .await
            .expect("structured run should prefer validated artifact");

        assert_eq!(
            result.output,
            AnalysisOutput {
                alert_id: "a-4".to_string(),
                disposition: "escalate".to_string(),
                confidence: 0.87,
            }
        );
    }
}
