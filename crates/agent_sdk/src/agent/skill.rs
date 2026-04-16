//! Skill Management Module (Protocol-Independent)
//!
//! Skills are the core behavioral unit of an agent. Each skill has:
//! - **Metadata**: id, name, description, tags, input/output modes
//! - **Instructions** (optional): behavioral guidance injected into LLM context
//! - **Handler** (optional): code that executes on activation (pre-processing or tool call)
//! - **expose**: whether the skill appears on the discovery card (default: true)
//! - **llm_callable**: whether the LLM can invoke the handler via `read_skill` tool (default: false)
//!
//! This module has **zero A2A imports**. Protocol adapters (A2A, MCP, etc.)
//! consume `SkillDefinition` and convert to their own types.

use crate::error::{SdkError, SdkResult};
use log::{debug, error, info, trace};
use serde_json::Value;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use super::message::{MessageContext, TaskContext};

// ---------------------------------------------------------------------------
// Handler types
// ---------------------------------------------------------------------------

/// Skill handler: receives JSON parameters, returns JSON result.
pub type SkillHandler =
    Arc<dyn Fn(Value) -> Pin<Box<dyn Future<Output = Result<Value, String>> + Send>> + Send + Sync>;

/// Context-aware skill handler: also receives execution context.
pub type SkillHandlerWithContext = Arc<
    dyn Fn(
            Value,
            SkillExecutionContext,
        ) -> Pin<Box<dyn Future<Output = Result<Value, String>> + Send>>
        + Send
        + Sync,
>;

/// Notification handler: fire-and-forget, no response.
pub type NotificationHandler =
    Arc<dyn Fn(Value) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send>> + Send + Sync>;

// ---------------------------------------------------------------------------
// SkillDefinition — protocol-independent skill metadata
// ---------------------------------------------------------------------------

/// Protocol-independent skill definition.
///
/// This is the single source of truth for skill metadata inside the SDK.
/// Protocol adapters (A2A `AgentSkill`, MCP, etc.) convert from this type.
#[derive(Debug, Clone)]
pub struct SkillDefinition {
    pub id: String,
    pub name: String,
    pub description: String,
    pub input_modes: Vec<String>,
    pub output_modes: Vec<String>,
    pub schema: Option<Value>,
    pub examples: Option<Vec<String>>,
    pub tags: Option<Vec<String>>,
    /// Behavioral guidance injected into LLM context when this skill is activated.
    pub instructions: Option<String>,
    /// Whether the skill is visible on the discovery card (default: true).
    pub expose: bool,
    /// Whether the LLM can invoke this skill's handler via `read_skill` tool (default: false).
    pub llm_callable: bool,
}

// ---------------------------------------------------------------------------
// SkillOutput / SkillError
// ---------------------------------------------------------------------------

/// Transport-agnostic, string-first skill output.
#[derive(Debug, Clone)]
pub struct SkillOutput {
    pub text: String,
}

impl From<Value> for SkillOutput {
    fn from(value: Value) -> Self {
        match value {
            Value::String(s) => SkillOutput { text: s },
            other => {
                let pretty =
                    serde_json::to_string_pretty(&other).unwrap_or_else(|_| other.to_string());
                SkillOutput { text: pretty }
            }
        }
    }
}

/// Errors specific to skill execution.
#[derive(Debug, Clone)]
pub enum SkillError {
    NotImplemented { skill_id: String },
    ExecutionFailed { skill_id: String, reason: String },
}

impl std::fmt::Display for SkillError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SkillError::NotImplemented { skill_id } => {
                write!(f, "Skill '{}' not implemented", skill_id)
            }
            SkillError::ExecutionFailed { skill_id, reason } => {
                write!(f, "Skill '{}' execution failed: {}", skill_id, reason)
            }
        }
    }
}

impl std::error::Error for SkillError {}

// ---------------------------------------------------------------------------
// SkillExecutionContext
// ---------------------------------------------------------------------------

/// Execution context optionally available to skill handlers.
#[derive(Debug, Clone)]
pub struct SkillExecutionContext {
    pub message_ctx: Arc<MessageContext>,
    pub task_ctx: Option<Arc<TaskContext>>,
}

impl SkillExecutionContext {
    pub fn new(message_ctx: MessageContext, task_ctx: Option<TaskContext>) -> Self {
        Self {
            message_ctx: Arc::new(message_ctx),
            task_ctx: task_ctx.map(Arc::new),
        }
    }
}

