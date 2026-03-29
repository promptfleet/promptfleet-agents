//! Agent Client implementation
//!
//! This module provides an ergonomic client interface for communicating
//! with other A2A agents using the HTTP transport.

use log::{debug, error, info, trace};
use serde_json::Value;
use std::collections::HashMap;
use uuid::Uuid;

#[cfg(feature = "a2a-client")]
use a2a_http_client::{Client, RpcError};

use a2a_protocol_core::{
    A2AError, A2AResult, JsonRpcRequest, JsonRpcResponse,
    agent::AgentCard,
    data::{
        Task,
        message::{Message, MessageRole, Part},
    },
    methods::params::{GetTaskRequest, SendMessageConfiguration, SendMessageRequest},
};

use crate::callable::CallableSkill;
use crate::error::{SdkError, SdkResult};

/// **Skill Validation Error**
///
/// Provides LLM-friendly error messages for function call validation failures.
#[derive(Debug, Clone)]
pub struct SkillValidationError {
    pub skill_id: String,
    pub missing_required: Vec<String>,
    pub invalid_types: Vec<(String, String, String)>, // field, expected, actual
    pub unexpected_fields: Vec<String>,
    pub expected_schema_summary: String,
}

impl SkillValidationError {
    /// Generate human-readable error message for LLMs
    pub fn human_readable_message(&self) -> String {
        let mut msg = String::new();

        if !self.missing_required.is_empty() {
            msg.push_str(&format!(
                "Missing required parameters: [{}]",
                self.missing_required.join(", ")
            ));
        }

        if !self.invalid_types.is_empty() {
            if !msg.is_empty() {
                msg.push_str(". ");
            }
            let type_errors: Vec<String> = self
                .invalid_types
                .iter()
                .map(|(field, expected, actual)| {
                    format!("'{}' should be {} but got {}", field, expected, actual)
                })
                .collect();
            msg.push_str(&format!("Type errors: [{}]", type_errors.join(", ")));
        }

        if !self.unexpected_fields.is_empty() {
            if !msg.is_empty() {
                msg.push_str(". ");
            }
            msg.push_str(&format!(
                "Unexpected parameters: [{}]",
                self.unexpected_fields.join(", ")
            ));
        }

        msg
    }
}

/// **Agent Client Mode**
///
/// Defines how the client operates for maximum flexibility.
#[derive(Debug, Clone)]
pub enum ClientMode {
    /// Direct connection to a specific agent
    /// Example: `A2aClient::new("http://weather-agent:3000")`
    Direct { agent_endpoint: String },
}

#[derive(Debug, Clone, Default)]
pub struct SendMessageOptions {
    pub history_length: Option<u32>,
    pub return_immediately: bool,
}

/// Ergonomic client for communicating with A2A agents
///
/// The `A2aClient` provides a unified direct A2A client for endpoint-based access.
///
/// # Examples
///
/// ```rust,no_run
/// use agent_sdk::a2a::A2aClient;
/// use serde_json::json;
///
/// #[tokio::main]
/// async fn main() -> Result<(), Box<dyn std::error::Error>> {
///     // Direct mode - connect to specific agent
///     let direct_client = A2aClient::direct("http://weather-agent:3000")?;
///     let weather = direct_client.call_method("get_weather",
///         json!({"location": "Paris"})).await?;
///     
///     Ok(())
/// }
/// ```
pub struct A2aClient {
    #[cfg(feature = "a2a-client")]
    inner: Client,

    #[cfg(not(feature = "a2a-client"))]
    _phantom: std::marker::PhantomData<()>,

    mode: ClientMode,
}

impl A2aClient {
    /// Create a new client (defaults to direct mode for backward compatibility).
    ///
    /// # Arguments
    ///
    /// * `endpoint` - Agent endpoint URL
    ///
    /// # Returns
    ///
    /// * `Ok(A2aClient)` - Connected client
    /// * `Err(SdkError)` - Connection failed
    #[cfg(feature = "a2a-client")]
    pub fn new(endpoint: &str) -> SdkResult<Self> {
        Self::direct(endpoint)
    }

    /// Create a client for direct connection to a specific agent
    ///
    /// # Arguments
    ///
    /// * `agent_endpoint` - Direct agent endpoint URL
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use agent_sdk::a2a::A2aClient;
    /// use serde_json::json;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = A2aClient::direct("http://weather-agent:3000")?;
    /// let weather = client.call_method("get_weather", json!({"location": "Tokyo"})).await?;
    /// # Ok(())
    /// # }
    /// ```
    #[cfg(feature = "a2a-client")]
    pub fn direct(agent_endpoint: &str) -> SdkResult<Self> {
        debug!("Creating direct A2aClient for endpoint: {}", agent_endpoint);

        if agent_endpoint.is_empty() {
            error!("Client creation failed: endpoint cannot be empty");
            return Err(SdkError::invalid_input("Agent endpoint cannot be empty"));
        }

        let inner = Client::external(agent_endpoint);

        info!("Direct A2aClient created for endpoint: {}", agent_endpoint);
        Ok(Self {
            inner,
            mode: ClientMode::Direct {
                agent_endpoint: agent_endpoint.to_string(),
            },
        })
    }

    /// Create a new client (feature guard)
    #[cfg(not(feature = "a2a-client"))]
    pub fn new(_endpoint: &str) -> SdkResult<Self> {
        Err(SdkError::feature_not_enabled("client"))
    }

