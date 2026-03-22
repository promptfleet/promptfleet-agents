//! # A2A Protocol Core — v1.0.0
//!
//! Pure A2A (Agent-to-Agent) protocol domain logic, completely transport agnostic.

pub mod agent;
pub mod error;
pub mod protocol;
pub mod registry;
pub mod security;
pub mod transport;

#[cfg(feature = "protocol-core")]
pub mod data;

#[cfg(feature = "protocol-core")]
pub mod methods;

#[cfg(feature = "event-stream")]
pub mod streaming;

#[cfg(feature = "protocol-core")]
pub mod services;

pub use agent::*;
pub use error::*;
pub use protocol::*;
pub use registry::*;
pub use security::*;
pub use transport::*;

#[cfg(feature = "protocol-core")]
pub use data::*;

#[cfg(feature = "protocol-core")]
pub use methods::*;

/// A2A Protocol Version
pub const A2A_PROTOCOL_VERSION: &str = "1.0";

pub use protocol_transport_core::{
    error_codes as jsonrpc_error_codes, JsonRpcError, JsonRpcId, JsonRpcIncoming,
    JsonRpcNotification, JsonRpcRequest, JsonRpcResponse, JSONRPC_VERSION,
};