// ---------------------------------------------------------------------------
// SkillContext — resolved skill state for LLM injection
// ---------------------------------------------------------------------------

/// Resolved skill context ready for LLM message injection.
///
/// Produced by `SkillRegistry::resolve_skill_context()`. Contains the handler's
/// output (if any) and the skill's instructions (if any).
#[derive(Debug, Clone)]
pub struct SkillContext {
    pub skill_id: String,
    pub handler_output: Option<String>,
    pub instructions: Option<String>,
}

// ---------------------------------------------------------------------------
// SkillRegistry
// ---------------------------------------------------------------------------

/// Manages skill handlers, definitions, and notifications.
///
/// Protocol-independent: no A2A types stored. Protocol adapters read
/// `SkillDefinition` values via accessor methods.
#[derive(Clone)]
pub struct SkillRegistry {
    skill_handlers: HashMap<String, SkillHandler>,
    skill_handlers_with_context: HashMap<String, SkillHandlerWithContext>,
    notification_handlers: HashMap<String, NotificationHandler>,
    skill_definitions: HashMap<String, SkillDefinition>,
}

impl std::fmt::Debug for SkillRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SkillRegistry")
            .field(
                "skill_handlers",
                &format_args!("{} handlers", self.skill_handlers.len()),
            )
            .field(
                "skill_handlers_with_context",
                &format_args!("{} handlers", self.skill_handlers_with_context.len()),
            )
            .field(
                "notification_handlers",
                &format_args!("{} handlers", self.notification_handlers.len()),
            )
            .field("skill_definitions", &self.skill_definitions.len())
            .finish()
    }
}

impl SkillRegistry {
    pub fn new() -> Self {
        Self {
            skill_handlers: HashMap::new(),
            skill_handlers_with_context: HashMap::new(),
            notification_handlers: HashMap::new(),
            skill_definitions: HashMap::new(),
        }
    }

    // -- Fluent registration --

