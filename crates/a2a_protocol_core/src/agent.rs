//! A2A v1.0 Agent Metadata Types
//!
//! Complete AgentCard redesign per A2A Protocol v1.0.0 specification.

use crate::security::{SecurityRequirement, SecurityScheme};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// **Agent Skill** (v1.0)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSkill {
    pub id: String,
    pub name: String,
    pub description: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub examples: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_modes: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_modes: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub security_requirements: Option<Vec<SecurityRequirement>>,
}

/// **Agent Interface** (v1.0) — how to reach this agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentInterface {
    pub url: String,
    pub protocol_binding: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub tenant: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub protocol_version: Option<String>,
}

/// **Agent Capabilities** (v1.0)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCapabilities {
    #[serde(default)]
    pub streaming: bool,

    #[serde(default)]
    pub push_notifications: bool,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub extensions: Option<Vec<AgentExtension>>,

    #[serde(default)]
    pub extended_agent_card: bool,
}

/// **Agent Extension** (v1.0)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentExtension {
    pub uri: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    #[serde(default)]
    pub required: bool,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

/// **Agent Provider** (v1.0)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentProvider {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    pub organization: String,
}

/// JWS signature for agent card integrity.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCardSignature {
    pub protected: String,
    pub signature: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub header: Option<Value>,
}

/// **Agent Card** — v1.0 top-level metadata object.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCard {
    pub name: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub supported_interfaces: Option<Vec<AgentInterface>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<AgentCapabilities>,

    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub skills: Vec<AgentSkill>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_input_modes: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_output_modes: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub security_schemes: Option<HashMap<String, SecurityScheme>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub security_requirements: Option<Vec<SecurityRequirement>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub signatures: Option<Vec<AgentCardSignature>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon_url: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<AgentProvider>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub documentation_url: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, Value>>,
}

impl Default for AgentCapabilities {
    fn default() -> Self {
        Self {
            streaming: false,
            push_notifications: false,
            extensions: None,
            extended_agent_card: false,
        }
    }
}

impl AgentCard {
    /// Minimal card with just a name.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: None,
            version: None,
            supported_interfaces: None,
            capabilities: Some(AgentCapabilities::default()),
            skills: Vec::new(),
            default_input_modes: None,
            default_output_modes: None,
            security_schemes: None,
            security_requirements: None,
            signatures: None,
            icon_url: None,
            provider: None,
            documentation_url: None,
            metadata: None,
        }
    }

    /// Register a method capability on this agent card.
    ///
    /// Stores the method name and description in the card's `pf:methods`
    /// metadata map. Used by `A2AProtocol::register_method` to keep the
    /// agent card in sync with the method registry.
    pub fn with_capability(
        mut self,
        method: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        let method = method.into();
        let desc: String = description.into();
        let meta = self.metadata.get_or_insert_with(HashMap::new);
        let methods = meta
            .entry("pf:methods".to_string())
            .or_insert_with(|| serde_json::json!({}));
        if let Some(obj) = methods.as_object_mut() {
            obj.insert(method, serde_json::json!(desc));
        }
        self
    }

    pub fn supports_method(&self, method: &str) -> bool {
        self.metadata
            .as_ref()
            .and_then(|m| m.get("pf:methods"))
            .and_then(|v| v.as_object())
            .map(|o| o.contains_key(method))
            .unwrap_or(false)
    }

    pub fn get_method_description(&self, method: &str) -> Option<String> {
        self.metadata
            .as_ref()?
            .get("pf:methods")?
            .as_object()?
            .get(method)?
            .as_str()
            .map(|s| s.to_string())
    }

    pub fn add_skill(mut self, skill: AgentSkill) -> Self {
        self.skills.push(skill);
        self
    }

    pub fn get_skill(&self, skill_id: &str) -> Option<&AgentSkill> {
        self.skills.iter().find(|s| s.id == skill_id)
    }

    pub fn get_skills_by_input_mode(&self, input_mode: &str) -> Vec<&AgentSkill> {
        self.skills
            .iter()
            .filter(|s| {
                s.input_modes
                    .as_ref()
                    .map(|modes| modes.contains(&input_mode.to_string()))
                    .unwrap_or(false)
            })
            .collect()
    }

    pub fn supports_structured_skills(&self) -> bool {
        self.skills.iter().any(|s| {
            s.input_modes
                .as_ref()
                .map(|m| m.contains(&"application/json".to_string()))
                .unwrap_or(false)
        })
    }
}

impl AgentSkill {
    pub fn with_examples(mut self, examples: Vec<String>) -> Self {
        self.examples = Some(examples);
        self
    }

    pub fn with_example(mut self, example: impl Into<String>) -> Self {
        self.examples
            .get_or_insert_with(Vec::new)
            .push(example.into());
        self
    }

    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = Some(tags);
        self
    }

    pub fn supports_json_input(&self) -> bool {
        self.input_modes
            .as_ref()
            .map(|m| m.contains(&"application/json".to_string()))
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_card_creation() {
        let card = AgentCard::new("test-agent").with_capability("ping", "Simple ping method");
        assert_eq!(card.name, "test-agent");
        assert!(card.supports_method("ping"));
        assert!(!card.supports_method("unknown"));
    }

    #[test]
    fn test_agent_card_serialization_camel_case() {
        let card = AgentCard::new("test");
        let json = serde_json::to_value(&card).unwrap();
        assert!(json.get("name").is_some());
        assert!(json.get("capabilities").is_some());
    }

    #[test]
    fn test_agent_card_with_interface() {
        let mut card = AgentCard::new("test-agent");
        card.supported_interfaces = Some(vec![AgentInterface {
            url: "http://localhost:8080/jsonrpc".to_string(),
            protocol_binding: "JSONRPC".to_string(),
            tenant: None,
            protocol_version: Some("1.0".to_string()),
        }]);
        let json = serde_json::to_value(&card).unwrap();
        let iface = &json["supportedInterfaces"][0];
        assert_eq!(iface["protocolBinding"], "JSONRPC");
    }

    #[test]
    fn test_agent_skill() {
        let skill = AgentSkill {
            id: "get_weather".to_string(),
            name: "Get Weather".to_string(),
            description: "Get current weather".to_string(),
            tags: Some(vec!["weather".to_string()]),
            examples: None,
            input_modes: Some(vec![
                "application/json".to_string(),
                "text/plain".to_string(),
            ]),
            output_modes: Some(vec!["text/plain".to_string()]),
            security_requirements: None,
        };
        assert!(skill.supports_json_input());

        let card = AgentCard::new("weather-agent").add_skill(skill);
        assert_eq!(card.skills.len(), 1);
        assert!(card.supports_structured_skills());
    }
}
