//! Context window management strategies.
//!
//! Each strategy implements [`ContextStrategy`] and decides which messages
//! to keep when the conversation history exceeds the token budget.

use crate::budget::ContextBudget;
use crate::tokens;
use serde::{Deserialize, Serialize};

/// Selects which context strategy to use (serializable for config).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ContextStrategyKind {
    /// Keep the most recent messages that fit within budget.
    SlidingWindow,
    /// Sliding window, but prepend a summary of evicted messages.
    SlidingWindowWithSummary,
    /// Score messages by importance; keep highest-scoring within budget.
    PriorityBased,
}

impl Default for ContextStrategyKind {
    fn default() -> Self {
        Self::SlidingWindow
    }
}

/// Trait for context window management strategies.
///
/// Implementations decide which messages to retain when the conversation
/// history exceeds the available token budget. The contract:
///
/// - Input: full message history (OpenAI JSON format) + budget
/// - Output: trimmed message list that fits within `budget.available_for_history`
/// - Ordering must be preserved (messages keep their chronological order)
pub trait ContextStrategy: Send + Sync {
    /// Apply this strategy to trim messages to fit within budget.
    ///
    /// Returns a new `Vec` containing only the messages that should be
    /// sent to the LLM. The caller owns the returned vector.
    fn apply(
        &self,
        messages: &[serde_json::Value],
        budget: &ContextBudget,
    ) -> Vec<serde_json::Value>;

    /// Human-readable name for logging and diagnostics.
    fn name(&self) -> &'static str;
}

// ---------------------------------------------------------------------------
// SlidingWindow — keep the last N messages that fit
// ---------------------------------------------------------------------------

/// Keeps the most recent messages that fit within the token budget.
///
/// This is the simplest and most predictable strategy. It always retains
/// the newest context, which is usually the most relevant. Messages are
/// dropped from the beginning (oldest first).
///
/// If a system message is present at position 0, it is always retained.
pub struct SlidingWindow;

impl ContextStrategy for SlidingWindow {
    fn apply(
        &self,
        messages: &[serde_json::Value],
        budget: &ContextBudget,
    ) -> Vec<serde_json::Value> {
        if messages.is_empty() {
            return Vec::new();
        }

        let budget_tokens = budget.available_for_history;

        // Check if everything fits
        let total = tokens::estimate_messages_tokens(messages);
        if total <= budget_tokens {
            return messages.to_vec();
        }

        // Separate system message (always retained) from the rest
        let (system_msg, rest) = if is_system_message(&messages[0]) {
            (Some(&messages[0]), &messages[1..])
        } else {
            (None, messages)
        };

        let system_tokens = system_msg
            .map(|m| tokens::estimate_message_json_tokens(m))
            .unwrap_or(0);
        let remaining_budget = budget_tokens.saturating_sub(system_tokens);

        // Walk backwards, accumulating messages until budget is exhausted
        let mut kept: Vec<&serde_json::Value> = Vec::new();
        let mut used: u32 = 3; // messages array framing overhead
        for msg in rest.iter().rev() {
            let msg_tokens = tokens::estimate_message_json_tokens(msg);
            if used + msg_tokens > remaining_budget {
                break;
            }
            used += msg_tokens;
            kept.push(msg);
        }
        kept.reverse();

        // Reconstruct: system (if any) + kept messages
        let mut result = Vec::with_capacity(kept.len() + 1);
        if let Some(sys) = system_msg {
            result.push(sys.clone());
        }
        for msg in kept {
            result.push(msg.clone());
        }

        result
    }

    fn name(&self) -> &'static str {
        "sliding_window"
    }
}

// ---------------------------------------------------------------------------
// PriorityBased — score messages by importance
// ---------------------------------------------------------------------------

/// Scores messages by importance and keeps the highest-scoring within budget.
///
/// Scoring heuristics:
/// - System messages: always retained (infinite priority)
/// - User messages: high priority (recency-weighted)
/// - Assistant messages with tool_calls: medium-high priority
/// - Tool result messages: medium priority (paired with their tool_call)
/// - Assistant text messages: medium priority (recency-weighted)
///
/// Within each priority tier, more recent messages score higher.
pub struct PriorityBased;

impl ContextStrategy for PriorityBased {
    fn apply(
        &self,
        messages: &[serde_json::Value],
        budget: &ContextBudget,
    ) -> Vec<serde_json::Value> {
        if messages.is_empty() {
            return Vec::new();
        }

        let budget_tokens = budget.available_for_history;
        let total = tokens::estimate_messages_tokens(messages);
        if total <= budget_tokens {
            return messages.to_vec();
        }

        let len = messages.len();
        let mut scored: Vec<(usize, f64, u32)> = messages
            .iter()
            .enumerate()
            .map(|(i, msg)| {
                let tokens = tokens::estimate_message_json_tokens(msg);
                let score = score_message(msg, i, len);
                (i, score, tokens)
            })
            .collect();

        // Sort by score descending (highest priority first)
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // Greedily select messages within budget
        let mut selected_indices: Vec<usize> = Vec::new();
        let mut used: u32 = 3; // framing overhead
        for (idx, _score, tokens) in &scored {
            if used + tokens > budget_tokens {
                continue;
            }
            used += tokens;
            selected_indices.push(*idx);
        }

        // Restore chronological order
        selected_indices.sort();

        selected_indices
            .iter()
            .map(|&i| messages[i].clone())
            .collect()
    }

    fn name(&self) -> &'static str {
        "priority_based"
    }
}