    /// Start registering a skill (optional handler — metadata-only if you omit `.handler()`).
    pub fn add_skill(&mut self, skill_id: &str) -> SkillEntryBuilder<'_> {
        SkillEntryBuilder::new(self, skill_id)
    }

    /// Register a skill with a handler (convenience — same as `add_skill(name).handler(handler)`).
    pub fn skill<F, Fut>(&mut self, name: &str, handler: F) -> SkillEntryBuilder<'_>
    where
        F: Fn(Value) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Value, String>> + Send + 'static,
    {
        self.add_skill(name).handler(handler)
    }

    // -- Execution --

    /// Execute a skill by ID (simple handler path).
    pub async fn execute_skill(&self, skill_id: &str, parameters: &Value) -> Result<Value, String> {
        debug!(
            "Executing skill '{}' with parameters: {}",
            skill_id, parameters
        );

        if let Some(handler) = self.skill_handlers.get(skill_id) {
            let start = std::time::Instant::now();
            let result = handler(parameters.clone()).await;
            let ms = start.elapsed().as_secs_f64() * 1000.0;
            match &result {
                Ok(_) => info!("Skill '{}' executed in {:.2}ms", skill_id, ms),
                Err(e) => error!("Skill '{}' failed after {:.2}ms: {}", skill_id, ms, e),
            }
            result
        } else if self.skill_handlers_with_context.contains_key(skill_id) {
            Err(format!(
                "Skill '{}' requires execution context but none was provided",
                skill_id
            ))
        } else {
            Err(format!(
                "Skill '{}' not found. Available: {:?}",
                skill_id,
                self.list_skills()
            ))
        }
    }

    /// Execute with optional execution context (prefers context-aware handler).
    pub async fn execute_skill_with_ctx(
        &self,
        skill_id: &str,
        parameters: &Value,
        exec_ctx: &SkillExecutionContext,
    ) -> Result<Value, String> {
        if let Some(handler) = self.skill_handlers_with_context.get(skill_id) {
            let start = std::time::Instant::now();
            let result = handler(parameters.clone(), exec_ctx.clone()).await;
            let ms = start.elapsed().as_secs_f64() * 1000.0;
            match &result {
                Ok(_) => info!("Skill '{}' (ctx) executed in {:.2}ms", skill_id, ms),
                Err(e) => error!("Skill '{}' (ctx) failed after {:.2}ms: {}", skill_id, ms, e),
            }
            result
        } else {
            self.execute_skill(skill_id, parameters).await
        }
    }

    /// Execute and return string-first output.
    pub async fn execute_skill_text(
        &self,
        skill_id: &str,
        parameters: &Value,
    ) -> Result<SkillOutput, SkillError> {
        if !self.skill_handlers.contains_key(skill_id)
            && !self.skill_handlers_with_context.contains_key(skill_id)
        {
            return Err(SkillError::NotImplemented {
                skill_id: skill_id.to_string(),
            });
        }
        match self.execute_skill(skill_id, parameters).await {
            Ok(val) => Ok(SkillOutput::from(val)),
            Err(reason) => Err(SkillError::ExecutionFailed {
                skill_id: skill_id.to_string(),
                reason,
            }),
        }
    }

    /// Execute with context and return string-first output.
    pub async fn execute_skill_text_with_ctx(
        &self,
        skill_id: &str,
        parameters: &Value,
        exec_ctx: &SkillExecutionContext,
    ) -> Result<SkillOutput, SkillError> {
        if self.skill_handlers_with_context.contains_key(skill_id) {
            match self
                .execute_skill_with_ctx(skill_id, parameters, exec_ctx)
                .await
            {
                Ok(val) => return Ok(SkillOutput::from(val)),
                Err(reason) => {
                    return Err(SkillError::ExecutionFailed {
                        skill_id: skill_id.to_string(),
                        reason,
                    });
                }
            }
        }
        self.execute_skill_text(skill_id, parameters).await
    }

    // -- Skill context resolution (for LLM injection) --

    /// Resolve skill context: run handler (if present) and collect instructions.
    ///
    /// Returns `None` when neither handler output nor instructions exist
    /// (metadata-only skill — nothing to inject).
    pub async fn resolve_skill_context(
        &self,
        skill_id: &str,
        parameters: &Value,
    ) -> Option<SkillContext> {
        let def = self.skill_definitions.get(skill_id)?;

        let handler_output = if self.skill_handlers.contains_key(skill_id) {
            match self.execute_skill_text(skill_id, parameters).await {
                Ok(out) => Some(out.text),
                Err(SkillError::NotImplemented { .. }) => None,
                Err(SkillError::ExecutionFailed { reason, .. }) => {
                    Some(format!("[handler error: {}]", reason))
                }
            }
        } else {
            None
        };

        let instructions = def.instructions.clone();

        if handler_output.is_none() && instructions.is_none() {
            return None;
        }

        Some(SkillContext {
            skill_id: skill_id.to_string(),
            handler_output,
            instructions,
        })
    }

    // -- Notifications --

    pub fn register_notification<F, Fut>(&mut self, name: &str, handler: F) -> SdkResult<()>
    where
        F: Fn(Value) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(), String>> + Send + 'static,
    {
        if name.is_empty() {
            return Err(SdkError::invalid_input("Notification name cannot be empty"));
        }
        let wrapped: NotificationHandler = Arc::new(
            move |params: Value| -> Pin<Box<dyn Future<Output = Result<(), String>> + Send>> {
                Box::pin(handler(params))
            },
        );
        self.notification_handlers.insert(name.to_string(), wrapped);
        Ok(())
    }

    // -- Accessors --

    pub fn list_skills(&self) -> Vec<String> {
        let mut s: Vec<String> = self.skill_handlers.keys().cloned().collect();
        s.extend(self.skill_handlers_with_context.keys().cloned());
        s
    }

    pub fn list_notifications(&self) -> Vec<String> {
        self.notification_handlers.keys().cloned().collect()
    }

    /// All skill definitions (protocol-independent).
    pub fn get_skill_definitions(&self) -> &HashMap<String, SkillDefinition> {
        &self.skill_definitions
    }

    /// Skills with `expose == true` (for discovery card adapters).
    pub fn get_exposed_skills(&self) -> Vec<&SkillDefinition> {
        self.skill_definitions
            .values()
            .filter(|d| d.expose)
            .collect()
    }

    /// Skills with `llm_callable == true` (for read_skill tool generation).
    pub fn get_llm_callable_skills(&self) -> Vec<&SkillDefinition> {
        self.skill_definitions
            .values()
            .filter(|d| d.llm_callable)
            .collect()
    }

    /// Get instructions for a skill.
    pub fn get_instructions(&self, skill_id: &str) -> Option<&str> {
        self.skill_definitions
            .get(skill_id)
            .and_then(|d| d.instructions.as_deref())
    }

    pub fn get_skill_handlers(&self) -> &HashMap<String, SkillHandler> {
        &self.skill_handlers
    }

    pub fn get_notification_handlers(&self) -> &HashMap<String, NotificationHandler> {
        &self.notification_handlers
    }

    // -- Builder entry point --

    // -- Internal registration (called by SkillEntryBuilder) --

    pub(crate) fn register_metadata_entry(
        &mut self,
        name: &str,
        definition: SkillDefinition,
    ) -> SdkResult<()> {
        if name.is_empty() {
            return Err(SdkError::invalid_input("Skill name cannot be empty"));
        }
        if self.skill_handlers.contains_key(name)
            || self.skill_handlers_with_context.contains_key(name)
            || self.skill_definitions.contains_key(name)
        {
            return Err(SdkError::invalid_input(format!(
                "Skill '{}' already exists",
                name
            )));
        }

        self.skill_definitions.insert(name.to_string(), definition);
        trace!("Skill '{}' registered (metadata only)", name);
        Ok(())
    }

    pub(crate) fn register_from_builder(
        &mut self,
        name: &str,
        handler: SkillHandler,
        definition: SkillDefinition,
    ) -> SdkResult<()> {
        if name.is_empty() {
            return Err(SdkError::invalid_input("Skill name cannot be empty"));
        }
        if self.skill_handlers.contains_key(name)
            || self.skill_handlers_with_context.contains_key(name)
            || self.skill_definitions.contains_key(name)
        {
            return Err(SdkError::invalid_input(format!(
                "Skill '{}' already exists",
                name
            )));
        }

        self.skill_handlers.insert(name.to_string(), handler);
        self.skill_definitions.insert(name.to_string(), definition);
        trace!("Skill '{}' registered via builder", name);
        Ok(())
    }

    /// Register a context-aware skill handler for test and benchmark support.
    ///
    /// This is feature-gated to avoid expanding the default production API
    /// surface while still allowing native seam harnesses to benchmark the real
    /// `execute_skill_with_ctx` path.
    #[cfg(feature = "test-support")]
    pub fn register_contextual_skill_for_test<F, Fut>(
        &mut self,
        name: &str,
        handler: F,
        definition: SkillDefinition,
    ) -> SdkResult<()>
    where
        F: Fn(Value, SkillExecutionContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Value, String>> + Send + 'static,
    {
        if name.is_empty() {
            return Err(SdkError::invalid_input("Skill name cannot be empty"));
        }
        if self.skill_handlers.contains_key(name)
            || self.skill_handlers_with_context.contains_key(name)
            || self.skill_definitions.contains_key(name)
        {
            return Err(SdkError::invalid_input(format!(
                "Skill '{}' already exists",
                name
            )));
        }

        let contextual_handler: SkillHandlerWithContext = Arc::new(
            move |params: Value,
                  exec_ctx: SkillExecutionContext|
                  -> Pin<Box<dyn Future<Output = Result<Value, String>> + Send>> {
                Box::pin(handler(params, exec_ctx))
            },
        );

        self.skill_handlers_with_context
            .insert(name.to_string(), contextual_handler);
        self.skill_definitions.insert(name.to_string(), definition);
        trace!("Context-aware skill '{}' registered for test support", name);
        Ok(())
    }
}

