//! A2A Protocol v1.0 Data Structures

#[cfg(feature = "protocol-core")]
pub mod task;

#[cfg(feature = "protocol-core")]
pub mod message;

#[cfg(feature = "protocol-core")]
pub mod artifact;

pub mod notification;

#[cfg(feature = "protocol-core")]
pub use task::{Task, TaskState, TaskStatus};

#[cfg(feature = "protocol-core")]
pub use message::{Message, MessageRole, Part};

#[cfg(feature = "protocol-core")]
pub use artifact::Artifact;

pub use notification::{AuthenticationInfo, TaskPushNotificationConfig};
