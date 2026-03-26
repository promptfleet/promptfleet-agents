//! Typed interaction payloads for human-in-the-loop flows (questions, confirmations).
//!
//! See [`InteractionRequest`] and [`InteractionResponse`] for the wire shape consumed by adapters.

use serde::{Deserialize, Serialize};

/// Interaction kinds supported by the typed interaction contract.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InteractionKind {
    Question,
    Confirmation,
}

/// A selectable option for interaction prompts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InteractionOption {
    pub id: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Request payload emitted by agents to ask for user input.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InteractionRequest {
    pub interaction_id: String,
    pub kind: InteractionKind,
    pub question: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<InteractionOption>,
    #[serde(default)]
    pub allow_free_text: bool,
    #[serde(default)]
    pub allow_cancel: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_option_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub continuation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_node: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

/// Response payload for resolving a pending interaction.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InteractionResponse {
    pub interaction_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_option_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub free_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confirmed: Option<bool>,
    #[serde(default)]
    pub cancelled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}