impl Default for SkillRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// inject_skill_context — LLM message enrichment (requires `llm-engine`)
// ---------------------------------------------------------------------------

/// Enrich LLM messages with resolved skill context.
///
/// Appends instructions and/or handler output to the system message.
#[cfg(feature = "llm-engine")]
pub fn inject_skill_context(messages: &mut Vec<llm_client::ChatMessage>, ctx: &SkillContext) {
    let mut supplement = format!("\n\n## Active Skill: {}", ctx.skill_id);
    if let Some(ref instr) = ctx.instructions {
        supplement.push_str(&format!("\n\n### Instructions\n{}", instr));
    }
    if let Some(ref output) = ctx.handler_output {
        supplement.push_str(&format!("\n\n### Pre-fetched Context\n{}", output));
    }

    if let Some(first) = messages.first_mut() {
        if first.role == "system" {
            if let Some(ref content) = first.content {
                first.content = Some(format!("{}{}", content, supplement));
                return;
            }
        }
    }
    messages.insert(
        0,
        llm_client::ChatMessage {
            role: "system".into(),
            content: Some(supplement.trim_start().to_string()),
            ..Default::default()
        },
    );
}

// ---------------------------------------------------------------------------
// build_read_skill_tool — LLM tool for invoking skills
// ---------------------------------------------------------------------------

