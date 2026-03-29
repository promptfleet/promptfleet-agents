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

    pub fn with_tenant(mut self, tenant: impl Into<String>) -> Self {
        self.tenant = Some(tenant.into());
        self
    }

    pub fn with_token(mut self, token: impl Into<String>) -> Self {
        self.token = Some(token.into());
        self
    }

    pub fn with_authentication(mut self, auth: AuthenticationInfo) -> Self {
        self.authentication = Some(auth);
        self
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

    #[test]
    fn test_builder_chain_with_tenant_and_token() {
        let config = TaskPushNotificationConfig::new("cfg-1", "task-1", "https://example.com/hook")
            .with_tenant("acme-corp")
            .with_token("bearer-abc");
        assert_eq!(config.tenant.as_deref(), Some("acme-corp"));
        assert_eq!(config.token.as_deref(), Some("bearer-abc"));
    }

    #[test]
    fn test_builder_chain_with_authentication() {
        let auth = AuthenticationInfo {
            scheme: "Bearer".to_string(),
            credentials: "tok-xyz".to_string(),
        };
        let config = TaskPushNotificationConfig::new("cfg-2", "task-2", "https://example.com/hook")
            .with_authentication(auth.clone());
        assert_eq!(
            config.authentication.as_ref().unwrap().credentials,
            "tok-xyz"
        );
    }

    #[test]
    fn test_config_roundtrip_with_auth() {
        let auth = AuthenticationInfo {
            scheme: "Bearer".to_string(),
            credentials: "secret".to_string(),
        };
        let config = TaskPushNotificationConfig::new("c", "t", "https://x.example.com")
            .with_tenant("tenant-1")
            .with_authentication(auth);
        let json = serde_json::to_value(&config).unwrap();
        assert_eq!(json["tenant"], "tenant-1");
        assert_eq!(json["authentication"]["scheme"], "Bearer");
        let deser: TaskPushNotificationConfig = serde_json::from_value(json).unwrap();
        assert_eq!(deser.task_id, "t");
    }
}
