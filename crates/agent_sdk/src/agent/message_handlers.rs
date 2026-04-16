//! Message Handler Management Module
//!
//! Manages message handlers (Custom, LLM, Default) following clean architecture.
//! Coordinates message processing through various handler types.

use log::{debug, info, warn};
use std::sync::Arc;

use super::{
    message::{MessageContext, TaskContext},
    response::RuntimeResponse,
    response_builders::ResponseBuilder,
    skill::SkillRegistry,
};
use crate::error::{SdkError, SdkResult};

#[cfg(feature = "llm-engine")]
use crate::agent::{
    history_policy::HistoryPolicyRuntime,
    llm_orchestrator::{LlmInvoker, LlmPolicy, LlmRequestDefaults, execute_runtime},
    tools::{ToolExecutor, ToolRegistry, ToolSpec},
};
#[cfg(feature = "llm-engine")]
use crate::runtime_vars::{CheckpointEnv, CheckpointMode};

// Unified handler future type (non-Send on wasm32)
#[cfg(target_arch = "wasm32")]
type HandlerFuture =
    std::pin::Pin<Box<dyn std::future::Future<Output = SdkResult<RuntimeResponse>>>>;
#[cfg(not(target_arch = "wasm32"))]
type HandlerFuture =
    std::pin::Pin<Box<dyn std::future::Future<Output = SdkResult<RuntimeResponse>> + Send>>;

/// Unified message handler function type.
pub type MessageHandlerFn =
    Arc<dyn Fn(MessageContext, Option<TaskContext>) -> HandlerFuture + Send + Sync>;

/// Manages message handlers and coordinates message processing.
pub struct MessageHandlerManager {
    active_handler: Option<MessageHandlerFn>,

    /// Reference to skill registry for default handling and LLM skill wiring.
    skill_registry: Arc<SkillRegistry>,
}

impl MessageHandlerManager {
    pub fn new(skill_registry: Arc<SkillRegistry>) -> Self {
        Self {
            active_handler: None,
            skill_registry,
        }
    }

