//! Agent Configuration Module
//!
//! This module provides configuration types and patterns for agent behavior,
//! capabilities, and communication patterns following clean architecture principles.

/// History preparation mode for durable continuation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HistoryPolicyMode {
    PassThrough,
    HistoryManager,
}

impl Default for HistoryPolicyMode {
    fn default() -> Self {
        Self::PassThrough
    }
}

/// Strategy name exposed through SDK config without leaking `llm_context_core` types.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HistoryStrategyKind {
    SlidingWindow,
    SlidingWindowWithSummary,
    PriorityBased,
}

impl Default for HistoryStrategyKind {
    fn default() -> Self {
        Self::SlidingWindow
    }
}

/// Agent-level history policy configuration.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct HistoryPolicyConfig {
    #[serde(default)]
    pub mode: HistoryPolicyMode,
    #[serde(default)]
    pub strategy: HistoryStrategyKind,
    #[serde(default = "default_context_window_tokens")]
    pub context_window_tokens: u32,
    #[serde(default = "default_max_output_tokens")]
    pub max_output_tokens: u32,
    #[serde(default)]
    pub enable_summarization: bool,
    #[serde(default)]
    pub enable_long_term_memory: bool,
    #[serde(default = "default_history_recall_top_k")]
    pub recall_top_k: usize,
    #[serde(default = "default_history_memory_token_budget")]
    pub memory_token_budget: u32,
}

const fn default_context_window_tokens() -> u32 {
    128_000
}

const fn default_max_output_tokens() -> u32 {
    16_384
}

const fn default_history_recall_top_k() -> usize {
    5
}

const fn default_history_memory_token_budget() -> u32 {
    2_000
}

impl Default for HistoryPolicyConfig {
    fn default() -> Self {
        Self {
            mode: HistoryPolicyMode::PassThrough,
            strategy: HistoryStrategyKind::SlidingWindow,
            context_window_tokens: default_context_window_tokens(),
            max_output_tokens: default_max_output_tokens(),
            enable_summarization: false,
            enable_long_term_memory: false,
            recall_top_k: default_history_recall_top_k(),
            memory_token_budget: default_history_memory_token_budget(),
        }
    }
}

/// **Agent Configuration**
///
/// Configuration for agent behavior, capabilities, and response patterns.
///
/// # Response Modes
///
/// The agent supports different response patterns:
///
/// - **Stateful (default)**: Creates Task responses for conversation continuity
/// - **Stateless**: Prefers lightweight Message responses for API-style interactions
///
/// # Examples
///
/// ```rust
/// use agent_sdk::agent::AgentConfig;
///
/// // Default stateful agent
/// let config = AgentConfig::default();
///
/// // Stateless agent for API-style interactions
/// let config = AgentConfig::new("api-agent", "API Agent")
///     .stateless();
/// ```
#[derive(Debug, Clone)]
pub struct AgentConfig {
    pub name: String,
    pub description: String,
    pub version: String,
    /// Optional storage namespace prefix used by shared task-storage backends.
    /// When absent, the SDK derives a local prefix from the agent name.
    pub storage_prefix: Option<String>,
    pub max_message_size: u64,
    pub streaming: bool,
    pub batch_processing: bool,
    pub concurrent_tasks: Option<u32>,
    /// Prefer stateless responses (Message) over stateful (Task) for methods
    /// Default: false (stateful - creates Tasks for conversation continuity)
    /// When true: prefers lightweight Message responses for API-style interactions
    pub stateless_methods: bool,
    /// Base URL for constructing absolute `AgentInterface` URLs in the agent card.
    /// When set, `/jsonrpc` becomes `{base_url}/jsonrpc`.
    /// When `None`, the interface URL remains a relative path `/jsonrpc`.
    pub base_url: Option<String>,
    /// Optional history policy used for durable continuation preparation.
    pub history_policy: Option<HistoryPolicyConfig>,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            name: "sdk-agent".to_string(),
            description: "Agent built with SDK".to_string(),
            version: "1.0.0".to_string(),
            storage_prefix: None,
            max_message_size: 1_048_576, // 1MB
            streaming: false,
            batch_processing: false,
            concurrent_tasks: Some(10),
            stateless_methods: false,
            base_url: None,
            history_policy: None,
        }
    }
}

impl AgentConfig {
    /// Create a new agent configuration
    ///
    /// # Arguments
    ///
    /// * `name` - Agent name
    /// * `description` - Agent description
    ///
    /// # Examples
    ///
    /// ```rust
    /// use agent_sdk::agent::AgentConfig;
    ///
    /// let config = AgentConfig::new("my-agent", "My Agent Description");
    /// ```
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            ..Default::default()
        }
    }

    /// Configure agent for stateless method responses
    ///
    /// When enabled, methods will prefer lightweight Message responses
    /// over Task responses for API-style interactions.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use agent_sdk::agent::AgentConfig;
    ///
    /// let config = AgentConfig::new("api-agent", "API Agent")
    ///     .stateless();
    /// ```
    pub fn stateless(mut self) -> Self {
        self.stateless_methods = true;
        self
    }

    /// Configure agent for stateful method responses (default)
    ///
    /// Methods will create Task responses for conversation continuity.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use agent_sdk::agent::AgentConfig;
    ///
    /// let config = AgentConfig::default()
    ///     .stateful(); // Explicit stateful (default behavior)
    /// ```
    pub fn stateful(mut self) -> Self {
        self.stateless_methods = false;
        self
    }

    /// Set the base URL for absolute `AgentInterface` URLs in the agent card.
    ///
    /// The A2A v1.0 spec requires absolute URLs in `AgentInterface.url`.
    /// When set, the SDK produces `{base_url}/jsonrpc` instead of `/jsonrpc`.
    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = Some(url.into());
        self
    }

    /// Attach a history policy for pre/post-turn continuation preparation.
    pub fn with_history_policy(mut self, policy: HistoryPolicyConfig) -> Self {
        self.history_policy = Some(policy);
        self
    }
}