/// Build a `read_skill` `ToolSpec` from all `llm_callable` skills in the registry.
///
/// Returns `None` if no skills are llm_callable.
#[cfg(feature = "llm-engine")]
pub fn build_read_skill_tool(registry: &SkillRegistry) -> Option<super::tools::ToolSpec> {
    use super::tools::{ToolExecutor, ToolKind, ToolSpec};

    let callable = registry.get_llm_callable_skills();
    if callable.is_empty() {
        return None;
    }

    let skill_ids: Vec<&str> = callable.iter().map(|s| s.id.as_str()).collect();
    let descriptions: Vec<String> = callable
        .iter()
        .map(|s| format!("- {}: {}", s.id, s.description))
        .collect();

    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "skill_id": {
                "type": "string",
                "enum": skill_ids,
                "description": format!(
                    "ID of the skill to read/activate:\n{}",
                    descriptions.join("\n")
                )
            }
        },
        "required": ["skill_id"],
        "additionalProperties": false
    });

    Some(ToolSpec {
        name: "read_skill".to_string(),
        description: Some(
            "Read/activate a registered skill to retrieve context or data.".to_string(),
        ),
        parameters: schema,
        kind: ToolKind::Skill,
        strict: true,
        parallel_ok: false,
        executor: ToolExecutor::Simple(Arc::new(|_args| {
            Box::pin(async move {
                // Placeholder — the actual dispatch happens in the LLM handler closure
                // which has access to the SkillRegistry. This executor is replaced at
                // wiring time in set_llm_tools_handler_configured / configure_llm_runtime.
                Ok(serde_json::json!({"error": "read_skill executor not wired"}))
            })
        })),
    })
}

/// Build a wired `read_skill` executor that dispatches to the given skill registry.
#[cfg(feature = "llm-engine")]
pub fn build_wired_read_skill_tool(registry: Arc<SkillRegistry>) -> Option<super::tools::ToolSpec> {
    use super::tools::{ToolExecutor, ToolKind, ToolSpec};

    let callable = registry.get_llm_callable_skills();
    if callable.is_empty() {
        return None;
    }

    let skill_ids: Vec<String> = callable.iter().map(|s| s.id.clone()).collect();
    let descriptions: Vec<String> = callable
        .iter()
        .map(|s| format!("- {}: {}", s.id, s.description))
        .collect();

    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "skill_id": {
                "type": "string",
                "enum": skill_ids,
                "description": format!(
                    "ID of the skill to read/activate:\n{}",
                    descriptions.join("\n")
                )
            }
        },
        "required": ["skill_id"],
        "additionalProperties": false
    });

    let reg = registry.clone();
    Some(ToolSpec {
        name: "read_skill".to_string(),
        description: Some(
            "Read/activate a registered skill to retrieve context or data.".to_string(),
        ),
        parameters: schema,
        kind: ToolKind::Skill,
        strict: true,
        parallel_ok: false,
        executor: ToolExecutor::Simple(Arc::new(move |args| {
            let reg = reg.clone();
            Box::pin(async move {
                let skill_id = args.get("skill_id").and_then(|v| v.as_str()).unwrap_or("");
                if skill_id.is_empty() {
                    return Ok(serde_json::json!({"error": "skill_id is required"}));
                }
                match reg.execute_skill(skill_id, &Value::Null).await {
                    Ok(val) => Ok(val),
                    Err(e) => Ok(serde_json::json!({"error": e})),
                }
            })
        })),
    })
}

// ---------------------------------------------------------------------------
// SkillEntryBuilder — fluent registration API (optional handler)
// ---------------------------------------------------------------------------

