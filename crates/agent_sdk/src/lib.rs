//! # Agent SDK
//!
//! Runtime-first SDK for building PromptFleet agents with optional protocol
//! adapters.
//!
//! The public foundation of this crate is the agent runtime: skills, tools,
//! services, protocol-neutral message types, and runtime event streams.
//! Protocol adapters such as A2A and AG-UI are enabled through explicit
//! features rather than defining the identity of the crate.
//!
//! The canonical model is:
//! - build one [`Agent`]
//! - compose A2A support with [`crate::a2a`]
//! - compose AG-UI support with [`crate::agui`]
//!
//! ## Feature Presets
//!
//! - `agui-agent`: user-facing AG-UI agent runtime
//! - `a2a-agent`: A2A-capable interoperable agent runtime
//! - `dual-agent`: both A2A and AG-UI
//! - `pf-agent`: PromptFleet opinionated full bundle
//!
//! ## Which Features Do I Need?
//!
//! - Building a runtime with skills, tools, and your own loop: `agent-core`
//! - Adding the built-in LLM loop: `llm-engine`
//! - Serving only a user-facing AG-UI experience: `agui-agent`
//! - Serving and calling A2A agents: `a2a-agent`
//! - Supporting both A2A and AG-UI in one runtime: `dual-agent`
//! - Using the full PromptFleet stack: `pf-agent`
//! - Adding context trimming or token budgeting: `context-window`
//! - Adding observability, storage, or sub-agents: compose the matching primitive features on top
//!
//! ## Feature matrix (surfaces)
//!
//! | Surface | Native | WASM |
//! | --- | --- | --- |
//! | [`AgentBuilder`] | yes | yes |
//! | [`crate::a2a::A2aApp`] | yes | yes |
//! | [`crate::a2a::A2aClient`] | yes | target-gated |
//! | [`AgentHostBuilder`] + A2A | yes | yes |
//! | [`AgentHostBuilder`] + AG-UI | yes | no |
//!
//! ## Quick Start
//!
//! ### Core Agent
//!
//! ```rust,no_run
//! use agent_sdk::{AgentBuilder, error::SdkResult};
//! use serde_json::json;
//!
//! #[tokio::main]
//! async fn main() -> SdkResult<()> {
//!     let mut agent = AgentBuilder::new("weather-agent")?.build()?;
//!
//!     agent
//!         .add_skill("get_weather")
//!         .handler(|params| async move {
//!             let location = params["location"].as_str().unwrap_or("unknown");
//!             Ok(json!({"location": location, "temp": 22, "condition": "sunny"}))
//!         })
//!         .register()?;
//!
//!     // Metadata-only skill (no handler): use `add_skill("id").description("...").register()?`
//!
//!     Ok(())
//! }
//! ```
//!
//! ### LLM runtime (`llm-engine`)
//!
//! On **native** with `llm-engine`, call [`crate::Agent::configure_llm_runtime`] after
//! [`AgentBuilder::build`] with an OpenAI-compatible [`llm_client::LlmClient`] (or another type
//! that implements the invoker traits) and a [`crate::agent::tools::ToolRegistry`]:
//!
//! ```rust,ignore
//! use agent_sdk::{AgentBuilder, SdkResult};
//! use agent_sdk::agent::tools::ToolRegistry;
//! use llm_client::{LlmClient, WireFormat};
//! use llm_client::auth::ApiKeyAuth;
//!
//! fn wire_llm() -> SdkResult<()> {
//!     let mut agent = AgentBuilder::new("my-agent")?.build()?;
//!     let client = LlmClient::builder(WireFormat::OpenAiCompat)
//!         .base_url("https://api.openai.com/v1")
//!         .auth(ApiKeyAuth::new("sk-..."))
//!         .build()
//!         .map_err(|e| agent_sdk::SdkError::configuration(e.to_string()))?;
//!     let tools = ToolRegistry::new();
//!     agent.configure_llm_runtime(client, "gpt-4o-mini", tools, None, None, None)?;
//!     Ok(())
//! }
//! ```
//!
//! ### Migration from older SDK snapshots
//!
//! - Use [`AgentBuilder::new`] or [`AgentBuilder::from_config`] instead of crate-root `new`/`new_runtime` helpers (removed).
//! - Use [`crate::Agent::configure_llm_runtime`] instead of `set_llm_tools_message_handler_configured` / `_with`.
//! - Use [`SkillEntryBuilder`] via [`crate::Agent::add_skill`] for optional handler + full metadata.
//!
//! ### Native Host Composition
//!
//! ```rust,no_run
//! # #[cfg(all(not(target_arch = "wasm32"), feature = "dual-agent"))]
//! # {
//! use agent_sdk::{AgentBuilder, AgentHostBuilder, SdkResult};
//! use agent_sdk::agui::AgUiConfig;
//!
//! # fn example() -> SdkResult<()> {
//! let agent = AgentBuilder::from_config_path("agent.json")?.build()?;
//! let router = AgentHostBuilder::new(agent)
//!     .with_a2a()
//!     .with_agui(AgUiConfig::default())
//!     .build_router()?;
//! # let _ = router;
//! # Ok(())
//! # }
//! # }
//! ```

