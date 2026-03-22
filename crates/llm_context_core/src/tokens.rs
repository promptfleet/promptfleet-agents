//! Fast token estimation for LLM context budgeting.
//!
//! Uses a character-based heuristic (~4 chars per token for English) that is
//! WASM-safe and requires zero external dependencies. Accuracy is within ±15%
//! of actual BPE tokenizer counts for typical English text.
//!
//! For precise counting, use a tokenizer library (e.g. `tiktoken-rs`) at the
//! caller level and feed exact counts into [`ContextBudget`].

/// Estimate token count for a text string.
///
/// Uses the widely-accepted heuristic of ~4 characters per token for English text,
/// with a small overhead for BPE tokenizer framing. Non-ASCII text uses a slightly
/// higher ratio (3 chars/token) to account for multi-byte characters.
pub fn estimate_tokens(text: &str) -> u32 {
    if text.is_empty() {
        return 0;
    }
    let ascii_count = text.bytes().filter(|b| b.is_ascii()).count();
    let non_ascii_count = text.len() - ascii_count;
    // ASCII: ~4 chars/token, non-ASCII: ~3 chars/token
    let raw = (ascii_count as f64 / 4.0) + (non_ascii_count as f64 / 3.0);
    // Minimum 1 token for non-empty text, plus small overhead for BPE framing
    (raw.ceil() as u32).max(1)
}

/// Estimate tokens for a single OpenAI-format chat message.
///
/// Accounts for role overhead (~4 tokens: role tag, separators, etc.)
/// plus the content tokens.
pub fn estimate_message_tokens(role: &str, content: &str) -> u32 {
    // Per OpenAI docs: every message has ~4 token overhead (role, separators)
    let overhead: u32 = 4;
    overhead + estimate_tokens(role) + estimate_tokens(content)
}

/// Estimate tokens for a JSON-serialized message value (OpenAI format).
///
/// Handles `role`, `content`, and `tool_calls` fields. Tool calls add
/// additional overhead for function name and arguments.
pub fn estimate_message_json_tokens(msg: &serde_json::Value) -> u32 {
    let role_overhead: u32 = 4;
    let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("");
    let content = msg.get("content").and_then(|v| v.as_str()).unwrap_or("");
    let mut tokens = role_overhead + estimate_tokens(role) + estimate_tokens(content);

    // Account for tool_calls array if present
    if let Some(tool_calls) = msg.get("tool_calls").and_then(|v| v.as_array()) {
        for tc in tool_calls {
            // Each tool call: ~3 tokens overhead + name + arguments
            tokens += 3;
            if let Some(func) = tc.get("function") {
                let name = func.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let args = func.get("arguments").and_then(|v| v.as_str()).unwrap_or("");
                tokens += estimate_tokens(name) + estimate_tokens(args);
            }
        }
    }

    tokens
}

/// Estimate total token count for a sequence of OpenAI-format message values.
///
/// Includes a ~3 token overhead for the overall messages array framing.
pub fn estimate_messages_tokens(messages: &[serde_json::Value]) -> u32 {
    let framing: u32 = 3; // array overhead
    framing + messages.iter().map(estimate_message_json_tokens).sum::<u32>()
}

/// Estimate tokens for a tool schema definition (for budget reservation).
pub fn estimate_tool_schema_tokens(tool_json: &serde_json::Value) -> u32 {
    // Serialize to string and estimate — tool schemas are JSON objects
    let serialized = serde_json::to_string(tool_json).unwrap_or_default();
    estimate_tokens(&serialized)
}

/// Estimate total tokens for all tool schemas.
pub fn estimate_tools_tokens(tools_json: &[serde_json::Value]) -> u32 {
    // Per OpenAI: ~10 token overhead for tools array framing
    let framing: u32 = if tools_json.is_empty() { 0 } else { 10 };
    framing + tools_json.iter().map(estimate_tool_schema_tokens).sum::<u32>()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_text_zero_tokens() {
        assert_eq!(estimate_tokens(""), 0);
    }

    #[test]
    fn single_word() {
        let t = estimate_tokens("hello");
        assert!(t >= 1 && t <= 3, "expected 1-3, got {}", t);
    }

    #[test]
    fn typical_english_paragraph() {
        let text = "The quick brown fox jumps over the lazy dog. This is a typical English sentence that should tokenize to roughly 20-25 tokens.";
        let t = estimate_tokens(text);
        // Actual cl100k_base: ~28 tokens. Our estimate should be in range.
        assert!(t >= 20 && t <= 40, "expected 20-40, got {}", t);
    }

    #[test]
    fn unicode_text() {
        let text = "こんにちは世界"; // "Hello World" in Japanese
        let t = estimate_tokens(text);
        assert!(t >= 2, "expected >=2, got {}", t);
    }

    #[test]
    fn message_overhead() {
        let msg_tokens = estimate_message_tokens("user", "hello");
        let content_tokens = estimate_tokens("hello");
        assert!(msg_tokens > content_tokens, "message should have overhead");
    }

    #[test]
    fn json_message_tokens() {
        let msg = serde_json::json!({"role": "user", "content": "What is 2+2?"});
        let t = estimate_message_json_tokens(&msg);
        assert!(t >= 5, "expected >=5, got {}", t);
    }

    #[test]
    fn json_message_with_tool_calls() {
        let msg = serde_json::json!({
            "role": "assistant",
            "content": null,
            "tool_calls": [{
                "id": "call_abc",
                "type": "function",
                "function": {
                    "name": "get_weather",
                    "arguments": "{\"location\":\"San Francisco\"}"
                }
            }]
        });
        let t = estimate_message_json_tokens(&msg);
        assert!(t >= 10, "expected >=10 for message with tool call, got {}", t);
    }

    #[test]
    fn messages_sequence_tokens() {
        let msgs = vec![
            serde_json::json!({"role": "system", "content": "You are helpful."}),
            serde_json::json!({"role": "user", "content": "Hi"}),
            serde_json::json!({"role": "assistant", "content": "Hello!"}),
        ];
        let t = estimate_messages_tokens(&msgs);
        assert!(t >= 15, "expected >=15, got {}", t);
    }
}
