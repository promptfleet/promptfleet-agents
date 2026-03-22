//! # llm_context_core
//!
//! WASM-compatible LLM context window management library.
//!
//! Provides token estimation, context budget computation, pluggable history
//! management strategies, and trait ports for long-term memory backends.
//!
//! ## Architecture
//!
//! ```text
//! ┌──────────────────────────────────────────────────┐
//! │               LLM Context Window                  │
//! │  ┌──────────┐  ┌─────────────┐  ┌─────────────┐ │
//! │  │ System   │  │  Long-Term  │  │  Working    │ │
//! │  │ Prompt   │  │  Memories   │  │  Memory     │ │
//! │  │ (fixed)  │  │ (retrieved) │  │ (recent N)  │ │
//! │  └──────────┘  └─────────────┘  └─────────────┘ │
//! └──────────────────────────────────────────────────┘
//! ```
//!
//! - **Tier 1 (Working)**: Current turn messages, bounded by token budget
//! - **Tier 2 (Short-Term)**: Conversation history with sliding window / summarization
//! - **Tier 3 (Long-Term)**: Cross-conversation semantic retrieval (Qdrant, etc.)

pub mod tokens;
pub mod budget;
pub mod strategy;
pub mod history;
pub mod memory;

pub use budget::ContextBudget;
pub use history::{HistoryManager, HistoryManagerConfig};
pub use memory::{LongTermMemory, MemoryEntry, MemoryFilters, MemoryType};
pub use strategy::{ContextStrategy, ContextStrategyKind};
pub use tokens::estimate_tokens;
