//! History manager — orchestrates context window management across tiers.
//!
//! The [`HistoryManager`] is the main integration point. It:
//! 1. Applies a [`ContextStrategy`] to trim history within budget
//! 2. Optionally injects long-term memories from a [`LongTermMemory`] backend
//! 3. Optionally summarizes evicted messages via a [`Summarizer`]
//! 4. Tracks token usage and provides diagnostics

use crate::budget::ContextBudget;
use crate::memory::{LongTermMemory, MemoryEntry, MemoryFilters, MemoryType, NoOpMemory};
use crate::strategy::{self, ContextStrategy, ContextStrategyKind};
use crate::tokens;
use serde::{Deserialize, Serialize};
use std::pin::Pin;
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Summarizer trait
// ---------------------------------------------------------------------------

/// Future type for summarization (WASM-compatible).
#[cfg(target_arch = "wasm32")]
pub type SummarizerFuture = Pin<Box<dyn Future<Output = Result<String, String>>>>;

#[cfg(not(target_arch = "wasm32"))]
pub type SummarizerFuture = Pin<Box<dyn Future<Output = Result<String, String>> + Send>>;

/// Trait for summarizing evicted conversation messages.
///
/// Implementations can use an LLM or extractive methods to compress
/// a sequence of messages into a concise summary.
pub trait Summarizer: Send + Sync {
    /// Summarize the given messages into a single text string.
    fn summarize(&self, messages: &[serde_json::Value]) -> SummarizerFuture;
}

/// Extractive summarizer — no LLM calls, just extracts key content.
///
/// Takes the first and last user messages plus any tool results,
/// joining them into a brief "Previously:" block.
pub struct ExtractiveSnippets {
    /// Maximum character length of the summary.
    pub max_chars: usize,
}

impl Default for ExtractiveSnippets {
    fn default() -> Self {
        Self { max_chars: 500 }
    }
}

impl Summarizer for ExtractiveSnippets {
    fn summarize(&self, messages: &[serde_json::Value]) -> SummarizerFuture {
        let max_chars = self.max_chars;
        let messages = messages.to_vec();
        Box::pin(async move {
            if messages.is_empty() {
                return Ok(String::new());
            }
            let mut snippets: Vec<String> = Vec::new();
            for msg in &messages {
                let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("");
                let content = msg.get("content").and_then(|v| v.as_str()).unwrap_or("");
                if content.is_empty() {
                    continue;
                }
                match role {
                    "user" | "assistant" => {
                        let truncated = if content.len() > 100 {
                            format!("{}...", &content[..100])
                        } else {
                            content.to_string()
                        };
                        snippets.push(format!("[{}] {}", role, truncated));
                    }
                    "tool" => {
                        let truncated = if content.len() > 80 {
                            format!("{}...", &content[..80])
                        } else {
                            content.to_string()
                        };
                        snippets.push(format!("[tool result] {}", truncated));
                    }
                    _ => {}
                }
            }
            let mut summary = snippets.join("\n");
            if summary.len() > max_chars {
                summary.truncate(max_chars);
                summary.push_str("...");
            }
            Ok(summary)
        })
    }
}

// ---------------------------------------------------------------------------
// HistoryManager configuration
// ---------------------------------------------------------------------------

/// Configuration for the [`HistoryManager`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryManagerConfig {
    /// Which strategy to use for trimming history.
    #[serde(default)]
    pub strategy: ContextStrategyKind,
    /// Whether to generate summaries of evicted messages.
    #[serde(default)]
    pub enable_summarization: bool,
    /// Whether to query long-term memory for relevant context.
    #[serde(default)]
    pub enable_long_term_memory: bool,
    /// Number of long-term memories to inject per turn.
    #[serde(default = "default_recall_top_k")]
    pub recall_top_k: usize,
    /// Maximum tokens to allocate for injected long-term memories.
    #[serde(default = "default_memory_token_budget")]
    pub memory_token_budget: u32,
}

fn default_recall_top_k() -> usize {
    5
}
fn default_memory_token_budget() -> u32 {
    2000
}

