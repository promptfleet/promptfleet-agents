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
    /// Stable id for this option (used in `selected_option_id` on the response).
    pub id: String,
    /// Short label shown in the UI.
    pub label: String,
    /// Optional longer description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Request payload emitted by agents to ask for user input.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InteractionRequest {
    /// Correlates the request with [`InteractionResponse::interaction_id`].
    pub interaction_id: String,
    /// Question vs confirmation flow.
    pub kind: InteractionKind,
    /// Prompt text shown to the user.
    pub question: String,
    /// For choice-style prompts; empty if free-text only.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<InteractionOption>,
    /// Whether the user may type an answer instead of picking an option.
    #[serde(default)]
    pub allow_free_text: bool,
    /// Whether the user may dismiss without resolving (cancel).
    #[serde(default)]
    pub allow_cancel: bool,
    /// Pre-selected option id when `options` is non-empty.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_option_id: Option<String>,
    /// Optional auto-expiry for the interaction (milliseconds).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    /// Optional id for multi-step / resumed flows.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub continuation_id: Option<String>,
    /// Optional coordination graph node id (if applicable).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_node: Option<String>,
    /// Arbitrary extension payload.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

/// Response payload for resolving a pending interaction.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InteractionResponse {
    /// Must match the pending [`InteractionRequest::interaction_id`].
    pub interaction_id: String,
    /// Chosen option when the user picked from `options`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_option_id: Option<String>,
    /// Free-text answer when `allow_free_text` was true.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub free_text: Option<String>,
    /// For confirmations: explicit true/false when applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confirmed: Option<bool>,
    /// True when the user cancelled instead of resolving.
    #[serde(default)]
    pub cancelled: bool,
    /// Arbitrary extension payload.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}
