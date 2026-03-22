//! Long-term memory trait ports.
//!
//! Defines the interface for cross-conversation semantic memory backends
//! (e.g. Qdrant + embeddings). This module contains only trait definitions
//! and types — implementations live in `agent_memory_store`.
//!
//! WASM-compatible: no async runtime dependencies.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

/// Type of memory entry — helps with filtering and relevance scoring.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryType {
    /// Summarized conversation segment.
    Summary,
    /// Extracted factual statement (e.g. "User prefers dark mode").
    Fact,
    /// User instruction or preference.
    Instruction,
    /// Compressed tool result worth remembering.
    ToolResult,
    /// Arbitrary user-defined type.
    Custom(String),
}

/// A single memory entry stored in the long-term memory backend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    /// Unique ID for this memory entry.
    pub id: String,
    /// Agent that created this memory.
    pub agent_id: String,
    /// User this memory belongs to (for multi-tenant isolation).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    /// Conversation this memory was extracted from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    /// The text content to embed and store.
    pub content: String,
    /// Classification of this memory.
    pub memory_type: MemoryType,
    /// Unix timestamp (seconds) when this memory was created.
    pub timestamp: u64,
    /// Relevance score (set during retrieval, 0.0 to 1.0).
    #[serde(default)]
    pub score: f32,
    /// Arbitrary metadata (e.g. source turn number, tags).
    #[serde(default)]
    pub metadata: HashMap<String, serde_json::Value>,
}

/// Filters for memory retrieval.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MemoryFilters {
    /// Filter by agent ID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    /// Filter by user ID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    /// Filter by conversation ID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    /// Filter by memory type(s).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub memory_types: Vec<MemoryType>,
    /// Only return memories newer than this timestamp (seconds).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_timestamp: Option<u64>,
}

/// Boxed future type for WASM compatibility (no Send bound on wasm32).
#[cfg(target_arch = "wasm32")]
pub type MemoryFuture<T> = Pin<Box<dyn Future<Output = Result<T, String>>>>;

/// Boxed future type for native (requires Send).
#[cfg(not(target_arch = "wasm32"))]
pub type MemoryFuture<T> = Pin<Box<dyn Future<Output = Result<T, String>> + Send>>;

/// Trait port for long-term memory backends.
///
/// Implementations handle embedding, storage, and semantic retrieval.
/// The trait is object-safe and WASM-compatible.
///
/// # Implementations
///
/// - `agent_memory_store::QdrantMemoryStore` — Qdrant + embedding_provider_lib
/// - In-memory store for testing
pub trait LongTermMemory: Send + Sync {
    /// Store a memory entry (embedding + indexing happens internally).
    fn store(&self, entry: MemoryEntry) -> MemoryFuture<()>;

    /// Recall relevant memories by semantic similarity to a query.
    ///
    /// Returns up to `top_k` entries sorted by relevance (highest first).
    fn recall(
        &self,
        query: &str,
        top_k: usize,
        filters: MemoryFilters,
    ) -> MemoryFuture<Vec<MemoryEntry>>;

    /// Delete memories matching the given filters.
    ///
    /// Returns the number of entries deleted.
    fn forget(&self, filters: MemoryFilters) -> MemoryFuture<u64>;
}

/// No-op memory backend (used when long-term memory is disabled).
pub struct NoOpMemory;

impl LongTermMemory for NoOpMemory {
    fn store(&self, _entry: MemoryEntry) -> MemoryFuture<()> {
        Box::pin(async { Ok(()) })
    }

    fn recall(
        &self,
        _query: &str,
        _top_k: usize,
        _filters: MemoryFilters,
    ) -> MemoryFuture<Vec<MemoryEntry>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn forget(&self, _filters: MemoryFilters) -> MemoryFuture<u64> {
        Box::pin(async { Ok(0) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_entry_serialization() {
        let entry = MemoryEntry {
            id: "mem-001".to_string(),
            agent_id: "weather-agent".to_string(),
            user_id: Some("user-42".to_string()),
            conversation_id: Some("conv-99".to_string()),
            content: "User prefers Celsius for temperature".to_string(),
            memory_type: MemoryType::Fact,
            timestamp: 1700000000,
            score: 0.95,
            metadata: HashMap::new(),
        };
        let json = serde_json::to_string(&entry).unwrap();
        let deser: MemoryEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(deser.id, "mem-001");
        assert_eq!(deser.memory_type, MemoryType::Fact);
    }

    #[test]
    fn memory_filters_default() {
        let f = MemoryFilters::default();
        assert!(f.agent_id.is_none());
        assert!(f.memory_types.is_empty());
    }

    #[tokio::test]
    async fn noop_memory() {
        let mem = NoOpMemory;
        let entry = MemoryEntry {
            id: "test".to_string(),
            agent_id: "a".to_string(),
            user_id: None,
            conversation_id: None,
            content: "test".to_string(),
            memory_type: MemoryType::Summary,
            timestamp: 0,
            score: 0.0,
            metadata: HashMap::new(),
        };
        assert!(mem.store(entry).await.is_ok());
        let results = mem
            .recall("anything", 5, MemoryFilters::default())
            .await
            .unwrap();
        assert!(results.is_empty());
        let deleted = mem.forget(MemoryFilters::default()).await.unwrap();
        assert_eq!(deleted, 0);
    }
}