/// Score a message for priority-based selection.
///
/// Returns a score where higher = more important. System messages get
/// `f64::MAX` to ensure they're always kept.
fn score_message(msg: &serde_json::Value, index: usize, total_count: usize) -> f64 {
    let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("");
    let recency = if total_count > 0 {
        (index as f64 + 1.0) / total_count as f64
    } else {
        1.0
    };

    match role {
        "system" => f64::MAX,
        "user" => 100.0 + recency * 50.0,
        "assistant" => {
            let has_tool_calls = msg
                .get("tool_calls")
                .and_then(|v| v.as_array())
                .map(|a| !a.is_empty())
                .unwrap_or(false);
            if has_tool_calls {
                80.0 + recency * 40.0
            } else {
                60.0 + recency * 30.0
            }
        }
        "tool" => {
            // Tool results are important if recent, less so if old
            50.0 + recency * 40.0
        }
        _ => 10.0 + recency * 5.0,
    }
}

// ---------------------------------------------------------------------------
// Factory
// ---------------------------------------------------------------------------

/// Create a boxed strategy from a [`ContextStrategyKind`].
///
/// For `SlidingWindowWithSummary`, falls back to `SlidingWindow` since
/// summarization requires an external `Summarizer` (see [`HistoryManager`]).
pub fn create_strategy(kind: &ContextStrategyKind) -> Box<dyn ContextStrategy> {
    match kind {
        ContextStrategyKind::SlidingWindow => Box::new(SlidingWindow),
        ContextStrategyKind::SlidingWindowWithSummary => {
            // Summarization is handled at the HistoryManager level;
            // the raw strategy is still sliding window
            Box::new(SlidingWindow)
        }
        ContextStrategyKind::PriorityBased => Box::new(PriorityBased),
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn is_system_message(msg: &serde_json::Value) -> bool {
    msg.get("role").and_then(|v| v.as_str()) == Some("system")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_msg(role: &str, content: &str) -> serde_json::Value {
        serde_json::json!({"role": role, "content": content})
    }

    #[test]
    fn sliding_window_fits_all() {
        let budget = ContextBudget::new(100_000, 4_000, None, &[]);
        let msgs = vec![
            make_msg("system", "Be helpful"),
            make_msg("user", "Hello"),
            make_msg("assistant", "Hi there!"),
        ];
        let result = SlidingWindow.apply(&msgs, &budget);
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn sliding_window_trims_oldest() {
        // Very tight budget — available_for_history = 30 - 0 - 0 - 0 = 30 tokens
        // Each message is ~8-12 tokens (role overhead + content), so 4 messages
        // won't all fit in 30 tokens.
        let budget = ContextBudget::new(30, 0, None, &[]);
        let msgs = vec![
            make_msg("user", "First message with some longer text to consume tokens"),
            make_msg("assistant", "First reply that also has quite a bit of text in it"),
            make_msg("user", "Second message with additional padding for budget overflow"),
            make_msg("assistant", "Final reply with more text to force trimming"),
        ];
        let result = SlidingWindow.apply(&msgs, &budget);
        // Should keep fewer messages than the original
        assert!(
            result.len() < msgs.len(),
            "expected trimming, got {} messages (budget=30 tokens)",
            result.len()
        );
        // Last message should always be kept (most recent)
        let last_original = msgs.last().unwrap();
        let last_result = result.last().unwrap();
        assert_eq!(last_result["content"], last_original["content"]);
    }

    #[test]
    fn sliding_window_preserves_system() {
        let budget = ContextBudget::new(80, 10, None, &[]);
        let msgs = vec![
            make_msg("system", "You are a helpful assistant"),
            make_msg("user", "First message long enough to force trimming in small budget"),
            make_msg("assistant", "Reply that is also fairly long to use tokens"),
            make_msg("user", "Last"),
        ];
        let result = SlidingWindow.apply(&msgs, &budget);
        // System message should be first
        assert_eq!(
            result[0]["role"].as_str().unwrap(),
            "system",
            "system message should be preserved"
        );
    }

    #[test]
    fn priority_based_fits_all() {
        let budget = ContextBudget::new(100_000, 4_000, None, &[]);
        let msgs = vec![
            make_msg("user", "Hello"),
            make_msg("assistant", "Hi"),
        ];
        let result = PriorityBased.apply(&msgs, &budget);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn priority_based_keeps_system_and_recent() {
        let budget = ContextBudget::new(80, 10, None, &[]);
        let msgs = vec![
            make_msg("system", "You are helpful"),
            make_msg("user", "Old question that is pretty long to consume tokens"),
            make_msg("assistant", "Old answer that also consumes many tokens"),
            make_msg("user", "New question"),
            make_msg("assistant", "New answer"),
        ];
        let result = PriorityBased.apply(&msgs, &budget);
        // System should be kept
        let has_system = result
            .iter()
            .any(|m| m["role"].as_str() == Some("system"));
        assert!(has_system, "system message should be preserved");
        // Most recent user message should be kept (highest user score)
        let has_new_q = result
            .iter()
            .any(|m| m["content"].as_str() == Some("New question"));
        assert!(has_new_q, "most recent user message should be kept");
    }

    #[test]
    fn create_strategy_factory() {
        let s = create_strategy(&ContextStrategyKind::SlidingWindow);
        assert_eq!(s.name(), "sliding_window");

        let s = create_strategy(&ContextStrategyKind::PriorityBased);
        assert_eq!(s.name(), "priority_based");

        // SlidingWindowWithSummary falls back to sliding_window at strategy level
        let s = create_strategy(&ContextStrategyKind::SlidingWindowWithSummary);
        assert_eq!(s.name(), "sliding_window");
    }

    #[test]
    fn empty_messages() {
        let budget = ContextBudget::new(1000, 100, None, &[]);
        assert!(SlidingWindow.apply(&[], &budget).is_empty());
        assert!(PriorityBased.apply(&[], &budget).is_empty());
    }
}