    /// Initialize default handler (skill dispatch + conversational fallback).
    pub fn initialize_default_handler(&mut self) {
        let skill_registry = Arc::clone(&self.skill_registry);
        let default_handler: MessageHandlerFn = Arc::new(
            move |msg_ctx: MessageContext, task_ctx: Option<TaskContext>| {
                let skill_registry = Arc::clone(&skill_registry);
                Box::pin(async move {
                    if let Some(skill_call) = &msg_ctx.skill_call {
                        match skill_registry
                            .execute_skill(&skill_call.skill_id, &skill_call.parameters)
                            .await
                        {
                            Ok(result) => ResponseBuilder::create_skill_success_response(
                                skill_call, result, &msg_ctx, task_ctx,
                            ),
                            Err(error) => {
                                if error.contains("not found") {
                                    ResponseBuilder::create_skill_not_implemented_response(
                                        skill_call, &msg_ctx, task_ctx,
                                    )
                                } else {
                                    Err(SdkError::method_execution(
                                        "skill_execution",
                                        format!(
                                            "Skill '{}' execution failed: {}",
                                            skill_call.skill_id, error
                                        ),
                                    ))
                                }
                            }
                        }
                    } else {
                        ResponseBuilder::create_conversational_response(&msg_ctx, task_ctx)
                    }
                })
            },
        );

        self.active_handler = Some(default_handler);
        debug!("Default message handler initialized");
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn set_custom_handler<F, Fut>(&mut self, handler: F)
    where
        F: Fn(MessageContext, Option<TaskContext>) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = SdkResult<RuntimeResponse>> + Send + 'static,
    {
        self.active_handler = Some(Arc::new(move |msg_ctx, task_ctx| {
            Box::pin(handler(msg_ctx, task_ctx))
        }));
        info!("Activated Custom message handler");
    }

    #[cfg(target_arch = "wasm32")]
    pub fn set_custom_handler<F, Fut>(&mut self, handler: F)
    where
        F: Fn(MessageContext, Option<TaskContext>) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = SdkResult<RuntimeResponse>> + 'static,
    {
        self.active_handler = Some(Arc::new(move |msg_ctx, task_ctx| {
            Box::pin(handler(msg_ctx, task_ctx))
        }));
        info!("Activated Custom message handler");
    }

    /// Tools-first LLM handler with model-aware request defaults.
    ///
    /// Wires SkillRegistry into the handler closure for:
    /// 1. `resolve_skill_context()` when a `skill_call` is present
    /// 2. `build_wired_read_skill_tool()` for llm_callable skills
    /// 3. Skill list injection into the system prompt for LLM awareness
    #[cfg(feature = "llm-engine")]
    pub(crate) fn set_llm_tools_handler_configured(
        &mut self,
        llm: Arc<dyn LlmInvoker>,
        model: &str,
        mut tools: ToolRegistry,
        policy: Option<LlmPolicy>,
        system_message: Option<String>,
        request_defaults: Option<LlmRequestDefaults>,
        history_policy_runtime: Arc<dyn HistoryPolicyRuntime>,
    ) -> SdkResult<()> {
        let checkpoint_env = CheckpointEnv::load_optional();

        // -- checkpoint_task tool (internal sentinel) --
        let mut properties = serde_json::Map::new();
        properties.insert("emit".to_string(), serde_json::json!({
            "type": "array",
            "description": "Internal-only events for logging/diagnostics (not a user-visible message).",
            "items": {
                "type": "object",
                "properties": {
                    "level": { "type": "string", "enum": ["debug","info","warn","error"], "default": "info" },
                    "code": { "type": "string", "minLength": 1, "description": "Short stable identifier, e.g. delegate.start" },
                    "message": { "type": "string", "minLength": 1 },
                    "data": { "type": "object", "description": "Optional structured payload.", "additionalProperties": true }
                },
                "required": ["code","message"],
                "additionalProperties": false
            }
        }));
        properties.insert("internal_state".to_string(), serde_json::json!({
            "type": "object",
            "description": "Internal-only snapshot; may optionally be mirrored into task metadata for polling.",
            "properties": {
                "stage": { "type": "string", "minLength": 1, "description": "e.g. discovering|delegating|waiting_external|synthesizing|waiting_input|done|failed" },
                "progress": { "type": "integer", "minimum": 0, "maximum": 100 },
                "note": { "type": "string" }
            },
            "additionalProperties": false
        }));

        if checkpoint_env.mode != CheckpointMode::StateOnly {
            properties.insert("task_patch".to_string(), serde_json::json!({
                "type": "object",
                "description": "Patch to persist into durable task state so pollers can observe progress.",
                "properties": {
                    "state": { "type": "string", "enum": ["working","input_required","completed","failed","canceled","rejected"] },
                    "status_text": { "type": "string", "description": "Human-readable status persisted with the task state." },
                    "meta": { "type": "object", "description": "Task metadata for pollers (stage/progress/errors/etc).", "additionalProperties": true },
                    "append_history_text": { "type": "string", "description": "Optional assistant message to append to task history." },
                    "artifacts": {
                        "type": "array",
                        "description": "Optional durable outputs to attach as task artifacts.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "name": { "type": "string", "minLength": 1 },
                                "description": { "type": "string" },
                                "json": { "description": "JSON payload to store as a data artifact." }
                            },
                            "required": ["name","json"],
                            "additionalProperties": false
                        }
                    }
                },
                "required": ["state"],
                "additionalProperties": false
            }));
        }

        if checkpoint_env.mode == CheckpointMode::ResponseControl {
            let mut respond_enum = vec!["task"];
            if checkpoint_env.allow_message_response {
                respond_enum.push("message");
            }
            properties.insert("respond".to_string(), serde_json::json!({
                "type": "object",
                "description": "If provided, finalize immediately with a single synchronous response.",
                "properties": {
                    "kind": { "type": "string", "enum": respond_enum },
                    "message_text": { "type": "string", "description": "Required if kind=message." },
                    "final_text": { "type": "string", "description": "Optional final assistant text (prefer task_patch.append_history_text)." }
                },
                "required": ["kind"],
                "additionalProperties": false
            }));
        }

