//! # A2A Protocol Core — v1.0.0
//!
//! Pure A2A (Agent-to-Agent) protocol domain logic, completely transport agnostic.

pub mod agent;
pub mod error;
pub mod protocol;
pub mod registry;
pub mod security;
pub mod transport;

#[cfg(feature = "protocol-core")]
pub mod data;

#[cfg(feature = "protocol-core")]
pub mod methods;

#[cfg(feature = "event-stream")]
pub mod streaming;

#[cfg(feature = "protocol-core")]
pub mod services;

// Agent metadata types
pub use agent::{
    AgentCapabilities, AgentCard, AgentCardSignature, AgentExtension, AgentInterface,
    AgentProvider, AgentSkill,
};

// Error types and A2A error codes
pub use error::{A2AError, A2AResult, a2a_error_codes};

// Protocol handler
pub use protocol::A2AProtocol;

// Method registry
pub use registry::{
    A2AMethodHandler, A2AMethodRegistry, A2ANotificationHandler, MethodMetadata, RegistryStats,
};

// Security scheme types
pub use security::{
    ApiKeySecurityScheme, AuthorizationCodeOAuthFlow, ClientCredentialsOAuthFlow,
    DeviceCodeOAuthFlow, HttpAuthSecurityScheme, MutualTlsSecurityScheme, OAuth2SecurityScheme,
    OAuthFlows, OpenIdConnectSecurityScheme, SecurityRequirement, SecurityScheme,
};

// Transport traits (no MockTransport — test-only helper)
pub use transport::{A2ATransport, A2ATransportFactory};

// Data types (feature = "protocol-core")
#[cfg(feature = "protocol-core")]
pub use data::{
    Artifact, AuthenticationInfo, Message, MessageRole, Part, Task, TaskPushNotificationConfig,
    TaskState, TaskStatus,
};

// Method params and discovery types (feature = "protocol-core")
// Handler functions (handle_message_send, handle_tasks_*) are not re-exported — use via methods module.
#[cfg(feature = "protocol-core")]
pub use methods::{
    discovery::{
        AgentDiscovery, AuthenticatedExtendedCardParams, AuthenticatedExtendedCardResult,
        DefaultAgentDiscovery,
    },
    params::{
        CancelTaskRequest, CreateTaskPushNotificationConfigRequest,
        DeleteTaskPushNotificationConfigRequest, GetTaskPushNotificationConfigRequest,
        GetTaskRequest, ListTaskPushNotificationConfigsRequest, ListTasksRequest,
        ListTasksResponse, MessageSendParams, MessageSendResponse, SendMessageConfiguration,
        SendMessageRequest, SendMessageResponse, SubscribeToTaskRequest, TaskCancelParams,
        TaskGetParams, TaskListParams, TaskListResult,
    },
};

// Task storage service (feature = "protocol-core")
#[cfg(feature = "protocol-core")]
pub use services::{ConversationContext, InMemoryTaskStorage, TaskStorage};

/// A2A Protocol Version
pub const A2A_PROTOCOL_VERSION: &str = "1.0";

pub use protocol_transport_core::{
    JSONRPC_VERSION, JsonRpcError, JsonRpcId, JsonRpcIncoming, JsonRpcNotification, JsonRpcRequest,
    JsonRpcResponse, error_codes as jsonrpc_error_codes,
};