// Protocol-agnostic agent domain types (no A2A dependency needed by consumers)
pub use agent_core;
mod conversions;

// Re-export protocol-neutral runtime vocabulary by default.
pub use agent_core::{AgentMessage, ContentPart, ConversationContext, Role, TaskPhase};

/// A2A-specific types and composition helpers.
pub mod a2a;

/// AG-UI-specific streaming/composition helpers.
pub mod agui;

// Core agent implementation
mod a2a_app;
pub mod agent;
pub mod builder;
pub mod callable;
pub mod error;
pub mod host;
pub mod interaction;
pub mod services;

// Optional features
#[cfg(feature = "a2a-client")]
mod client;

#[cfg(feature = "a2a-server")]
mod server;

#[cfg(feature = "a2a-server")]
pub mod wasm_kv_task_storage;

#[cfg(all(not(target_arch = "wasm32"), feature = "redis-storage"))]
pub mod redis_task_storage;

pub mod runtime_vars;
pub mod timeout_policy;

#[cfg(all(not(target_arch = "wasm32"), feature = "sub-agents"))]
mod a2a_sub_agent;

// SDK-owned observability flush/runtime helpers.
#[cfg(feature = "agent-observability")]
pub mod observability_runtime;

// A2A-native tool helpers (JSON-RPC helpers exposed as LLM tools)
#[cfg(feature = "a2a-tools")]
pub mod a2a_tools;

// NEW: MCP tools integration (outbound + inbound interface)
#[cfg(feature = "mcp-client")]
pub mod mcp_tools;

#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
pub mod streaming;

#[cfg(all(not(target_arch = "wasm32"), feature = "sub-agents"))]
pub mod sub_agent;

// Re-export key types
pub use agent::{Agent as AgentRuntime, AgentConfig as RuntimeConfig};
pub use agent::{
    Agent, AgentConfig, HistoryPolicyConfig, HistoryPolicyMode, HistoryStrategyKind, MessageType,
    SkillCall, SkillEntryBuilder,
};
pub use callable::CallableSkill;
pub use error::{SdkError, SdkResult};
pub use host::{AgentHost, AgentHostBuilder};
pub use interaction::{
    InteractionKind, InteractionOption, InteractionRequest, InteractionResponse,
};
pub use timeout_policy::TimeoutPolicy;

pub use services::ServiceContainer;

// Re-export protocol-independent skill types
pub use agent::skill::SkillContext;
pub use agent::skill::SkillDefinition;

pub use builder::AgentBuilder;

#[cfg(feature = "agent-observability")]
pub use observability_runtime::ObservabilityRuntime;

// Re-export A2A tool helpers (when enabled)
#[cfg(feature = "a2a-tools")]
pub use a2a_tools::tools::{
    A2AToolConfig, make_tools_from_config as a2a_tools_from_config,
    make_tools_from_names as a2a_tools_from_names,
};

/// SDK version info
pub const SDK_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Create a new A2A client (requires `a2a-client`)
#[cfg(feature = "a2a-client")]
pub fn new_client(endpoint: &str) -> Result<a2a::A2aClient, SdkError> {
    a2a::A2aClient::new(endpoint)
}