/// Fluent builder for skill registration.
///
/// Created via [`SkillRegistry::add_skill`] / [`crate::Agent::add_skill`], or [`SkillRegistry::skill`] /
/// [`crate::Agent::skill`] when providing a handler. Finalize with [`.register()`](Self::register).
pub struct SkillEntryBuilder<'a> {
    registry: &'a mut SkillRegistry,
    name: String,
    handler: Option<SkillHandler>,
    display_name: Option<String>,
    description: Option<String>,
    schema: Option<Value>,
    examples: Option<Vec<String>>,
    tags: Option<Vec<String>>,
    input_modes: Option<Vec<String>>,
    output_modes: Option<Vec<String>>,
    instructions: Option<String>,
    expose: bool,
    llm_callable: bool,
}

impl<'a> SkillEntryBuilder<'a> {
    pub(crate) fn new(registry: &'a mut SkillRegistry, name: &str) -> Self {
        Self {
            registry,
            name: name.to_string(),
            handler: None,
            display_name: None,
            description: None,
            schema: None,
            examples: None,
            tags: None,
            input_modes: None,
            output_modes: None,
            instructions: None,
            expose: true,
            llm_callable: false,
        }
    }

    /// Attach an async handler. Omit for metadata-only skills (discovery card + LLM awareness).
    pub fn handler<F, Fut>(mut self, handler: F) -> Self
    where
        F: Fn(Value) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Value, String>> + Send + 'static,
    {
        self.handler = Some(Arc::new(
            move |params: Value| -> Pin<Box<dyn Future<Output = Result<Value, String>> + Send>> {
                Box::pin(handler(params))
            },
        ));
        self
    }

    pub fn display_name<S: Into<String>>(mut self, name: S) -> Self {
        self.display_name = Some(name.into());
        self
    }

    pub fn description<S: Into<String>>(mut self, desc: S) -> Self {
        self.description = Some(desc.into());
        self
    }

    pub fn schema(mut self, schema: Value) -> Self {
        self.schema = Some(schema);
        self
    }

    pub fn examples(mut self, examples: Vec<String>) -> Self {
        self.examples = Some(examples);
        self
    }

    pub fn example<S: Into<String>>(mut self, example: S) -> Self {
        let example = example.into();
        match &mut self.examples {
            Some(examples) => examples.push(example),
            None => self.examples = Some(vec![example]),
        }
        self
    }

    pub fn tags(mut self, tags: &[&str]) -> Self {
        self.tags = Some(tags.iter().map(|&s| s.to_string()).collect());
        self
    }

    pub fn tag<S: Into<String>>(mut self, tag: S) -> Self {
        let tag = tag.into();
        match &mut self.tags {
            Some(tags) => tags.push(tag),
            None => self.tags = Some(vec![tag]),
        }
        self
    }

    pub fn input_modes(mut self, modes: &[&str]) -> Self {
        self.input_modes = Some(modes.iter().map(|&s| s.to_string()).collect());
        self
    }

    pub fn output_modes(mut self, modes: &[&str]) -> Self {
        self.output_modes = Some(modes.iter().map(|&s| s.to_string()).collect());
        self
    }

    pub fn json_only(mut self) -> Self {
        self.input_modes = Some(vec!["application/json".to_string()]);
        self.output_modes = Some(vec!["application/json".to_string()]);
        self
    }

    pub fn text_only(mut self) -> Self {
        self.input_modes = Some(vec!["text/plain".to_string()]);
        self.output_modes = Some(vec!["text/plain".to_string()]);
        self
    }

    /// Set behavioral instructions (injected into LLM context when skill is activated).
    pub fn instructions<S: Into<String>>(mut self, text: S) -> Self {
        self.instructions = Some(text.into());
        self
    }

    /// Control whether the skill is visible on the discovery card (default: true).
    pub fn expose(mut self, visible: bool) -> Self {
        self.expose = visible;
        self
    }

    /// Control whether the LLM can invoke this skill via `read_skill` tool (default: false).
    pub fn llm_callable(mut self, callable: bool) -> Self {
        self.llm_callable = callable;
        self
    }