        let checkpoint_schema = serde_json::Value::Object(serde_json::Map::from_iter([
            ("type".to_string(), serde_json::json!("object")),
            (
                "properties".to_string(),
                serde_json::Value::Object(properties),
            ),
            ("required".to_string(), serde_json::json!([])),
            ("additionalProperties".to_string(), serde_json::json!(false)),
            (
                "examples".to_string(),
                serde_json::json!([
                    {
                        "task_patch": {
                            "state": "working",
                            "status_text": "Delegating CSV parsing to tabular-parser-agent",
                            "meta": { "stage": "delegating", "progress": 30 }
                        }
                    },
                    {
                        "task_patch": {
                            "state": "input_required",
                            "status_text": "I need the CSV delimiter (comma/semicolon) and whether there is a header row.",
                            "meta": { "stage": "waiting_input", "required": ["delimiter","has_header"] }
                        },
                        "respond": { "kind": "task" }
                    },
                    {
                        "task_patch": {
                            "state": "completed",
                            "append_history_text": "Parsed 3 rows successfully.",
                            "artifacts": [{ "name": "records", "json": { "records": [{"price":10,"qty":2}] } }]
                        },
                        "respond": { "kind": "task" }
                    }
                ]),
            ),
        ]));

        let checkpoint_task = ToolSpec {
            name: "checkpoint_task".to_string(),
            description: Some(
                "Checkpoint progress and/or finalize the current message/send response.\n\
                 Use this to set an explicit task state (working, input_required, completed, failed, ...), attach artifacts, and provide a status message.\n\
                 If you include respond.kind, the tools loop will return immediately with that response."
                    .to_string(),
            ),
            parameters: checkpoint_schema,
            kind: crate::agent::tools::ToolKind::Function,
            strict: true,
            parallel_ok: false,
            executor: ToolExecutor::Simple(Arc::new(|args| Box::pin(async move {
                Ok(serde_json::json!({
                    "__engine_stop": true,
                    "__checkpoint_args": args,
                }))
            }))),
        };
        tools.register(checkpoint_task);

        // -- Wire read_skill tool from llm_callable skills --
        let skill_registry_arc = Arc::clone(&self.skill_registry);
        if let Some(read_skill_tool) =
            super::skill::build_wired_read_skill_tool(skill_registry_arc.clone())
        {
            tools.register(read_skill_tool);
            debug!("read_skill tool injected from llm_callable skills");
        }

        // -- Build skill summary for system prompt --
        let skill_summary = build_skill_summary_for_prompt(&self.skill_registry);

        let mut policy = policy.unwrap_or_default();
        policy.checkpoint_mode = checkpoint_env.mode;
        policy.checkpoint_allow_message_response = checkpoint_env.allow_message_response;
        policy.checkpoint_mirror_internal_state_to_task_meta =
            checkpoint_env.mirror_internal_state_to_task_meta;

        let model = model.to_string();
        let sr = skill_registry_arc;
        let handler: MessageHandlerFn = Arc::new(
            move |msg_ctx: MessageContext, task_ctx: Option<TaskContext>| {
                let tools = tools.clone();
                let policy = policy.clone();
                let model = model.clone();
                let inv = llm.clone();
                let sys = system_message.clone();
                let defaults = request_defaults.clone();
                let skill_summary = skill_summary.clone();
                let sr = sr.clone();
                let history_policy_runtime = history_policy_runtime.clone();
                Box::pin(async move {
                    // Resolve skill context if a skill_call is present
                    let skill_ctx = if let Some(ref sc) = msg_ctx.skill_call {
                        sr.resolve_skill_context(&sc.skill_id, &sc.parameters).await
                    } else {
                        None
                    };

                    execute_runtime(
                        inv,
                        &model,
                        &tools,
                        &policy,
                        &msg_ctx,
                        task_ctx,
                        sys.as_deref(),
                        defaults.as_ref(),
                        skill_ctx.as_ref(),
                        skill_summary.as_deref(),
                        history_policy_runtime.as_ref(),
                    )
                    .await
                })
            },
        );