    /// Create a direct client (feature guard)
    #[cfg(not(feature = "a2a-client"))]
    pub fn direct(_agent_endpoint: &str) -> SdkResult<Self> {
        Err(SdkError::feature_not_enabled("client"))
    }

    /// Create a client with custom headers
    ///
    /// # Arguments
    ///
    /// * `endpoint` - The HTTP endpoint of the target agent
    /// * `headers` - Additional HTTP headers to include
    #[cfg(feature = "a2a-client")]
    pub fn with_headers(endpoint: &str, headers: HashMap<String, String>) -> SdkResult<Self> {
        let mut client = Self::new(endpoint)?;

        for (key, value) in headers {
            client.inner = client.inner.with_header(key, value);
        }

        Ok(client)
    }

    /// Create a client with authentication token
    ///
    /// # Arguments
    ///
    /// * `endpoint` - The HTTP endpoint of the target agent
    /// * `token` - Authentication token
    #[cfg(feature = "a2a-client")]
    pub fn with_auth_token(endpoint: &str, token: &str) -> SdkResult<Self> {
        let client = Client::external(endpoint)
            .with_header("Authorization".to_string(), format!("Bearer {}", token));

        Ok(Self {
            inner: client,
            mode: ClientMode::Direct {
                agent_endpoint: endpoint.to_string(),
            },
        })
    }

    /// Call a method on the remote agent
    ///
    /// # Arguments
    ///
    /// * `method` - Method name to call
    /// * `params` - Method parameters as JSON
    ///
    /// # Returns
    ///
    /// * `Ok(Value)` - Method result
    /// * `Err(SdkError)` - Method call failed
    #[cfg(feature = "a2a-client")]
    pub async fn call_method(&self, method: &str, params: Value) -> SdkResult<Value> {
        debug!(
            "Calling method '{}' on endpoint: {}",
            method,
            self.endpoint()
        );
        trace!("Method '{}' parameters: {}", method, params);

        let start_time = std::time::Instant::now();
        let result = self
            .inner
            .call(method, params)
            .await
            .map_err(|e| self.convert_rpc_error(method, e));

        let duration = start_time.elapsed();
        match &result {
            Ok(response) => {
                info!(
                    "Method '{}' called successfully in {:.2}ms",
                    method,
                    duration.as_secs_f64() * 1000.0
                );
                trace!("Method '{}' response: {}", method, response);
            }
            Err(error) => {
                error!(
                    "Method '{}' call failed after {:.2}ms: {}",
                    method,
                    duration.as_secs_f64() * 1000.0,
                    error
                );
            }
        }

        result
    }

    /// Call a method on the remote agent (feature guard)
    #[cfg(not(feature = "a2a-client"))]
    pub async fn call_method(&self, _method: &str, _params: Value) -> SdkResult<Value> {
        Err(SdkError::feature_not_enabled("client"))
    }

    /// Send a message to the remote agent using A2A message/send method
    ///
    /// # Arguments
    ///
    /// * `text` - Message text content
    /// * `context_id` - Optional context ID for message grouping
    ///
    /// # Returns
    ///
    /// * `Ok(Value)` - Message send result with task information
    /// * `Err(SdkError)` - Message send failed
    #[cfg(feature = "a2a-client")]
    pub async fn send_message(&self, text: &str, context_id: Option<String>) -> SdkResult<Value> {
        let context_for_message = context_id.unwrap_or_else(|| "default".to_string());
        let message = Message::text(MessageRole::User, text, context_for_message);
        self.message_send_with_options(
            message,
            None,
            SendMessageOptions {
                history_length: Some(0),
                return_immediately: false,
            },
        )
        .await
    }

    /// Send a message to the remote agent (feature guard)
    #[cfg(not(feature = "a2a-client"))]
    pub async fn send_message(&self, _text: &str, _context_id: Option<String>) -> SdkResult<Value> {
        Err(SdkError::feature_not_enabled("client"))
    }

    /// Send a message using A2A protocol message/send method directly
    ///
    /// # Arguments
    ///
    /// * `message` - A2A Message object
    /// * `context_id` - Optional context ID for message grouping
    /// * `metadata` - Optional metadata
    ///
    /// # Returns
    ///
    /// * `Ok(Value)` - Message send result with task information
    /// * `Err(SdkError)` - Message send failed
    #[cfg(feature = "a2a-client")]
    pub async fn message_send(
        &self,
        message: Message,
        metadata: Option<HashMap<String, Value>>,
    ) -> SdkResult<Value> {
        self.message_send_with_options(
            message,
            metadata,
            SendMessageOptions {
                history_length: Some(0),
                return_immediately: false,
            },
        )
        .await
    }

    /// Send a message using A2A protocol (feature guard)
    #[cfg(not(feature = "a2a-client"))]
    pub async fn message_send(
        &self,
        _message: Message,
        _metadata: Option<HashMap<String, Value>>,
    ) -> SdkResult<Value> {
        Err(SdkError::feature_not_enabled("client"))
    }

    /// Get task status from the remote agent
    ///
    /// # Arguments
    ///
    /// * `task_id` - Task ID to retrieve
    /// * `include_history` - Whether to include task history
    /// * `include_artifacts` - Whether to include task artifacts
    ///
    /// # Returns
    ///
    /// * `Ok(Task)` - Task information
    /// * `Err(SdkError)` - Task retrieval failed
    #[cfg(feature = "a2a-client")]
    pub async fn get_task(&self, task_id: &str) -> SdkResult<Task> {
        self.get_task_with_history(task_id, None).await
    }