/// **🚀 Zero-Boilerplate Agent Macro** - For ultimate convenience
///
/// This macro generates the complete `#[http_component]` function for you:
///
/// ```rust,ignore
/// use agent_sdk::a2a_serve;
///
/// a2a_serve! {
///     AgentBuilder::new("my-agent")?.build()?
///         .skill("echo", |params| async move {
///             Ok(json!({"echo": params}))
///         }).register()?
/// }
/// ```
///
/// Expands to:
/// ```rust,ignore
/// #[spin_sdk::http_component]
/// fn handle_request(req: spin_sdk::http::Request) -> anyhow::Result<spin_sdk::http::Response> {
///     static APP: std::sync::OnceLock<agent_sdk::a2a::A2aApp> = std::sync::OnceLock::new();
///     let app = APP.get_or_init(|| {
///         let agent = AgentBuilder::new("my-agent")
///             .and_then(|builder| {
///                 let mut agent = builder.build()?;
///                 agent.skill("echo", |params| async move {
///                     Ok(json!({"echo": params}))
///                 }).register()?;
///                 Ok(agent)
///             })
///             .expect("Failed to initialize agent");
///         agent_sdk::a2a::app(agent).expect("Failed to initialize A2A app")
///     });
///     Ok(app.serve(req)?)
/// }
/// ```
#[cfg(all(feature = "a2a-server", target_arch = "wasm32"))]
#[macro_export]
macro_rules! a2a_serve {
    ($agent_expr:expr) => {
        static APP: std::sync::OnceLock<$crate::a2a::A2aApp> = std::sync::OnceLock::new();

        #[spin_sdk::http_component]
        fn handle_request(
            req: spin_sdk::http::Request,
        ) -> anyhow::Result<spin_sdk::http::Response> {
            let app = APP.get_or_init(|| {
                let agent = ($agent_expr).expect("Failed to initialize agent");
                $crate::a2a::app(agent).expect("Failed to initialize A2A app")
            });
            Ok(app.serve(req)?)
        }
    };
}

/// Prelude module for common imports
pub mod prelude {
    pub use crate::{
        AgentMessage, AgentRuntime, ContentPart, MessageType, Role, RuntimeConfig, SdkError,
        ServiceContainer, SkillCall, SkillDefinition, SkillEntryBuilder, TaskPhase,
    };

    pub use agent_core::ConversationContext;

    #[cfg(feature = "a2a-client")]
    pub use crate::a2a::A2aClient;
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn test_core_agent_workflow() {
        let mut agent =
            AgentRuntime::new_runtime("test-workflow-agent").expect("Should create agent");

        let skill_result = agent
            .skill("process_data", |params| async move {
                let data = params.get("data").and_then(|v| v.as_str()).unwrap_or("");
                Ok(json!({"processed": format!("Processed: {}", data)}))
            })
            .register();
        assert!(skill_result.is_ok(), "Should register skill successfully");

        let agent_card = a2a::agent_card(&agent);
        assert_eq!(agent_card.name, "test-workflow-agent");

        let skills = agent.list_skills();
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0], "process_data");
    }

    #[cfg(feature = "a2a-client")]
    #[test]
    fn test_client_creation_and_configuration() {
        // Test client creation with various configurations
        let client_result = a2a::A2aClient::new("http://localhost:8080");
        assert!(
            client_result.is_ok(),
            "Should create client with valid endpoint"
        );

        let invalid_client = a2a::A2aClient::new("");
        assert!(invalid_client.is_err(), "Should fail with empty endpoint");

        let direct_client = a2a::A2aClient::direct("http://agent:3000");
        assert!(direct_client.is_ok(), "Should create direct client");
    }

    #[test]
    fn test_service_container_integration() {
        // Test the dependency injection system
        #[derive(Clone, Debug)]
        struct TestService {
            value: i32,
        }

        #[derive(Clone, Debug)]
        struct AnotherService {
            name: String,
        }

        let mut container = ServiceContainer::new();

        // Register multiple services
        container.register(TestService { value: 42 });
        container.register(AnotherService {
            name: "test".to_string(),
        });

        // Retrieve services
        let test_service: Option<std::sync::Arc<TestService>> = container.get();
        let another_service: Option<std::sync::Arc<AnotherService>> = container.get();

        assert!(test_service.is_some());
        assert!(another_service.is_some());
        assert_eq!(test_service.unwrap().value, 42);
        assert_eq!(another_service.unwrap().name, "test");
    }

    #[test]
    fn test_error_handling_comprehensive() {
        // Test comprehensive error scenarios

        // Configuration errors
        let config_err = SdkError::configuration("Invalid configuration parameter");
        assert_eq!(config_err.category(), "configuration");
        assert!(!config_err.is_recoverable());

        // Skill registration errors
        let skill_err = SdkError::skill_registration("get_weather", "Handler not provided");
        assert_eq!(skill_err.category(), "skill");
        assert!(!skill_err.is_recoverable());

        // A2A protocol errors
        let a2a_err = SdkError::A2AProtocol(a2a::A2AError::internal("Network timeout"));
        assert_eq!(a2a_err.category(), "a2a_protocol");

        // Check error formatting
        let formatted = format!("{}", skill_err);
        assert!(formatted.contains("get_weather"));
        assert!(formatted.contains("Handler not provided"));
    }

    #[test]
    fn test_agent_configurations() {
        // Test different agent configurations
        let default_config = AgentConfig::default();
        assert_eq!(default_config.name, "sdk-agent");
        assert!(!default_config.stateless_methods);

        let agent_result = Agent::new_with_config(default_config);
        assert!(agent_result.is_ok());

        // Test custom configuration
        let custom_config = AgentConfig::new("custom-agent", "Custom Agent").stateless();
        assert!(custom_config.stateless_methods);

        let custom_agent = Agent::new_with_config(custom_config);
        assert!(custom_agent.is_ok());

        // Test builder methods
        let stateful_config = AgentConfig::new("stateful-agent", "Stateful Agent").stateful();
        assert!(!stateful_config.stateless_methods);

        let stateful_agent = Agent::new_with_config(stateful_config);
        assert!(stateful_agent.is_ok());
    }

    #[tokio::test]
    async fn test_end_to_end_agent_workflow() {
        let mut agent =
            AgentRuntime::new_runtime("weather-calculator-agent").expect("Should create agent");

        agent
            .skill("get_weather", |params| async move {
                let location = params
                    .get("location")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                Ok(json!({
                    "location": location,
                    "temperature": 22,
                    "condition": "sunny",
                    "humidity": 60
                }))
            })
            .register()
            .expect("Should register weather skill");

        agent
            .skill("calculate_comfort_index", |params| async move {
                let temp = params
                    .get("temperature")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(20);
                let humidity = params
                    .get("humidity")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(50);

                let comfort_index = if temp >= 18 && temp <= 24 && humidity >= 40 && humidity <= 70
                {
                    "comfortable"
                } else {
                    "uncomfortable"
                };

                Ok(json!({
                    "comfort_index": comfort_index,
                    "temperature": temp,
                    "humidity": humidity
                }))
            })
            .register()
            .expect("Should register calculation skill");

        let agent_card = a2a::agent_card(&agent);
        assert_eq!(agent_card.name, "weather-calculator-agent");
        assert_eq!(agent_card.version.as_deref(), Some("1.0.0"));

        let skills = agent.list_skills();
        assert_eq!(skills.len(), 2);
        assert!(skills.contains(&"get_weather".to_string()));
        assert!(skills.contains(&"calculate_comfort_index".to_string()));
    }
}

