//! # ToolEngine — protocol-agnostic tool-augmented LLM execution core.
//!
//! The engine is the single source of truth for the SDK's tool-calling
//! loop. It takes an LLM turn invoker, a set of tools, and configuration
//! — then runs the iterative LLM→tools→LLM cycle until the model produces
//! a final text response, a sentinel tool signals stop, or a safety gate
//! triggers.
//!
//! ## Architecture
//!
//! ```text
//! ┌──────────────────────────────────────────────────────────┐
//! │ engine::core_loop::execute<F>()                          │
//! │   invoker: &dyn LlmTurnInvoker  (streaming or sync)     │
//! │   on_event: &F  (Fn(AgentTraceEvent) callback)           │
//! │   → EngineResult { text, usage, stop_signal }            │
//! └────────────────────────┬─────────────────────────────────┘
//! ┌────────────────────────┼──────────────────────────────┐
//! │                        │                              │
//! │  StreamingTurnInvoker  │  RequestResponseTurnInvoker  │
//! │  (native, LlmStream-  │  (WASM+native, LlmInvoker)   │
//! │   Invoker, emits       │  parses JSON response        │
//! │   deltas via sink)     │                              │
//! └────────────────────────┴──────────────────────────────┘
//! ```
//!
//! Adapters wrap the engine for their respective protocols:
//! - **`ToolEngine`** (native): `run_text`, `run_stream` convenience API
//! - **Compatibility adapter**: `execute_a2a` — runtime execution plus protocol wrapping
//! - **Studio runtime**: protocol-agnostic `run_tools_loop_agnostic`

pub(crate) mod core_loop;
pub(crate) mod invokers;
#[cfg(feature = "test-support")]
pub mod test_support;
mod types;

#[cfg(test)]
mod tests;

// Universal types (WASM + native)
pub use types::{
    EngineConfig, EngineError, EngineResult, LlmTurnInvoker, ToolCallInfo, TurnFuture, TurnResult,
};

// Native-only types
#[cfg(not(target_arch = "wasm32"))]
pub use types::{ToolEngine, ToolEngineBuilder};

// Re-export invokers for adapter use
pub use invokers::RequestResponseTurnInvoker;
#[cfg(not(target_arch = "wasm32"))]
pub use invokers::StreamingTurnInvoker;