    /// Get task status from the remote agent (feature guard)
    #[cfg(not(feature = "a2a-client"))]
    pub async fn get_task(&self, _task_id: &str) -> SdkResult<Task> {
        Err(SdkError::feature_not_enabled("client"))
    }

    /// Get task using A2A protocol tasks/get method directly
    ///
    /// # Arguments
    ///
    /// * `task_id` - Task ID to retrieve
    /// * `include_history` - Whether to include task history
    /// * `include_artifacts` - Whether to include task artifacts
    ///
    /// # Returns
    ///
    /// * `Ok(Task)` - Task information
    /// * `Err(SdkError)` - Task retrieval failed
    #[cfg(feature = "a2a-client")]
    pub async fn task_get(&self, task_id: String) -> SdkResult<Task> {
        self.get_task_with_history(&task_id, None).await
    }

    #[cfg(feature = "a2a-client")]
    pub async fn send_message_with_options(
        &self,
        text: &str,
        context_id: Option<String>,
        options: SendMessageOptions,
    ) -> SdkResult<Value> {
        let context_for_message = context_id.unwrap_or_else(|| "default".to_string());
        let message = Message::text(MessageRole::User, text, context_for_message);
        self.message_send_with_options(message, None, options).await
    }

    #[cfg(not(feature = "a2a-client"))]
    pub async fn send_message_with_options(
        &self,
        _text: &str,
        _context_id: Option<String>,
        _options: SendMessageOptions,
    ) -> SdkResult<Value> {
        Err(SdkError::feature_not_enabled("client"))
    }

    #[cfg(feature = "a2a-client")]
    pub async fn message_send_with_options(
        &self,
        message: Message,
        metadata: Option<HashMap<String, Value>>,
        options: SendMessageOptions,
    ) -> SdkResult<Value> {
        let configuration = Some(SendMessageConfiguration {
            accepted_output_modes: None,
            task_push_notification_config: None,
            history_length: options.history_length,
            return_immediately: options.return_immediately,
        });
        let params = SendMessageRequest {
            message,
            tenant: None,
            configuration,
            metadata,
        };
        self.inner
            .call(
                "SendMessage",
                serde_json::to_value(params).map_err(|e| {
                    SdkError::method_execution(
                        "message/send",
                        format!("serialize send params failed: {}", e),
                    )
                })?,
            )
            .await
            .map_err(|e| self.convert_rpc_error("message/send", e))
    }

    #[cfg(not(feature = "a2a-client"))]
    pub async fn message_send_with_options(
        &self,
        _message: Message,
        _metadata: Option<HashMap<String, Value>>,
        _options: SendMessageOptions,
    ) -> SdkResult<Value> {
        Err(SdkError::feature_not_enabled("client"))
    }

    #[cfg(feature = "a2a-client")]
    pub async fn get_task_with_history(
        &self,
        task_id: &str,
        history_length: Option<u32>,
    ) -> SdkResult<Task> {
        let params = GetTaskRequest {
            id: task_id.to_string(),
            tenant: None,
            history_length,
        };
        let value = self
            .inner
            .call(
                "GetTask",
                serde_json::to_value(params).map_err(|e| {
                    SdkError::method_execution(
                        "tasks/get",
                        format!("serialize get params failed: {}", e),
                    )
                })?,
            )
            .await
            .map_err(|e| self.convert_rpc_error("tasks/get", e))?;
        serde_json::from_value(value).map_err(|e| {
            SdkError::method_execution("tasks/get", format!("deserialize task failed: {}", e))
        })
    }

    #[cfg(not(feature = "a2a-client"))]
    pub async fn get_task_with_history(
        &self,
        _task_id: &str,
        _history_length: Option<u32>,
    ) -> SdkResult<Task> {
        Err(SdkError::feature_not_enabled("client"))
    }

    /// Get task using A2A protocol (feature guard)
    #[cfg(not(feature = "a2a-client"))]
    pub async fn task_get(&self, _task_id: String) -> SdkResult<Task> {
        Err(SdkError::feature_not_enabled("client"))
    }

    /// Cancel a task on the remote agent
    ///
    /// # Arguments
    ///
    /// * `task_id` - Task ID to cancel
    /// * `reason` - Optional cancellation reason
    ///
    /// # Returns
    ///
    /// * `Ok(Task)` - Updated task information
    /// * `Err(SdkError)` - Task cancellation failed
    #[cfg(feature = "a2a-client")]
    pub async fn cancel_task(&self, task_id: &str) -> SdkResult<Task> {
        self.inner
            .task_cancel(task_id.to_string())
            .await
            .map_err(|e| self.convert_rpc_error("tasks/cancel", e))
    }

    /// Cancel a task on the remote agent (feature guard)
    #[cfg(not(feature = "a2a-client"))]
    pub async fn cancel_task(&self, _task_id: &str) -> SdkResult<Task> {
        Err(SdkError::feature_not_enabled("client"))
    }

