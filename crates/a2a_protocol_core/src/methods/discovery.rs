//! A2A v1.0 Agent Discovery Methods
//!
//! Simplified: `GetExtendedAgentCard` returns the same `AgentCard` type
//! with auth-gated extra data appended to metadata.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

use crate::{A2AError, A2AResult, AgentCard};

/// Params for `GetExtendedAgentCard` (was `agent/card/getAuthenticatedExtended`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthenticatedExtendedCardParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_token: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, Value>>,
}

/// Result for `GetExtendedAgentCard`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthenticatedExtendedCardResult {
    pub agent_card: AgentCard,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub discovery_metadata: Option<HashMap<String, Value>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
}

/// Trait for agent discovery.
pub trait AgentDiscovery {
    fn agent_authenticated_extended_card(
        &self,
        params: AuthenticatedExtendedCardParams,
    ) -> A2AResult<AuthenticatedExtendedCardResult>;
}

/// Default discovery implementation.
pub struct DefaultAgentDiscovery {
    agent_card: AgentCard,
    require_auth: bool,
}

impl DefaultAgentDiscovery {
    pub fn new(agent_card: AgentCard) -> Self {
        Self {
            agent_card,
            require_auth: false,
        }
    }

    pub fn with_authentication_required(mut self, required: bool) -> Self {
        self.require_auth = required;
        self
    }

    fn validate_authentication(&self, params: &AuthenticatedExtendedCardParams) -> A2AResult<()> {
        if self.require_auth && params.auth_token.is_none() {
            return Err(A2AError::invalid_params(
                "GetExtendedAgentCard",
                "Authentication token required",
            ));
        }
        Ok(())
    }
}

impl AgentDiscovery for DefaultAgentDiscovery {
    fn agent_authenticated_extended_card(
        &self,
        params: AuthenticatedExtendedCardParams,
    ) -> A2AResult<AuthenticatedExtendedCardResult> {
        self.validate_authentication(&params)?;

        let agent_card = self.agent_card.clone();

        let mut discovery_metadata = HashMap::new();
        discovery_metadata.insert(
            "method".to_string(),
            Value::String("GetExtendedAgentCard".to_string()),
        );
        discovery_metadata.insert(
            "supportsAuthentication".to_string(),
            Value::Bool(self.require_auth),
        );

        let timestamp = {
            #[cfg(feature = "time-stamps")]
            {
                Some(chrono::Utc::now().to_rfc3339())
            }
            #[cfg(not(feature = "time-stamps"))]
            {
                None
            }
        };

        Ok(AuthenticatedExtendedCardResult {
            agent_card,
            discovery_metadata: Some(discovery_metadata),
            timestamp,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_discovery_no_auth() {
        let card = AgentCard::new("test-agent");
        let discovery = DefaultAgentDiscovery::new(card);
        let params = AuthenticatedExtendedCardParams::default();
        let result = discovery.agent_authenticated_extended_card(params);
        assert!(result.is_ok());
    }

    #[test]
    fn test_discovery_auth_required() {
        let card = AgentCard::new("test-agent");
        let discovery = DefaultAgentDiscovery::new(card).with_authentication_required(true);
        let params = AuthenticatedExtendedCardParams::default();
        assert!(discovery.agent_authenticated_extended_card(params).is_err());

        let params_with_token = AuthenticatedExtendedCardParams {
            auth_token: Some("token".to_string()),
            scope: None,
            metadata: None,
        };
        assert!(
            discovery
                .agent_authenticated_extended_card(params_with_token)
                .is_ok()
        );
    }

    #[test]
    fn test_discovery_result_contains_metadata() {
        let card = AgentCard::new("test-agent");
        let discovery = DefaultAgentDiscovery::new(card);
        let result = discovery
            .agent_authenticated_extended_card(AuthenticatedExtendedCardParams::default())
            .unwrap();
        let meta = result.discovery_metadata.as_ref().unwrap();
        assert_eq!(meta["method"], "GetExtendedAgentCard");
    }

    #[test]
    fn test_discovery_supports_authentication_flag() {
        let card = AgentCard::new("test-agent");
        let discovery = DefaultAgentDiscovery::new(card).with_authentication_required(true);
        let params = AuthenticatedExtendedCardParams {
            auth_token: Some("token".to_string()),
            scope: None,
            metadata: None,
        };
        let result = discovery.agent_authenticated_extended_card(params).unwrap();
        let meta = result.discovery_metadata.as_ref().unwrap();
        assert_eq!(meta["supportsAuthentication"], true);
    }

    #[test]
    fn test_discovery_agent_card_preserved() {
        let card = AgentCard::new("my-agent");
        let discovery = DefaultAgentDiscovery::new(card);
        let result = discovery
            .agent_authenticated_extended_card(AuthenticatedExtendedCardParams::default())
            .unwrap();
        assert_eq!(result.agent_card.name, "my-agent");
    }

    #[cfg(feature = "time-stamps")]
    #[test]
    fn test_discovery_timestamp_present_with_feature() {
        let card = AgentCard::new("test-agent");
        let discovery = DefaultAgentDiscovery::new(card);
        let result = discovery
            .agent_authenticated_extended_card(AuthenticatedExtendedCardParams::default())
            .unwrap();
        assert!(
            result.timestamp.is_some(),
            "timestamp should be set when time-stamps feature is enabled"
        );
    }
}