impl Default for HistoryManagerConfig {
    fn default() -> Self {
        Self {
            strategy: ContextStrategyKind::SlidingWindow,
            enable_summarization: false,
            enable_long_term_memory: false,
            recall_top_k: default_recall_top_k(),
            memory_token_budget: default_memory_token_budget(),
        }
    }
}

// ---------------------------------------------------------------------------
// HistoryManager
// ---------------------------------------------------------------------------

/// Central manager for LLM context window management.
///
/// Call [`prepare_messages`](Self::prepare_messages) before each LLM invocation
/// to get a trimmed, budget-aware message list. Call
/// [`on_turn_complete`](Self::on_turn_complete) after each turn to update
/// summaries and store memories.
pub struct HistoryManager {
    config: HistoryManagerConfig,
    strategy: Box<dyn ContextStrategy>,
    summarizer: Option<Arc<dyn Summarizer>>,
    memory: Arc<dyn LongTermMemory>,
    /// Accumulated summaries of evicted conversation segments.
    summaries: Vec<String>,
}

impl HistoryManager {
    /// Create a new HistoryManager with the given configuration.
    pub fn new(config: HistoryManagerConfig) -> Self {
        let strategy = strategy::create_strategy(&config.strategy);
        Self {
            config,
            strategy,
            summarizer: None,
            memory: Arc::new(NoOpMemory),
            summaries: Vec::new(),
        }
    }

    /// Set the summarizer implementation.
    pub fn with_summarizer(mut self, summarizer: Arc<dyn Summarizer>) -> Self {
        self.summarizer = Some(summarizer);
        self
    }

    /// Set the long-term memory backend.
    pub fn with_memory(mut self, memory: Arc<dyn LongTermMemory>) -> Self {
        self.memory = memory;
        self
    }

    /// Seed the manager with previously persisted conversation summaries.
    pub fn with_summaries(mut self, summaries: Vec<String>) -> Self {
        self.summaries = summaries;
        self
    }

    /// Prepare messages for the next LLM invocation.
    ///
    /// This is the main entry point. It:
    /// 1. Optionally retrieves relevant long-term memories
    /// 2. Constructs the system message (with memories + summaries)
    /// 3. Applies the context strategy to trim history within budget
    ///
    /// Returns the message list ready to be sent to the LLM.
    pub async fn prepare_messages(
        &self,
        budget: &ContextBudget,
        system_message: Option<&str>,
        history: &[serde_json::Value],
        current_turn: &[serde_json::Value],
        memory_filters: &MemoryFilters,
    ) -> Vec<serde_json::Value> {
        let mut messages: Vec<serde_json::Value> = Vec::new();

        // 1. Build enriched system message with memories + summaries
        let enriched_system = self
            .build_enriched_system(system_message, current_turn, memory_filters)
            .await;
        if let Some(sys) = &enriched_system {
            if !sys.is_empty() {
                messages.push(serde_json::json!({"role": "system", "content": sys}));
            }
        }

        // 2. Add history + current turn messages
        messages.extend_from_slice(history);
        messages.extend_from_slice(current_turn);

        // 3. Apply strategy to fit within budget (strategy handles system preservation)
        self.strategy.apply(&messages, budget)
    }

    /// Notify the manager that a turn has completed.
    ///
    /// If summarization is enabled, this may generate a summary of evicted
    /// messages and optionally store it in long-term memory.
    pub async fn on_turn_complete(
        &mut self,
        evicted_messages: &[serde_json::Value],
        agent_id: &str,
        user_id: Option<&str>,
        conversation_id: Option<&str>,
    ) {
        if evicted_messages.is_empty() {
            return;
        }

        // Generate summary if enabled
        if self.config.enable_summarization {
            if let Some(summarizer) = &self.summarizer {
                match summarizer.summarize(evicted_messages).await {
                    Ok(summary) if !summary.is_empty() => {
                        log::debug!(
                            "history_manager: generated summary ({} chars) from {} evicted messages",
                            summary.len(),
                            evicted_messages.len()
                        );
                        self.summaries.push(summary.clone());

                        // Store summary in long-term memory if enabled
                        if self.config.enable_long_term_memory {
                            let entry = MemoryEntry {
                                id: uuid_v4(),
                                agent_id: agent_id.to_string(),
                                user_id: user_id.map(|s| s.to_string()),
                                conversation_id: conversation_id.map(|s| s.to_string()),
                                content: summary,
                                memory_type: MemoryType::Summary,
                                timestamp: now_unix_secs(),
                                score: 0.0,
                                metadata: Default::default(),
                            };
                            if let Err(e) = self.memory.store(entry).await {
                                log::warn!("history_manager: failed to store memory: {}", e);
                            }
                        }
                    }
                    Ok(_) => {}
                    Err(e) => {
                        log::warn!("history_manager: summarization failed: {}", e);
                    }
                }
            }
        }
    }

