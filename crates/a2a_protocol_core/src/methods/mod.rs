//! A2A v1.0 Standard Methods

#[cfg(feature = "protocol-core")]
pub mod messaging;

#[cfg(feature = "protocol-core")]
pub mod tasks;

#[cfg(feature = "protocol-core")]
pub mod params;

pub mod discovery;

// Handler functions (messaging::handle_*, tasks::handle_*) are accessible
// via their sub-modules but are intentionally not re-exported here.

#[cfg(feature = "protocol-core")]
pub use params::{
    CancelTaskRequest, CreateTaskPushNotificationConfigRequest,
    DeleteTaskPushNotificationConfigRequest, GetTaskPushNotificationConfigRequest, GetTaskRequest,
    ListTaskPushNotificationConfigsRequest, ListTasksRequest, ListTasksResponse, MessageSendParams,
    MessageSendResponse, SendMessageConfiguration, SendMessageRequest, SendMessageResponse,
    SubscribeToTaskRequest, TaskCancelParams, TaskGetParams, TaskListParams, TaskListResult,
};

pub use discovery::{
    AgentDiscovery, AuthenticatedExtendedCardParams, AuthenticatedExtendedCardResult,
    DefaultAgentDiscovery,
};