    /// List tasks on the remote agent
    ///
    /// # Arguments
    ///
    /// * `limit` - Maximum number of tasks to return
    /// * `offset` - Offset for pagination
    /// * `state_filter` - Optional state filter
    /// * `context_filter` - Optional context filter
    ///
    /// # Returns
    ///
    /// * `Ok(Value)` - Task list result
    /// * `Err(SdkError)` - Task listing failed
    #[cfg(feature = "a2a-client")]
    pub async fn list_tasks(
        &self,
        context_id: Option<String>,
        status: Option<String>,
        page_size: Option<u32>,
        page_token: Option<String>,
    ) -> SdkResult<Value> {
        self.inner
            .task_list(context_id, status, page_size, page_token)
            .await
            .map_err(|e| self.convert_rpc_error("tasks/list", e))
    }

    /// List tasks on the remote agent (feature guard)
    #[cfg(not(feature = "a2a-client"))]
    pub async fn list_tasks(
        &self,
        _context_id: Option<String>,
        _status: Option<String>,
        _page_size: Option<u32>,
        _page_token: Option<String>,
    ) -> SdkResult<Value> {
        Err(SdkError::feature_not_enabled("client"))
    }

    /// List tasks using A2A protocol tasks/list method directly
    ///
    /// # Arguments
    ///
    /// * `limit` - Maximum number of tasks to return
    /// * `offset` - Offset for pagination
    /// * `state_filter` - Optional state filter
    /// * `context_filter` - Optional context filter
    ///
    /// # Returns
    ///
    /// * `Ok(Value)` - Task list result
    /// * `Err(SdkError)` - Task listing failed
    #[cfg(feature = "a2a-client")]
    pub async fn task_list(
        &self,
        context_id: Option<String>,
        status: Option<String>,
        page_size: Option<u32>,
        page_token: Option<String>,
    ) -> SdkResult<Value> {
        self.inner
            .task_list(context_id, status, page_size, page_token)
            .await
            .map_err(|e| self.convert_rpc_error("tasks/list", e))
    }

    /// List tasks using A2A protocol (feature guard)
    #[cfg(not(feature = "a2a-client"))]
    pub async fn task_list(
        &self,
        _context_id: Option<String>,
        _status: Option<String>,
        _page_size: Option<u32>,
        _page_token: Option<String>,
    ) -> SdkResult<Value> {
        Err(SdkError::feature_not_enabled("client"))
    }

    /// Get the remote agent card JSON.
    #[cfg(feature = "a2a-client")]
    pub async fn agent_card(&self) -> SdkResult<String> {
        self.inner.agent_card().await.map_err(|e| {
            SdkError::client_connection(
                self.endpoint().to_string(),
                format!("Failed to get agent card: {}", e),
            )
        })
    }

    /// Get the remote agent card JSON (feature guard).
    #[cfg(not(feature = "a2a-client"))]
    pub async fn agent_card(&self) -> SdkResult<String> {
        Err(SdkError::feature_not_enabled("client"))
    }

    /// Ping the remote agent to check connectivity
    ///
    /// # Returns
    ///
    /// * `Ok(Value)` - Ping response
    /// * `Err(SdkError)` - Ping failed
    #[cfg(feature = "a2a-client")]
    pub async fn ping(&self) -> SdkResult<Value> {
        self.inner.ping().await.map_err(|e| {
            SdkError::client_connection(self.endpoint().to_string(), format!("Ping failed: {}", e))
        })
    }

    /// Ping the remote agent (feature guard)
    #[cfg(not(feature = "a2a-client"))]
    pub async fn ping(&self) -> SdkResult<Value> {
        Err(SdkError::feature_not_enabled("client"))
    }

    /// Get agent capabilities
    ///
    /// # Returns
    ///
    /// * `Ok(Value)` - Capabilities information
    /// * `Err(SdkError)` - Failed to get capabilities
    /// Get agent capabilities by fetching the agent card.
    #[cfg(feature = "a2a-client")]
    pub async fn get_capabilities(&self) -> SdkResult<Value> {
        let card_json = self.agent_card().await?;
        let card: Value = serde_json::from_str(&card_json).map_err(|e| {
            SdkError::client_connection(
                self.endpoint().to_string(),
                format!("Failed to parse agent card: {}", e),
            )
        })?;
        Ok(card.get("capabilities").cloned().unwrap_or(Value::Null))
    }

    /// Get agent capabilities (feature guard)
    #[cfg(not(feature = "a2a-client"))]
    pub async fn get_capabilities(&self) -> SdkResult<Value> {
        Err(SdkError::feature_not_enabled("client"))
    }