        self.active_handler = Some(handler);
        info!("Activated tools-first LLM handler");
        Ok(())
    }

    /// Handle message using the active handler.
    pub async fn handle_message(
        &self,
        msg_ctx: MessageContext,
        task_ctx: Option<TaskContext>,
    ) -> SdkResult<RuntimeResponse> {
        if let Some(handler) = &self.active_handler {
            debug!("Dispatching to active handler");
            handler(msg_ctx, task_ctx).await
        } else {
            warn!("No active handler set; returning conversational fallback");
            ResponseBuilder::create_conversational_response(&msg_ctx, task_ctx)
        }
    }
}

/// Build a skill summary string for LLM system prompt awareness.
#[cfg(feature = "llm-engine")]
fn build_skill_summary_for_prompt(registry: &SkillRegistry) -> Option<String> {
    let callable = registry.get_llm_callable_skills();
    if callable.is_empty() {
        return None;
    }
    let lines: Vec<String> = callable
        .iter()
        .map(|s| format!("- {}: {}", s.id, s.description))
        .collect();
    Some(format!(
        "\n\n## Available Skills\nYou can call `read_skill` with any of these skill IDs:\n{}",
        lines.join("\n")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::message::SkillExecutor;
    use crate::agent::skill::SkillRegistry;
    use serde_json::json;

    fn make_msg_with_data_skill(
        skill: &str,
        params: serde_json::Value,
    ) -> crate::a2a::MessageSendParams {
        use crate::a2a::{Message, MessageRole, Part};
        let mut data_map = serde_json::Map::new();
        data_map.insert("skill".to_string(), json!(skill));
        if let Some(p) = params.as_object() {
            for (k, v) in p {
                data_map.insert(k.clone(), v.clone());
            }
        }
        let message = Message::new(
            MessageRole::User,
            vec![Part::data(serde_json::Value::Object(data_map))],
            uuid::Uuid::new_v4().to_string(),
        );
        crate::a2a::MessageSendParams {
            message,
            configuration: None,
            metadata: None,
            tenant: None,
        }
    }

    #[tokio::test]
    async fn default_handler_executes_registered_skill() {
        let skill_registry = Arc::new({
            let mut r = SkillRegistry::new();
            r.skill("sum", |p| async move {
                let a = p.get("a").and_then(|v| v.as_i64()).unwrap_or(0);
                let b = p.get("b").and_then(|v| v.as_i64()).unwrap_or(0);
                Ok(json!({"sum": a + b}))
            })
            .register()
            .unwrap();
            r
        });
        let mut mgr = MessageHandlerManager::new(skill_registry);
        mgr.initialize_default_handler();

        let params = make_msg_with_data_skill("sum", json!({"a": 2, "b": 3}));
        let msg_ctx = crate::a2a::message_context_from_params(
            &params,
            false,
            Some(SkillExecutor::new(Arc::clone(&mgr.skill_registry))),
        );
        assert!(
            msg_ctx.skill_call.is_some(),
            "expected skill_call to be extracted"
        );
        let resp = mgr.handle_message(msg_ctx, None).await.expect("ok");
        match resp {
            RuntimeResponse::Task(task) => {
                assert_eq!(task.artifacts.len(), 1);
            }
            _ => panic!("expected Task"),
        }
    }

    #[tokio::test]
    async fn default_handler_skill_not_implemented() {
        let skill_registry = Arc::new(SkillRegistry::new());
        let mut mgr = MessageHandlerManager::new(skill_registry);
        mgr.initialize_default_handler();

        let params = make_msg_with_data_skill("unknown", json!({}));
        let msg_ctx = crate::a2a::message_context_from_params(&params, false, None);
        let resp = mgr.handle_message(msg_ctx, None).await.expect("ok");
        match resp {
            RuntimeResponse::Message(msg) => {
                assert!(
                    msg.message
                        .parts
                        .iter()
                        .any(|p| matches!(p, agent_core::ContentPart::Text(_)))
                );
            }
            _ => panic!("expected Message"),
        }
    }
}
