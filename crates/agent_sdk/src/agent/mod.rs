//! Agent Module
//!
//! This module provides the protocol-neutral runtime used by PromptFleet agents.
//!
//! ## Module Structure
//!
//! - **config**: Agent configuration and communication patterns
//! - **message**: Message types, contexts, and classification
//! - **skill**: Skill registration, execution, and metadata management  
//! - **core**: Main Agent struct and core functionality
//!
//! ## Architecture Benefits
//!
//! - **Single Responsibility**: Each module handles one architectural concern
//! - **Maintainability**: Smaller, focused modules (200-400 lines each)
//! - **Testability**: Easier to test individual components in isolation
//! - **Clean Dependencies**: Follows dependency inversion principles
//! - **Extensibility**: Easy to add new features without affecting other areas

// Declare submodules
pub mod config;
pub mod core;
pub mod message;
pub mod skill;

// New specialized modules for clean architecture
pub(crate) mod history_policy;
pub mod message_handlers;
pub mod response;
pub mod response_builders;
pub mod task_manager;
pub(crate) mod task_store;

// NEW: Tooling for LLM tool registry (separate from SkillRegistry)
#[cfg(feature = "llm-engine")]
pub mod interaction_tools;
#[cfg(feature = "llm-engine")]
pub mod tool_context;
pub mod tools;

// Agent trace events — protocol-agnostic execution trace
#[cfg(feature = "llm-engine")]
pub mod trace;

// NEW (gated): Minimal orchestrator surface for tools-first LLM loop
#[cfg(feature = "llm-engine")]
pub(crate) mod checkpoint;
#[cfg(feature = "llm-engine")]
mod finalization;
#[cfg(feature = "llm-engine")]
pub(crate) mod llm_invoker;
#[cfg(feature = "llm-engine")]
pub mod llm_orchestrator;

// Protocol-agnostic tool engine (WASM + native, feature-gated)
#[cfg(feature = "llm-engine")]
pub mod engine;

#[cfg(test)]
mod tests;

// Re-export public API for convenient access
pub use config::{AgentConfig, HistoryPolicyConfig, HistoryPolicyMode, HistoryStrategyKind};
pub use core::Agent;
#[cfg(feature = "llm-engine")]
pub use core::LlmRuntimeConfigurator;
pub use message::{MessageContext, MessageType, SkillCall, SkillExecutor, TaskContext};
pub use skill::{
    NotificationHandler, SkillContext, SkillDefinition, SkillEntryBuilder, SkillHandler,
    SkillRegistry,
};

// Export handler function type from message_handlers
pub use message_handlers::{MessageHandlerFn, MessageHandlerManager};

// Export specialized builders
pub use response::{
    Response, RuntimeArtifact, RuntimeMessage, RuntimeResponse, RuntimeTask, TaskOpts,
};
pub use response_builders::ResponseBuilder;
pub use task_manager::TaskManager;

// Export tools registry
#[cfg(feature = "llm-engine")]
pub use tool_context::ToolContext;
pub use tools::{ToolExecutionResult, ToolExecutor, ToolRegistry, ToolSpec};

// Export orchestrator types when enabled
#[cfg(feature = "llm-engine")]
pub use llm_orchestrator::LlmPolicy;

// Export trace event types when enabled
#[cfg(feature = "llm-engine")]
pub use trace::AgentTraceEvent;
#[cfg(all(feature = "llm-engine", not(target_arch = "wasm32")))]
pub use trace::AgentTraceStream;

// Export engine types when enabled (universal types + native-only types)
#[cfg(feature = "llm-engine")]
pub use engine::{
    EngineConfig, EngineError, EngineResult, LlmTurnInvoker, RequestResponseTurnInvoker,
    ToolCallInfo, TurnFuture, TurnResult,
};
#[cfg(all(feature = "llm-engine", not(target_arch = "wasm32")))]
pub use engine::{StreamingTurnInvoker, ToolEngine, ToolEngineBuilder};

/// **Agent Module Documentation**
///
/// This module provides the core runtime implementation with the following capabilities:
///
/// ## Core Features
///
/// - **Agent Creation**: Runtime-first constructors for different use cases
/// - **Skill Management**: Register, execute, and manage agent skills
/// - **Message Processing**: Route runtime messages and integrate optional LLM orchestration
/// - **Response Modes**: Control response types (stateful vs stateless)
/// - **Service Injection**: Type-safe dependency injection for external services
///
/// ## Quick Start Examples
///
/// ### Creating an Agent
///
/// ```rust,no_run
/// use agent_sdk::agent::{Agent, AgentConfig};
/// use serde_json::json;
///
/// // Simple runtime agent
/// let mut agent = Agent::new_runtime("my-agent")?;
///
/// // Custom configuration
/// let config = AgentConfig::new("api-agent", "API-only agent").stateless();
/// let mut agent = Agent::new_with_config(config)?;
/// # Ok::<(), agent_sdk::SdkError>(())
/// ```
///
/// ### Registering Skills
///
/// ```rust,no_run
/// # use agent_sdk::agent::Agent;
/// # use serde_json::json;
/// # let mut agent = Agent::new_runtime("test")?;
/// // Simple skill registration
/// agent.skill("get_weather", |params| async move {
///     let location = params["location"].as_str().unwrap_or("unknown");
///     Ok(json!({"location": location, "temp": 22}))
/// }).register()?;
///
/// // Fluent builder with metadata
/// agent.skill("analyze", |params| async move {
///     Ok(json!({"analysis": "complete"}))
/// })
/// .display_name("Data Analysis")
/// .schema(json!({"type": "object", "properties": {"data": {"type": "string"}}}))
/// .tags(&["analytics", "data"])
/// .register()?;
/// # Ok::<(), agent_sdk::SdkError>(())
/// ```
///
/// ### Response Modes
///
/// ```rust,no_run
/// # use agent_sdk::agent::AgentConfig;
/// // Stateful agent (default) - creates Tasks for conversation continuity
/// let config = AgentConfig::default();
///
/// // Stateless agent - prefers lightweight Message responses
/// let config = AgentConfig::new("api-agent", "API Agent").stateless();
/// ```
pub struct AgentModuleDocumentation;
