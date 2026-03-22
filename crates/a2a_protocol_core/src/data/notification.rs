//! A2A v1.0 Push Notification Types
//!
//! Spec-aligned types for task push notification configuration CRUD.

use serde::{Deserialize, Serialize};

/// Authentication information for push notification delivery.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthenticationInfo {
    pub scheme: String,
    pub credentials: String,
}

/// Per-task push notification configuration (v1.0).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskPushNotificationConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tenant: Option<String>,

    pub id: String,

    pub task_id: String,

    pub url: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub authentication: Option<AuthenticationInfo>,
}

impl TaskPushNotificationConfig {
    pub fn new(id: impl Into<String>, task_id: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            tenant: None,
            id: id.into(),
            task_id: task_id.into(),
            url: url.into(),
            token: None,
            authentication: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_push_notification_config() {
        let config = TaskPushNotificationConfig::new("cfg-1", "task-1", "https://example.com/hook");
        let json = serde_json::to_value(&config).unwrap();
        assert_eq!(json["taskId"], "task-1");
        assert_eq!(json["url"], "https://example.com/hook");
    }

    #[test]
    fn test_authentication_info() {
        let auth = AuthenticationInfo {
            scheme: "Bearer".to_string(),
            credentials: "secret".to_string(),
        };
        let json = serde_json::to_value(&auth).unwrap();
        assert_eq!(json["scheme"], "Bearer");
    }
}