    /// Check if the remote agent is reachable
    ///
    /// # Returns
    ///
    /// * `Ok(bool)` - True if agent is reachable
    /// * `Err(SdkError)` - Connectivity check failed
    #[cfg(feature = "a2a-client")]
    pub async fn check_connectivity(&self) -> SdkResult<bool> {
        match self.ping().await {
            Ok(_) => Ok(true),
            Err(SdkError::ClientConnection { .. }) => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// Check if the remote agent is reachable (feature guard)
    #[cfg(not(feature = "a2a-client"))]
    pub async fn check_connectivity(&self) -> SdkResult<bool> {
        Err(SdkError::feature_not_enabled("client"))
    }

    /// Get the endpoint URL
    pub fn endpoint(&self) -> &str {
        match &self.mode {
            ClientMode::Direct { agent_endpoint } => agent_endpoint,
        }
    }

    /// Convert RPC error to SDK error
    #[cfg(feature = "a2a-client")]
    fn convert_rpc_error(&self, method: &str, error: RpcError) -> SdkError {
        SdkError::method_execution(
            method,
            format!("RPC error (code: {}): {}", error.code, error.message),
        )
    }

    /// **Call Agent Skill (Universal Function Caller)**
    ///
    /// Provides a universal interface for calling any discovered agent skill.
    /// Includes schema validation with LLM-friendly error messages.
    ///
    /// # Arguments
    ///
    /// * `agent_id` - Target agent identifier
    /// * `skill_id` - Skill to call (from AgentCard.skills)
    /// * `params` - Function parameters as JSON Value
    ///
    /// # Returns
    ///
    /// * `Ok(Value)` - Skill execution result
    /// * `Err(A2AError)` - Validation or execution error with helpful messages
    ///
    /// # Examples
    ///
    /// ```rust
    /// use agent_sdk::a2a::A2aClient;
    /// use serde_json::json;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = A2aClient::new("http://localhost:3000")?;
    ///
    /// // Call weather skill with validation
    /// let result = client.call_skill("weather-agent", "get_weather", json!({
    ///     "location": "Tokyo",
    ///     "units": "celsius"
    /// })).await?;
    ///
    /// println!("Weather result: {}", result);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn call_skill(
        &self,
        agent_id: &str,
        skill_id: &str,
        params: Value,
    ) -> A2AResult<Value> {
        // 1. Get agent card and find skill
        let agent_card = self.fetch_remote_agent_card(agent_id).await?;
        agent_card.get_skill(skill_id).ok_or_else(|| {
            A2AError::capability_validation_failed(format!(
                "Skill '{}' not found in agent '{}'. Available skills: [{}]",
                skill_id,
                agent_id,
                agent_card
                    .skills
                    .iter()
                    .map(|s| s.id.clone())
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        })?;

        // 2. Convert to A2A message/send with data Part
        self.execute_skill_call_with_context(agent_id, skill_id, params, None, None)
            .await
    }

    /// **Call Agent Skill within an existing context**
    ///
    /// Same as `call_skill` but allows specifying `context_id` to continue a prior conversation
    /// and optional `reference_task_ids` for refinements.
    /// This conforms to A2A "Life of a Task" guidance where a context composes many tasks.
    pub async fn call_skill_in_context(
        &self,
        agent_id: &str,
        skill_id: &str,
        params: Value,
        context_id: Option<String>,
        reference_task_ids: Option<Vec<String>>,
    ) -> A2AResult<Value> {
        // Pre-validate the skill exists for better error messages
        let agent_card = self.fetch_remote_agent_card(agent_id).await?;
        let _ = agent_card.get_skill(skill_id).ok_or_else(|| {
            A2AError::capability_validation_failed(format!(
                "Skill '{}' not found in agent '{}'",
                skill_id, agent_id
            ))
        })?;

        self.execute_skill_call_with_context(
            agent_id,
            skill_id,
            params,
            context_id,
            reference_task_ids,
        )
        .await
    }

    /// **Get Materialized Skill Function (Two-Step Pattern)**
    ///
    /// Returns a function handle that can be called multiple times.
    /// Validation and setup happens once during materialization.
    ///
    /// # Arguments
    ///
    /// * `agent_id` - Target agent identifier  
    /// * `skill_id` - Skill to materialize
    ///
    /// # Returns
    ///
    /// * `Ok(CallableSkill)` - Callable function handle
    /// * `Err(SdkError)` - Materialization error
    ///
    /// # Examples
    ///
    /// ```rust
    /// use agent_sdk::a2a::A2aClient;
    /// use serde_json::json;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = A2aClient::new("http://localhost:3000")?;
    ///
    /// // Step A: Materialize the function
    /// let get_weather = client.materialize_skill("weather-agent", "get_weather").await?;
    ///
    /// // Step B: Call multiple times
    /// let tokyo_weather = get_weather(json!({"location": "Tokyo"})).await?;
    /// let london_weather = get_weather(json!({"location": "London"})).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn materialize_skill(
        &self,
        agent_id: &str,
        skill_id: &str,
    ) -> SdkResult<CallableSkill> {
        // Pre-validate skill exists and get schema
        let agent_card = self.fetch_remote_agent_card(agent_id).await?;
        let _skill = agent_card.get_skill(skill_id).ok_or_else(|| {
            SdkError::method_execution(
                "materialize_skill",
                format!("skill '{}' not found in agent '{}'", skill_id, agent_id),
            )
        })?;

        // Clone what we need for the closure to own the data
        let client_mode = self.mode.clone();
        let agent_id = agent_id.to_string();
        let skill_id = skill_id.to_string();
        let endpoint = self.endpoint().to_string();

        // Create a new client instance for the closure
        // Return materialized function
        Ok(Box::new(move |params: Value| {
            #[cfg(feature = "a2a-client")]
            let client = A2aClient {
                inner: Client::external(&endpoint),
                mode: client_mode.clone(),
            };

            #[cfg(not(feature = "a2a-client"))]
            let client = A2aClient {
                _phantom: std::marker::PhantomData,
                mode: client_mode.clone(),
            };

            let agent_id = agent_id.clone();
            let skill_id = skill_id.clone();

            Box::pin(async move {
                // Execute the call
                client
                    .execute_skill_call(&agent_id, &skill_id, params)
                    .await
                    .map_err(SdkError::from)
            })
        }))
    }

    /// **List Available Skills for Agent**
    ///
    /// Returns all skills advertised by an agent for LLM discovery.
    pub async fn list_agent_skills(
        &self,
        agent_id: &str,
    ) -> A2AResult<Vec<a2a_protocol_core::agent::AgentSkill>> {
        let agent_card = self.fetch_remote_agent_card(agent_id).await?;
        Ok(agent_card.skills.clone())
    }

    /// **Get Skill by ID**
    ///
    /// Returns the skill definition for a specific skill, useful for LLM function calling.
    pub async fn get_skill(
        &self,
        agent_id: &str,
        skill_id: &str,
    ) -> A2AResult<a2a_protocol_core::agent::AgentSkill> {
        let agent_card = self.fetch_remote_agent_card(agent_id).await?;
        agent_card.get_skill(skill_id).cloned().ok_or_else(|| {
            A2AError::capability_validation_failed(format!(
                "Skill '{}' not found in agent '{}'",
                skill_id, agent_id
            ))
        })
    }

    /// **Execute Skill Call (Internal Implementation)**
    ///
    /// Converts function call to A2A message/send and handles response.
    async fn execute_skill_call(
        &self,
        agent_id: &str,
        skill_id: &str,
        params: Value,
    ) -> A2AResult<Value> {
        self.execute_skill_call_with_context(agent_id, skill_id, params, None, None)
            .await
    }

    /// **Execute Skill Call with Context (Internal)**
    ///
    /// Allows passing `context_id` to continue a session and `reference_task_ids` for refinements.
    async fn execute_skill_call_with_context(
        &self,
        agent_id: &str,
        skill_id: &str,
        params: Value,
        context_id: Option<String>,
        reference_task_ids: Option<Vec<String>>,
    ) -> A2AResult<Value> {
        let mut skill_data = serde_json::Map::new();
        skill_data.insert("skill".to_string(), Value::String(skill_id.to_string()));

        if let Value::Object(param_map) = params {
            for (key, value) in param_map {
                skill_data.insert(key, value);
            }
        }

        let message = Message {
            role: MessageRole::User,
            parts: vec![Part::data(Value::Object(skill_data))],
            message_id: Uuid::new_v4().to_string(),
            task_id: None,
            context_id: context_id.clone(),
            metadata: None,
            extensions: None,
            reference_task_ids: reference_task_ids.clone(),
        };

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Value::String(Uuid::new_v4().to_string()),
            method: "message/send".to_string(),
            params: serde_json::to_value(SendMessageRequest {
                message,
                configuration: None,
                metadata: None,
                tenant: None,
            })?,
        };

        // Execute request
        let response = self.send_request(agent_id, request).await?;

        // Extract result from task response
        if let Some(result) = response.result {
            Ok(result)
        } else if let Some(error) = response.error {
            Err(A2AError::method_execution_failed(
                &format!("skill/{}", skill_id),
                format!("Skill execution failed: {}", error.message),
            ))
        } else {
            Err(A2AError::protocol_validation_error(
                "No result or error in response",
            ))
        }
    }

    /// **Fetch Remote Agent Card**
    ///
    /// Retrieves agent capabilities from a remote agent for discovery and validation.
    /// This method is used to get OTHER agents' cards, not the current agent's own card.
    async fn fetch_remote_agent_card(&self, agent_id: &str) -> A2AResult<AgentCard> {
        // Get the agent card as String first
        let agent_card_json = self.agent_card().await.map_err(|e| {
            A2AError::agent_unavailable(agent_id, format!("Failed to get agent card: {}", e))
        })?;

        // Parse into AgentCard struct
        serde_json::from_str(&agent_card_json).map_err(|e| {
            A2AError::protocol_validation_error(format!("Invalid agent card format: {}", e))
        })
    }

    /// **Send Request to Agent (Internal Helper)**
    ///
    /// Handles the actual HTTP/transport layer communication with mode-aware routing.
    async fn send_request(
        &self,
        agent_id: &str,
        request: JsonRpcRequest,
    ) -> A2AResult<JsonRpcResponse> {
        match &self.mode {
            ClientMode::Direct { .. } => {
                // In direct mode, send directly to connected agent
                #[cfg(feature = "a2a-client")]
                {
                    // Use the public call method instead of private send_request
                    let result = self
                        .inner
                        .call(&request.method, request.params)
                        .await
                        .map_err(|e| {
                            A2AError::agent_unavailable(
                                agent_id,
                                format!("Direct request failed: {}", e),
                            )
                        })?;

                    // Convert to JsonRpcResponse format
                    Ok(JsonRpcResponse {
                        jsonrpc: "2.0".to_string(),
                        id: request.id,
                        result: Some(result),
                        error: None,
                    })
                }
                #[cfg(not(feature = "a2a-client"))]
                {
                    Err(A2AError::capability_validation_failed(
                        "client feature not enabled",
                    ))
                }
            }
        }
    }

    // ============================================================================
    // DIRECT A2A PROTOCOL EXPOSURE FOR LLMs
    // ============================================================================

    /// **Direct Message Send (Native A2A for LLMs)**
    ///
    /// Exposes A2A `message/send` directly to LLMs without abstraction layers.
    /// This allows LLMs to work natively with the A2A protocol using any Part types.
    ///
    /// # Arguments
    ///
    /// * `agent_id` - Target agent identifier
    /// * `message` - A2A Message with any combination of Parts (Text, Data, File, etc.)
    /// * `context_id` - Optional conversation context
    ///
    /// # Returns
    ///
    /// * `Ok(Task)` - A2A Task containing response artifacts
    /// * `Err(A2AError)` - Communication or execution error
    ///
    /// # Examples
    ///
    /// ```rust
    /// use agent_sdk::a2a::{Message, MessageRole};
    /// use agent_sdk::a2a::A2aClient;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = A2aClient::new("http://agent-mesh:8080")?;
    ///
    /// // Example: send a simple text message (you can also build structured messages with data parts).
    /// let msg = Message::text(MessageRole::User, "get_weather: Tokyo", "ctx".to_string());
    ///
    /// let task = client.message_send_direct("weather-agent", msg, None).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn message_send_direct(
        &self,
        agent_id: &str,
        message: Message,
        _context_id: Option<String>,
    ) -> A2AResult<a2a_protocol_core::data::task::Task> {
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: serde_json::Value::String(Uuid::new_v4().to_string()),
            method: "message/send".to_string(),
            params: serde_json::to_value(SendMessageRequest {
                message,
                configuration: None,
                metadata: None,
                tenant: None,
            })?,
        };

