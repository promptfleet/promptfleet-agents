//! # A2A HTTP Server
//!
//! **A2A HTTP Server** with WASM default and native testing support
//!
//! ✅ **Target Adaptive**: WASM (Spin SDK) + Native (Axum) with identical APIs
//! ✅ **Complete Feature Parity**: All A2A protocol methods work identically
//! ✅ **Testing Excellence**: Native HTTP server for comprehensive testing
//! ✅ **Clean Architecture**: Protocol instance injection, no global state
//! ✅ **Comprehensive Logging**: Debug, info, warn, error levels with agent context

use log::{debug, info};

/// A2A v1.0 JSON-RPC method names.
pub mod method {
    pub const PING: &str = "Ping";
    pub const SEND_MESSAGE: &str = "SendMessage";
    pub const SEND_STREAMING_MESSAGE: &str = "SendStreamingMessage";
    pub const GET_AGENT_CARD: &str = "GetAgentCard";
    pub const GET_EXTENDED_AGENT_CARD: &str = "GetExtendedAgentCard";
    pub const GET_TASK: &str = "GetTask";
    pub const CANCEL_TASK: &str = "CancelTask";
    pub const LIST_TASKS: &str = "ListTasks";
}

// Target-specific implementations
#[cfg(target_arch = "wasm32")]
mod wasm_server;
#[cfg(target_arch = "wasm32")]
use wasm_server as implementation;

#[cfg(not(target_arch = "wasm32"))]
mod native_server;
#[cfg(not(target_arch = "wasm32"))]
use native_server as implementation;

// Unified public interface (identical for both targets)
pub use implementation::A2AHttpServer;

// Re-export core types for convenience
pub use a2a_protocol_core::{A2A_PROTOCOL_VERSION, A2AProtocol, AgentCard};
pub use protocol_transport_core::{JSONRPC_VERSION, JsonRpcIncoming, JsonRpcResponse};

// Adapter trait for delegating selected methods to application layer (SDK)
pub use a2a_app_ports::A2AAppPort;

// Streaming adapter trait (feature-gated)
#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
pub use implementation::A2AStreamingAppPort;

/// Initialize logging for A2A HTTP Server
///
/// This is a convenience function that can be called by applications
/// to ensure proper logging is configured for the A2A HTTP server.
///
/// # Arguments
/// * `component_name` - Name of the component (typically the agent name)
///
/// # Example
/// ```
/// a2a_http_server::init_logging("my-agent");
/// ```
pub fn init_logging(component_name: &str) {
    info!(
        "Initializing A2A HTTP Server logging for component: {}",
        component_name
    );
    debug!(
        "A2A HTTP Server v{} logging initialized",
        env!("CARGO_PKG_VERSION")
    );
}

/// Log server creation with target information
pub fn log_server_info(agent_id: &str) {
    #[cfg(target_arch = "wasm32")]
    info!(
        "Created A2A HTTP Server (WASM/Spin) for agent: {}",
        agent_id
    );

    #[cfg(not(target_arch = "wasm32"))]
    info!(
        "Created A2A HTTP Server (Native/Axum) for agent: {}",
        agent_id
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_creation() {
        let agent_card = AgentCard::new("test-agent".to_string());
        let server = A2AHttpServer::new_with_a2a_methods(agent_card);
        assert_eq!(server.agent_id(), "test-agent");
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn test_native_server_basic_functionality() {
        let agent_card = AgentCard::new("test-agent".to_string());
        let server = A2AHttpServer::new_with_a2a_methods(agent_card);
        assert!(server.can_serve());
    }
}
