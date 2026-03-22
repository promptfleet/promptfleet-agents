//! Context budget computation — tracks token allocation across the LLM
//! context window to prevent overflow and optimize utilization.

use crate::tokens;

/// Token budget allocation for an LLM context window.
///
/// Breaks the total context window into reserved zones and computes
/// the remaining space available for conversation history.
///
/// ```text
/// ┌────────────────────────────────────────────────────┐
/// │                  Context Window (total)             │
/// ├────────────┬────────────┬────────┬─────────────────┤
/// │  System    │   Tools    │ Output │   Available     │
/// │  (fixed)   │  (schemas) │ (gen)  │  for History    │
/// └────────────┴────────────┴────────┴─────────────────┘
/// ```
#[derive(Debug, Clone)]
pub struct ContextBudget {
    /// Total context window in tokens (from ModelCapabilities).
    pub total: u32,
    /// Tokens reserved for model output generation.
    pub reserved_output: u32,
    /// Tokens consumed by the system prompt.
    pub reserved_system: u32,
    /// Tokens consumed by tool schema definitions.
    pub reserved_tools: u32,
    /// Remaining tokens available for conversation history + memories.
    pub available_for_history: u32,
}

impl ContextBudget {
    /// Create a new budget from model capabilities and current context.
    ///
    /// # Parameters
    /// - `context_window`: Total tokens the model can accept
    /// - `max_output_tokens`: Tokens to reserve for generation
    /// - `system_message`: The system prompt text (will be estimated)
    /// - `tools_json`: Tool schema definitions (will be estimated)
    pub fn new(
        context_window: u32,
        max_output_tokens: u32,
        system_message: Option<&str>,
        tools_json: &[serde_json::Value],
    ) -> Self {
        let reserved_system = system_message
            .map(|s| tokens::estimate_message_tokens("system", s))
            .unwrap_or(0);
        let reserved_tools = tokens::estimate_tools_tokens(tools_json);
        let reserved_output = max_output_tokens;

        let available_for_history = context_window
            .saturating_sub(reserved_output)
            .saturating_sub(reserved_system)
            .saturating_sub(reserved_tools);

        Self {
            total: context_window,
            reserved_output,
            reserved_system,
            reserved_tools,
            available_for_history,
        }
    }

    /// Compute how many tokens are actually used by a set of history messages.
    pub fn history_usage(&self, history_messages: &[serde_json::Value]) -> u32 {
        tokens::estimate_messages_tokens(history_messages)
    }

    /// Check whether adding the given messages would exceed the history budget.
    pub fn would_exceed(&self, history_messages: &[serde_json::Value]) -> bool {
        self.history_usage(history_messages) > self.available_for_history
    }

    /// Utilization ratio (0.0 to 1.0) of the full context window.
    pub fn utilization(&self, history_messages: &[serde_json::Value]) -> f32 {
        if self.total == 0 {
            return 0.0;
        }
        let used = self.reserved_system
            + self.reserved_tools
            + self.reserved_output
            + self.history_usage(history_messages);
        (used as f32 / self.total as f32).min(1.0)
    }

    /// Remaining tokens available after current history usage.
    pub fn remaining(&self, history_messages: &[serde_json::Value]) -> u32 {
        self.available_for_history
            .saturating_sub(self.history_usage(history_messages))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_budget() {
        let budget = ContextBudget::new(128_000, 16_384, Some("You are helpful."), &[]);
        assert!(budget.available_for_history > 100_000);
        assert_eq!(budget.reserved_output, 16_384);
        assert!(budget.reserved_system > 0);
    }

    #[test]
    fn budget_with_tools() {
        let tools = vec![serde_json::json!({
            "type": "function",
            "function": {
                "name": "get_weather",
                "description": "Get weather for a location",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "location": {"type": "string"}
                    }
                }
            }
        })];
        let budget = ContextBudget::new(128_000, 16_384, Some("You are helpful."), &tools);
        assert!(budget.reserved_tools > 0);
        assert!(budget.available_for_history < 128_000 - 16_384);
    }

    #[test]
    fn would_exceed_detection() {
        let budget = ContextBudget::new(100, 30, None, &[]);
        // Budget available = 100 - 30 = 70
        let small = vec![serde_json::json!({"role": "user", "content": "hi"})];
        assert!(!budget.would_exceed(&small));

        // Create a large message that exceeds budget
        let big_text = "a".repeat(500);
        let big = vec![serde_json::json!({"role": "user", "content": big_text})];
        assert!(budget.would_exceed(&big));
    }

    #[test]
    fn utilization_ratio() {
        let budget = ContextBudget::new(1000, 200, None, &[]);
        let msgs = vec![serde_json::json!({"role": "user", "content": "Hello world"})];
        let util = budget.utilization(&msgs);
        assert!(util > 0.0 && util < 1.0, "utilization={}", util);
    }

    #[test]
    fn zero_context_window() {
        let budget = ContextBudget::new(0, 0, None, &[]);
        assert_eq!(budget.available_for_history, 0);
        assert_eq!(budget.utilization(&[]), 0.0);
    }
}