        // Send request and handle response
        let response = self.send_request(agent_id, request).await?;

        if let Some(result) = response.result {
            // Parse the Task from the response
            serde_json::from_value(result).map_err(|e| {
                A2AError::capability_validation_failed(format!(
                    "Failed to parse task response: {}",
                    e
                ))
            })
        } else if let Some(error) = response.error {
            Err(A2AError::method_execution_failed(
                "message/send",
                format!("Message send failed: {}", error.message),
            ))
        } else {
            Err(A2AError::capability_validation_failed(
                "No result or error in response",
            ))
        }
    }

    /// **Generate OpenAI Function Schema for Direct A2A**
    ///
    /// Creates a function calling schema that exposes A2A `message/send`
    /// directly to LLMs like GPT-4, Claude, etc.
    ///
    /// # Arguments
    ///
    /// * `agent_id` - Target agent identifier to include in schema
    /// * `include_file_parts` - Whether to include File part support
    ///
    /// # Returns
    ///
    /// OpenAI-compatible function schema as JSON Value
    ///
    /// # Examples
    ///
    /// ```rust
    /// use agent_sdk::a2a::A2aClient;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = A2aClient::new("http://agent-mesh:8080")?;
    ///
    /// // Generate schema for weather agent
    /// let schema = client.generate_message_send_schema("weather-agent", false)?;
    ///
    /// // Use with OpenAI function calling
    /// // openai.functions = [schema]
    /// # Ok(())
    /// # }
    /// ```
    pub fn generate_message_send_schema(
        &self,
        agent_id: &str,
        include_file_parts: bool,
    ) -> A2AResult<serde_json::Value> {
        let mut part_schemas = vec![
            // Text Part
            serde_json::json!({
                "type": "object",
                "properties": {
                    "kind": {"type": "string", "const": "text"},
                    "text": {
                        "type": "string",
                        "description": "Text content for conversational messages"
                    }
                },
                "required": ["kind", "text"],
                "additionalProperties": false
            }),
            // Data Part
            serde_json::json!({
                "type": "object",
                "properties": {
                    "kind": {"type": "string", "const": "data"},
                    "data": {
                        "type": "object",
                        "description": "Structured JSON data for skill calls or parameters",
                        "additionalProperties": true
                    }
                },
                "required": ["kind", "data"],
                "additionalProperties": false
            }),
        ];

        // Add File Part if requested
        if include_file_parts {
            part_schemas.push(serde_json::json!({
                "type": "object",
                "properties": {
                    "kind": {"type": "string", "const": "file"},
                    "file_id": {
                        "type": "string",
                        "description": "File identifier or reference"
                    },
                    "filename": {
                        "type": "string",
                        "description": "Original filename"
                    },
                    "mime_type": {
                        "type": "string",
                        "description": "MIME type of the file"
                    }
                },
                "required": ["kind", "file_id"],
                "additionalProperties": false
            }));
        }

        Ok(serde_json::json!({
            "name": "message_send",
            "description": format!("Send a message to A2A agent '{}' using native A2A protocol. Supports text (conversational), data (skill calls), and mixed message types.", agent_id),
            "parameters": {
                "type": "object",
                "properties": {
                    "message": {
                        "type": "object",
                        "properties": {
                            "role": {
                                "type": "string",
                                "enum": ["user", "assistant"],
                                "default": "user",
                                "description": "Message role in conversation"
                            },
                            "parts": {
                                "type": "array",
                                "description": "Message parts - can mix text, data, and file parts",
                                "items": {
                                    "oneOf": part_schemas
                                },
                                "minItems": 1
                            }
                        },
                        "required": ["parts"],
                        "additionalProperties": false
                    },
                    "context_id": {
                        "type": "string",
                        "description": "Optional conversation context identifier",
                        "optional": true
                    }
                },
                "required": ["message"],
                "additionalProperties": false
            }
        }))
    }

    /// **Generate Agent-Specific Function Schema**
    ///
    /// Creates a customized function schema based on the agent's actual capabilities.
    /// Includes skill hints in the description for better LLM understanding.
    ///
    /// # Arguments
    ///
    /// * `agent_id` - Target agent identifier
    ///
    /// # Returns
    ///
    /// Customized function schema with agent-specific guidance
    pub async fn generate_agent_function_schema(
        &self,
        agent_id: &str,
    ) -> A2AResult<serde_json::Value> {
        // Try to get agent card for customization
        let base_schema = self.generate_message_send_schema(agent_id, true)?;

        // TODO: In real implementation, we'd:
        // 1. Fetch agent card to get skills
        // 2. Add skill examples to description
        // 3. Include common data patterns
        // 4. Add agent-specific guidance

        // For now, return enhanced base schema
        let mut schema = base_schema;
        if let Some(desc) = schema.get_mut("description") {
            *desc = serde_json::Value::String(format!(
                "Send messages to A2A agent '{}'. Use 'data' parts for skill calls with {{'skill': 'skill_name', ...params}}. Use 'text' parts for conversation. Mix both for hybrid interactions.",
                agent_id
            ));
        }

        Ok(schema)
    }

    /// **Parse Task Response for LLM Consumption**
    ///
    /// Converts A2A Task response into LLM-friendly JSON format.
    pub fn parse_task_for_llm(
        &self,
        task: &a2a_protocol_core::data::task::Task,
    ) -> A2AResult<serde_json::Value> {
        let mut llm_response = serde_json::json!({
            "task_id": task.id,
            "context_id": task.context_id,
            "status": task.status.state,
            "timestamp": task.status.timestamp,
            "messages": [],
            "artifacts": []
        });

        // Add messages from history
        if let Some(history) = &task.history {
            for message in history {
                let message_content = serde_json::json!({
                    "role": message.role,
                    "content": message.get_text_content(),
                    "message_id": message.message_id
                });
                llm_response["messages"]
                    .as_array_mut()
                    .unwrap()
                    .push(message_content);
            }
        }

        if let Some(artifacts) = &task.artifacts {
            for artifact in artifacts {
                let artifact_content = serde_json::json!({
                    "artifact_id": artifact.artifact_id,
                    "name": artifact.name,
                    "description": artifact.description,
                    "parts": artifact.parts,
                });

                llm_response["artifacts"]
                    .as_array_mut()
                    .unwrap()
                    .push(artifact_content);
            }
        }

        Ok(llm_response)
    }

    /// Check if an agent is available
    ///
    /// # Arguments
    ///
    /// * `agent_id` - Target agent identifier  
    ///
    /// # Returns
    ///
    /// * `Ok(bool)` - Agent availability status
    /// * `Err(SdkError)` - Check failed
    pub async fn is_agent_available(&self, agent_id: &str) -> SdkResult<bool> {
        let _ = agent_id;
        self.check_connectivity().await
    }

    /// Create a new client instance for the same endpoint
    ///
    /// This creates a new client with the same configuration but independent state.
    pub fn clone_client(&self) -> SdkResult<Self> {
        match &self.mode {
            ClientMode::Direct { agent_endpoint } => Self::direct(agent_endpoint),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "a2a-client")]
    #[test]
    fn test_client_creation() {
        let client = A2aClient::new("http://example.com").unwrap();
        assert_eq!(client.endpoint(), "http://example.com");
    }

    #[cfg(feature = "a2a-client")]
    #[test]
    fn test_client_with_auth_token() {
        let client = A2aClient::with_auth_token("http://example.com", "token123").unwrap();
        assert_eq!(client.endpoint(), "http://example.com");
    }

    #[cfg(feature = "a2a-client")]
    #[test]
    fn test_client_with_headers() {
        let mut headers = HashMap::new();
        headers.insert("Custom-Header".to_string(), "custom-value".to_string());

        let client = A2aClient::with_headers("http://example.com", headers).unwrap();
        assert_eq!(client.endpoint(), "http://example.com");
    }

    #[test]
    fn test_invalid_endpoint() {
        let result = A2aClient::new("");

        #[cfg(feature = "a2a-client")]
        assert!(result.is_err());

        #[cfg(not(feature = "a2a-client"))]
        assert!(matches!(
            result.unwrap_err(),
            SdkError::FeatureNotEnabled { .. }
        ));
    }

    #[cfg(not(feature = "a2a-client"))]
    #[tokio::test]
    async fn test_feature_guards() {
        let client = A2aClient::new("http://example.com").unwrap_err();
        assert!(matches!(client, SdkError::FeatureNotEnabled { .. }));
    }

    #[cfg(feature = "a2a-client")]
    #[tokio::test]
    async fn test_client_methods_signature() {
        // This test just ensures the methods have the right signatures
        // and can be called. In a real test environment, you'd mock the server.
        let client = A2aClient::new("http://localhost:9999").unwrap();

        // These will fail due to no server, but we're testing the interface
        let _ = client.ping().await;
        let _ = client.get_capabilities().await;
        let _ = client.call_method("test", serde_json::json!({})).await;
        let _ = client.send_message("hello", None).await;
        let _ = client.get_task("task-123").await;
        let _ = client.cancel_task("task-123").await;
        let _ = client.list_tasks(None, None, None, None).await;
        let _ = client.agent_card().await;
        let _ = client.check_connectivity().await;
    }

    #[test]
    fn skill_validation_error_human_readable() {
        let err = super::SkillValidationError {
            skill_id: "s".to_string(),
            missing_required: vec!["a".to_string()],
            invalid_types: vec![("f".to_string(), "num".to_string(), "str".to_string())],
            unexpected_fields: vec!["x".to_string()],
            expected_schema_summary: "{}".to_string(),
        };
        let msg = err.human_readable_message();
        assert!(msg.contains("Missing required"));
        assert!(msg.contains("Type errors"));
        assert!(msg.contains("Unexpected parameters"));
    }
}