    /// Get accumulated summaries (for diagnostics).
    pub fn summaries(&self) -> &[String] {
        &self.summaries
    }

    /// Get the strategy name (for diagnostics).
    pub fn strategy_name(&self) -> &'static str {
        self.strategy.name()
    }

    /// Build an enriched system message that includes long-term memories
    /// and accumulated summaries.
    async fn build_enriched_system(
        &self,
        base_system: Option<&str>,
        current_turn: &[serde_json::Value],
        memory_filters: &MemoryFilters,
    ) -> Option<String> {
        let mut parts: Vec<String> = Vec::new();

        // Base system message
        if let Some(sys) = base_system {
            if !sys.is_empty() {
                parts.push(sys.to_string());
            }
        }

        // Accumulated conversation summaries
        if !self.summaries.is_empty() {
            let summaries_block = format!(
                "Previously in this conversation:\n{}",
                self.summaries.join("\n---\n")
            );
            // Check token budget for summaries
            let summary_tokens = tokens::estimate_tokens(&summaries_block);
            if summary_tokens <= self.config.memory_token_budget / 2 {
                parts.push(summaries_block);
            } else {
                // Only keep the most recent summary if budget is tight
                if let Some(latest) = self.summaries.last() {
                    parts.push(format!("Previously: {}", latest));
                }
            }
        }

        // Long-term memory retrieval
        if self.config.enable_long_term_memory && self.config.recall_top_k > 0 {
            // Extract query from the most recent user message in current_turn
            let query = current_turn
                .iter()
                .rev()
                .find(|m| m.get("role").and_then(|v| v.as_str()) == Some("user"))
                .and_then(|m| m.get("content").and_then(|v| v.as_str()))
                .unwrap_or("");

            if !query.is_empty() {
                match self
                    .memory
                    .recall(query, self.config.recall_top_k, memory_filters.clone())
                    .await
                {
                    Ok(memories) if !memories.is_empty() => {
                        let mut memory_block = String::from("Relevant context from memory:\n");
                        let mut used_tokens: u32 = tokens::estimate_tokens(&memory_block);
                        let memory_budget = self.config.memory_token_budget;

                        for mem in &memories {
                            let entry_text = format!(
                                "- [{}] {}\n",
                                format_memory_type(&mem.memory_type),
                                mem.content
                            );
                            let entry_tokens = tokens::estimate_tokens(&entry_text);
                            if used_tokens + entry_tokens > memory_budget {
                                break;
                            }
                            memory_block.push_str(&entry_text);
                            used_tokens += entry_tokens;
                        }

                        if used_tokens > tokens::estimate_tokens("Relevant context from memory:\n")
                        {
                            parts.push(memory_block);
                        }
                    }
                    Ok(_) => {}
                    Err(e) => {
                        log::warn!("history_manager: memory recall failed: {}", e);
                    }
                }
            }
        }

        if parts.is_empty() {
            None
        } else {
            Some(parts.join("\n\n"))
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn format_memory_type(mt: &MemoryType) -> &'static str {
    match mt {
        MemoryType::Summary => "summary",
        MemoryType::Fact => "fact",
        MemoryType::Instruction => "instruction",
        MemoryType::ToolResult => "tool_result",
        MemoryType::Custom(_) => "custom",
    }
}

fn uuid_v4() -> String {
    // Simple UUID v4 without pulling in uuid crate — good enough for memory IDs
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    // Mix in some entropy from the stack pointer and a counter
    let ptr = &hasher as *const _ as usize;
    ptr.hash(&mut hasher);
    now_unix_secs().hash(&mut hasher);
    let h1 = hasher.finish();
    h1.hash(&mut hasher);
    let h2 = hasher.finish();
    format!(
        "{:08x}-{:04x}-4{:03x}-{:04x}-{:012x}",
        (h1 >> 32) as u32,
        (h1 >> 16) as u16,
        (h1 & 0xFFF) as u16,
        (0x8000 | (h2 & 0x3FFF)) as u16,
        (h2 >> 16) & 0xFFFF_FFFF_FFFF,
    )
}

fn now_unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::budget::ContextBudget;

    fn make_msg(role: &str, content: &str) -> serde_json::Value {
        serde_json::json!({"role": role, "content": content})
    }

    #[tokio::test]
    async fn prepare_messages_basic() {
        let config = HistoryManagerConfig::default();
        let manager = HistoryManager::new(config);
        let budget = ContextBudget::new(100_000, 4_000, Some("Be helpful"), &[]);

        let history = vec![
            make_msg("user", "Hello"),
            make_msg("assistant", "Hi there!"),
        ];
        let current = vec![make_msg("user", "What is 2+2?")];

        let result = manager
            .prepare_messages(
                &budget,
                Some("Be helpful"),
                &history,
                &current,
                &MemoryFilters::default(),
            )
            .await;

        // Should have: system + 2 history + 1 current = 4 messages
        assert_eq!(result.len(), 4, "result: {:?}", result);
        assert_eq!(result[0]["role"].as_str().unwrap(), "system");
    }

    #[tokio::test]
    async fn prepare_messages_trims_when_over_budget() {
        let config = HistoryManagerConfig::default();
        let manager = HistoryManager::new(config);
        // Very tight budget
        let budget = ContextBudget::new(100, 20, None, &[]);

        let history: Vec<serde_json::Value> = (0..20)
            .map(|i| {
                make_msg(
                    "user",
                    &format!("Message number {} with some padding text", i),
                )
            })
            .collect();
        let current = vec![make_msg("user", "Latest question")];

        let result = manager
            .prepare_messages(&budget, None, &history, &current, &MemoryFilters::default())
            .await;

        assert!(
            result.len() < history.len() + current.len(),
            "expected trimming, got {} messages",
            result.len()
        );
    }

    #[tokio::test]
    async fn on_turn_complete_with_summarization() {
        let config = HistoryManagerConfig {
            enable_summarization: true,
            ..Default::default()
        };
        let summarizer: Arc<dyn Summarizer> = Arc::new(ExtractiveSnippets::default());
        let mut manager = HistoryManager::new(config).with_summarizer(summarizer);

        let evicted = vec![
            make_msg("user", "What is the weather?"),
            make_msg("assistant", "The weather in SF is sunny and 72°F."),
        ];

        manager
            .on_turn_complete(&evicted, "test-agent", None, None)
            .await;

        assert_eq!(manager.summaries().len(), 1);
        assert!(!manager.summaries()[0].is_empty());
    }

    #[tokio::test]
    async fn on_turn_complete_no_op_without_evictions() {
        let config = HistoryManagerConfig {
            enable_summarization: true,
            ..Default::default()
        };
        let summarizer: Arc<dyn Summarizer> = Arc::new(ExtractiveSnippets::default());
        let mut manager = HistoryManager::new(config).with_summarizer(summarizer);

        manager
            .on_turn_complete(&[], "test-agent", None, None)
            .await;

        assert!(manager.summaries().is_empty());
    }

    #[test]
    fn extractive_snippets_basic() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let summarizer = ExtractiveSnippets::default();
        let msgs = vec![
            make_msg("user", "Tell me about Rust"),
            make_msg("assistant", "Rust is a systems programming language."),
        ];
        let summary = rt.block_on(summarizer.summarize(&msgs)).unwrap();
        assert!(!summary.is_empty());
        assert!(summary.contains("Rust"));
    }
}
