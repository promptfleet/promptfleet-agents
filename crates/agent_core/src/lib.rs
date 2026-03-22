//! # Agent Core
//!
//! **Protocol-agnostic agent domain types.**
//!
//! This crate defines the universal vocabulary for agent interactions —
//! messages, content parts, task phases, and conversation context — without
//! any dependency on A2A, MCP, or any specific wire protocol.
//!
//! ## Design Principles
//!
//! - **Zero protocol dependencies**: only `serde` + `serde_json`
//! - **Minimal surface**: just the types every consumer needs
//! - **Conversion-friendly**: types designed for cheap `From`/`Into` mapping
//! - **WASM-compatible**: no I/O, no async, no platform-specific code
//!
//! ## Usage
//!
//! Consumers that only need to *describe* agent interactions (studio, coordination,
//! tests) depend on `agent_core` alone. The `agent_sdk` crate provides bidirectional
//! conversions between these types and protocol-specific types (A2A, MCP, etc.).
//!
//! ```rust
//! use agent_core::{AgentMessage, Role, ContentPart};
//!
//! // Simple text message — no protocol ceremony
//! let msg = AgentMessage::user_text("What's the weather?");
//!
//! // Structured content
//! let msg = AgentMessage::new(Role::User, vec![
//!     ContentPart::Text("Analyze this data".into()),
//!     ContentPart::Data(serde_json::json!({"rows": 42})),
//! ]);
//! ```

mod message;
mod task;

pub use message::*;
pub use task::*;