#[cfg(test)]
mod unit_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_agent_config_validation() {
        // Test valid configuration
        let valid_config = AgentConfig {
            name: "test-agent".to_string(),
            description: "Test agent for validation".to_string(),
            version: "1.0.0".to_string(),
            storage_prefix: None,
            max_message_size: 1_048_576,
            streaming: false,
            batch_processing: false,
            concurrent_tasks: Some(10),
            stateless_methods: true,
            base_url: None,
            history_policy: None,
        };

        let agent_result = AgentRuntime::new_with_config(valid_config);
        assert!(agent_result.is_ok());

        // Test invalid configuration - empty name
        let invalid_config = AgentConfig {
            name: "".to_string(),
            ..Default::default()
        };

        let invalid_agent = Agent::new_with_config(invalid_config);
        assert!(invalid_agent.is_err());
    }

    #[test]
    fn test_message_type_classification() {
        // Test message type enum values
        assert_eq!(MessageType::Text, MessageType::Text);
        assert_eq!(MessageType::Data, MessageType::Data);
        assert_eq!(MessageType::Mixed, MessageType::Mixed);

        // Test that variants are different
        assert_ne!(MessageType::Text, MessageType::Data);
        assert_ne!(MessageType::Data, MessageType::Mixed);
    }

    #[test]
    fn test_agent_card_generation() {
        let mut agent = AgentRuntime::new_runtime("card-test-agent").unwrap();

        agent
            .skill("test_skill", |params| async move {
                let input = params
                    .get("input")
                    .and_then(|v| v.as_str())
                    .unwrap_or("default");
                Ok(json!({"result": format!("Processed: {}", input)}))
            })
            .register()
            .unwrap();

        let agent_card = a2a::agent_card(&agent);

        assert_eq!(agent_card.name, "card-test-agent");
        assert_eq!(agent_card.version.as_deref(), Some("1.0.0"));

        let skills = agent.list_skills();
        assert_eq!(skills.len(), 1);
        assert!(skills.contains(&"test_skill".to_string()));
    }

    #[tokio::test]
    async fn test_skill_execution() {
        let mut agent = AgentRuntime::new_runtime("exec-test-agent").unwrap();

        agent
            .skill("calculate", |params| async move {
                let a = params.get("a").and_then(|v| v.as_i64()).unwrap_or(0);
                let b = params.get("b").and_then(|v| v.as_i64()).unwrap_or(0);
                Ok(json!({"sum": a + b}))
            })
            .register()
            .unwrap();

        let skills = agent.list_skills();
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0], "calculate");
    }
}