    /// Finalize registration.
    pub fn register(self) -> SdkResult<()> {
        let SkillEntryBuilder {
            registry,
            name,
            handler,
            display_name,
            description,
            schema,
            examples,
            tags,
            input_modes,
            output_modes,
            instructions,
            expose,
            llm_callable,
        } = self;

        let display = display_name.as_deref().unwrap_or(&name);
        let desc = description.unwrap_or_else(|| format!("{}: User-defined skill", display));

        let definition = SkillDefinition {
            id: name.clone(),
            name: display.to_string(),
            description: desc,
            input_modes: input_modes
                .unwrap_or_else(|| vec!["application/json".to_string(), "text/plain".to_string()]),
            output_modes: output_modes
                .unwrap_or_else(|| vec!["application/json".to_string(), "text/plain".to_string()]),
            schema,
            examples,
            tags,
            instructions,
            expose,
            llm_callable,
        };

        match handler {
            Some(h) => registry.register_from_builder(&name, h, definition),
            None => registry.register_metadata_entry(&name, definition),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn test_skill_builder_basic() {
        let mut reg = SkillRegistry::new();
        reg.skill("echo", |params| async move {
            Ok(json!({"ok": true, "p": params}))
        })
        .register()
        .unwrap();

        let out = reg
            .execute_skill_text("echo", &json!({"a": 1}))
            .await
            .expect("should work");
        let val: Value = serde_json::from_str(&out.text).expect("valid json");
        assert_eq!(val["ok"], json!(true));
        assert_eq!(val["p"]["a"], json!(1));
    }

    #[tokio::test]
    async fn test_skill_builder_with_instructions() {
        let mut reg = SkillRegistry::new();
        reg.skill(
            "research",
            |_p| async move { Ok(json!({"data": "fetched"})) },
        )
        .instructions("Break query into sub-questions. Cross-reference claims.")
        .register()
        .unwrap();

        assert_eq!(
            reg.get_instructions("research"),
            Some("Break query into sub-questions. Cross-reference claims.")
        );
    }

    #[test]
    fn test_skill_instructions_empty_by_default() {
        let mut reg = SkillRegistry::new();
        reg.skill("plain", |_p| async move { Ok(json!({})) })
            .register()
            .unwrap();

        assert_eq!(reg.get_instructions("plain"), None);
    }

    #[test]
    fn test_expose_and_llm_callable_defaults() {
        let mut reg = SkillRegistry::new();
        reg.skill("default_skill", |_p| async move { Ok(json!({})) })
            .register()
            .unwrap();

        let def = reg.get_skill_definitions().get("default_skill").unwrap();
        assert!(def.expose, "expose should default to true");
        assert!(!def.llm_callable, "llm_callable should default to false");
    }

    #[test]
    fn test_expose_false_hides_from_exposed() {
        let mut reg = SkillRegistry::new();
        reg.skill("hidden", |_p| async move { Ok(json!({})) })
            .expose(false)
            .register()
            .unwrap();
        reg.skill("visible", |_p| async move { Ok(json!({})) })
            .register()
            .unwrap();

        let exposed = reg.get_exposed_skills();
        assert_eq!(exposed.len(), 1);
        assert_eq!(exposed[0].id, "visible");
    }

    #[test]
    fn test_llm_callable_filter() {
        let mut reg = SkillRegistry::new();
        reg.skill("tool_skill", |_p| async move { Ok(json!({})) })
            .llm_callable(true)
            .register()
            .unwrap();
        reg.skill("regular", |_p| async move { Ok(json!({})) })
            .register()
            .unwrap();

        let callable = reg.get_llm_callable_skills();
        assert_eq!(callable.len(), 1);
        assert_eq!(callable[0].id, "tool_skill");
    }

    #[tokio::test]
    async fn test_resolve_skill_context_handler_and_instructions() {
        let mut reg = SkillRegistry::new();
        reg.skill(
            "fetch",
            |_p| async move { Ok(json!({"records": [1, 2, 3]})) },
        )
        .instructions("Analyze the records for anomalies.")
        .register()
        .unwrap();

        let ctx = reg
            .resolve_skill_context("fetch", &Value::Null)
            .await
            .expect("should resolve");
        assert_eq!(ctx.skill_id, "fetch");
        assert!(ctx.handler_output.is_some());
        assert!(ctx.instructions.is_some());
        assert!(ctx.instructions.unwrap().contains("anomalies"));
    }

    #[tokio::test]
    async fn test_resolve_skill_context_instructions_only() {
        let mut reg = SkillRegistry::new();
        reg.add_skill("guide")
            .description("A guidance-only skill")
            .register()
            .unwrap();
        // Manually set instructions on the definition
        if let Some(def) = reg.skill_definitions.get_mut("guide") {
            def.instructions = Some("Follow these steps carefully.".to_string());
        }

        let ctx = reg
            .resolve_skill_context("guide", &Value::Null)
            .await
            .expect("should resolve");
        assert!(ctx.handler_output.is_none());
        assert_eq!(
            ctx.instructions.as_deref(),
            Some("Follow these steps carefully.")
        );
    }

    #[tokio::test]
    async fn test_resolve_skill_context_metadata_only_returns_none() {
        let mut reg = SkillRegistry::new();
        reg.add_skill("empty")
            .description("No handler, no instructions")
            .register()
            .unwrap();

        let ctx = reg.resolve_skill_context("empty", &Value::Null).await;
        assert!(ctx.is_none());
    }

    #[cfg(feature = "llm-engine")]
    #[test]
    fn test_inject_skill_context_appends_to_system() {
        let mut messages = vec![
            llm_client::ChatMessage {
                role: "system".into(),
                content: Some("You are a helpful assistant.".into()),
                ..Default::default()
            },
            llm_client::ChatMessage {
                role: "user".into(),
                content: Some("Hello".into()),
                ..Default::default()
            },
        ];
        let ctx = SkillContext {
            skill_id: "research".to_string(),
            handler_output: Some("Found 3 papers.".to_string()),
            instructions: Some("Summarize findings.".to_string()),
        };

        inject_skill_context(&mut messages, &ctx);

        let sys = messages[0].content.as_deref().unwrap();
        assert!(sys.starts_with("You are a helpful assistant."));
        assert!(sys.contains("## Active Skill: research"));
        assert!(sys.contains("### Instructions\nSummarize findings."));
        assert!(sys.contains("### Pre-fetched Context\nFound 3 papers."));
    }

    #[cfg(feature = "llm-engine")]
    #[test]
    fn test_inject_skill_context_creates_system_if_missing() {
        let mut messages = vec![llm_client::ChatMessage {
            role: "user".into(),
            content: Some("Hello".into()),
            ..Default::default()
        }];
        let ctx = SkillContext {
            skill_id: "test".to_string(),
            handler_output: None,
            instructions: Some("Do the thing.".to_string()),
        };

        inject_skill_context(&mut messages, &ctx);

        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, "system");
        let sys = messages[0].content.as_deref().unwrap();
        assert!(sys.contains("## Active Skill: test"));
        assert!(sys.contains("Do the thing."));
    }

    #[tokio::test]
    async fn test_duplicate_skill_rejected() {
        let mut reg = SkillRegistry::new();
        reg.skill("dup", |_p| async move { Ok(json!({})) })
            .register()
            .unwrap();
        let result = reg
            .skill("dup", |_p| async move { Ok(json!({})) })
            .register();
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_empty_name_rejected() {
        let mut reg = SkillRegistry::new();
        let result = reg.skill("", |_p| async move { Ok(json!({})) }).register();
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_skill_not_found_error() {
        let reg = SkillRegistry::new();
        let err = reg
            .execute_skill_text("missing", &json!({}))
            .await
            .err()
            .expect("error");
        match err {
            SkillError::NotImplemented { skill_id } => assert_eq!(skill_id, "missing"),
            _ => panic!("wrong error variant"),
        }
    }

    #[cfg(feature = "llm-engine")]
    #[test]
    fn test_build_read_skill_tool_none_when_no_callable() {
        let reg = SkillRegistry::new();
        assert!(build_read_skill_tool(&reg).is_none());
    }

    #[cfg(feature = "llm-engine")]
    #[test]
    fn test_build_read_skill_tool_has_enum() {
        let mut reg = SkillRegistry::new();
        reg.skill("alpha", |_| async move { Ok(json!({})) })
            .llm_callable(true)
            .register()
            .unwrap();
        reg.skill("beta", |_| async move { Ok(json!({})) })
            .llm_callable(true)
            .register()
            .unwrap();
        reg.skill("gamma", |_| async move { Ok(json!({})) })
            .register()
            .unwrap();

        let tool = build_read_skill_tool(&reg).expect("should produce tool");
        assert_eq!(tool.name, "read_skill");
        let enum_vals = tool.parameters["properties"]["skill_id"]["enum"]
            .as_array()
            .unwrap();
        assert_eq!(enum_vals.len(), 2);
    }
}
