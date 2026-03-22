//! MCP servers configuration — compatible with the industry-standard `mcp.json` format
//! used by Cursor, Claude Desktop, VS Code, and other MCP clients.
//!
//! # Examples
//!
//! Stdio transport (local dev — native only):
//! ```json
//! {
//!   "mcp_servers": {
//!     "tavily": {
//!       "command": "npx",
//!       "args": ["-y", "tavily-mcp@latest"],
//!       "env": { "TAVILY_API_KEY": "${TAVILY_API_KEY}" }
//!     }
//!   }
//! }
//! ```
//!
//! Remote transport (production — WASM + native):
//! ```json
//! {
//!   "mcp_servers": {
//!     "tavily": {
//!       "url": "https://mcp.tavily.com/mcp/?tavilyApiKey=..."
//!     }
//!   }
//! }
//! ```

use serde::Deserialize;
use std::collections::HashMap;

use crate::mcp_tools::error::McpToolError;

/// Top-level MCP servers configuration.
///
/// Parses the industry-standard `mcp.json` format with optional PromptFleet
/// extensions (`tool_policy`, `auth`).
#[derive(Debug, Deserialize)]
pub struct McpServersConfig {
    /// Map of server_id → server entry
    pub mcp_servers: HashMap<String, McpServerEntry>,
}

/// A single MCP server entry.
///
/// Supports two transport patterns:
/// - **Stdio**: `command` + `args` + `env` (spawn child process — native only)
/// - **Remote**: `url` (connect via SSE or streamable HTTP — WASM + native)
#[derive(Debug, Deserialize)]
pub struct McpServerEntry {
    // ── Stdio transport (native only) ──
    /// Command to execute (e.g., "npx")
    #[serde(default)]
    pub command: Option<String>,

    /// Arguments to the command (e.g., ["-y", "tavily-mcp@latest"])
    #[serde(default)]
    pub args: Vec<String>,

    /// Environment variables for the spawned process
    #[serde(default)]
    pub env: HashMap<String, String>,

    // ── Remote transport (WASM + native) ──
    /// URL for remote MCP server (SSE or streamable HTTP)
    #[serde(default)]
    pub url: Option<String>,

    // ── Common fields ──
    /// Disable this server without removing config
    #[serde(default)]
    pub disabled: bool,

    /// Bearer token for remote transport authentication.
    /// Supports `${ENV_VAR}` interpolation.
    #[serde(default)]
    pub auth_token: Option<String>,

    /// Forward inbound caller Authorization to this MCP server when no static
    /// auth_token is configured.
    #[serde(default)]
    pub forward_caller_auth: bool,

    // ── PromptFleet extensions (optional, absent from standard mcp.json) ──
    /// Tool exposure policy (which tools the LLM can see/call)
    #[serde(default)]
    pub tool_policy: Option<ToolPolicy>,
}

/// Tool exposure policy for an MCP server.
///
/// Controls which tools from this server are visible to the LLM.
/// - Empty `expose` = expose all tools (default).
/// - Non-empty `expose` = only listed tools are visible.
/// - `deny` takes precedence over `expose`.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ToolPolicy {
    /// Tools to expose to the LLM (allowlist). Empty = expose all.
    #[serde(default)]
    pub expose: Vec<String>,

    /// Tools to deny (denylist, takes precedence over expose).
    #[serde(default)]
    pub deny: Vec<String>,
}

/// Transport type derived from config.
#[derive(Debug, PartialEq)]
pub enum McpTransportType {
    /// Spawn child process, JSON-RPC over stdin/stdout (native only)
    Stdio,
    /// Connect to remote URL via SSE or streamable HTTP (WASM + native)
    Remote,
    /// Invalid config (neither command nor url)
    Unknown,
}

impl McpServerEntry {
    /// Determine transport type from this entry's config.
    pub fn transport_type(&self) -> McpTransportType {
        if self.command.is_some() {
            McpTransportType::Stdio
        } else if self.url.is_some() {
            McpTransportType::Remote
        } else {
            McpTransportType::Unknown
        }
    }
}

impl ToolPolicy {
    /// Check whether a tool name is exposed by this policy.
    pub fn is_exposed(&self, tool_name: &str) -> bool {
        // Deny takes precedence
        if self.deny.iter().any(|d| d == tool_name) {
            return false;
        }
        // Empty expose = expose all
        if self.expose.is_empty() {
            return true;
        }
        self.expose.iter().any(|e| e == tool_name)
    }
}

impl McpServersConfig {
    /// Load config from a JSON file path.
    pub fn from_file(path: &str) -> Result<Self, McpToolError> {
        let content =
            std::fs::read_to_string(path).map_err(|e| McpToolError::ConfigError(e.to_string()))?;
        Self::from_json(&content)
    }

    /// Parse config from a JSON string.
    pub fn from_json(json: &str) -> Result<Self, McpToolError> {
        serde_json::from_str(json).map_err(|e| McpToolError::ConfigError(e.to_string()))
    }

    /// Iterate over enabled (non-disabled) servers.
    pub fn enabled_servers(&self) -> impl Iterator<Item = (&String, &McpServerEntry)> {
        self.mcp_servers.iter().filter(|(_, e)| !e.disabled)
    }
}

/// Resolve `${ENV_VAR}` patterns in a string value.
pub fn resolve_env_vars(value: &str) -> String {
    let mut result = value.to_string();
    // Simple ${VAR} pattern matching
    while let Some(start) = result.find("${") {
        if let Some(end) = result[start..].find('}') {
            let var_name = &result[start + 2..start + end];
            let replacement = std::env::var(var_name).unwrap_or_default();
            result = format!(
                "{}{}{}",
                &result[..start],
                replacement,
                &result[start + end + 1..]
            );
        } else {
            break;
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_stdio_config() {
        let json = r#"{
            "mcp_servers": {
                "tavily": {
                    "command": "npx",
                    "args": ["-y", "tavily-mcp@latest"],
                    "env": { "TAVILY_API_KEY": "test-key" }
                }
            }
        }"#;
        let config = McpServersConfig::from_json(json).unwrap();
        let tavily = &config.mcp_servers["tavily"];
        assert_eq!(tavily.command.as_deref(), Some("npx"));
        assert_eq!(tavily.args, vec!["-y", "tavily-mcp@latest"]);
        assert_eq!(tavily.transport_type(), McpTransportType::Stdio);
        assert!(!tavily.disabled);
    }

    #[test]
    fn parse_remote_config() {
        let json = r#"{
            "mcp_servers": {
                "tavily-remote": {
                    "url": "https://mcp.tavily.com/mcp/"
                }
            }
        }"#;
        let config = McpServersConfig::from_json(json).unwrap();
        let entry = &config.mcp_servers["tavily-remote"];
        assert_eq!(entry.transport_type(), McpTransportType::Remote);
        assert!(!entry.forward_caller_auth);
    }

    #[test]
    fn parse_disabled_server() {
        let json = r#"{
            "mcp_servers": {
                "disabled-server": {
                    "command": "npx",
                    "args": ["-y", "some-mcp"],
                    "disabled": true
                }
            }
        }"#;
        let config = McpServersConfig::from_json(json).unwrap();
        assert_eq!(config.enabled_servers().count(), 0);
    }

    #[test]
    fn tool_policy_defaults() {
        let policy = ToolPolicy::default();
        assert!(policy.is_exposed("any_tool"));
    }

    #[test]
    fn tool_policy_allowlist() {
        let policy = ToolPolicy {
            expose: vec!["search".into(), "extract".into()],
            deny: vec![],
        };
        assert!(policy.is_exposed("search"));
        assert!(policy.is_exposed("extract"));
        assert!(!policy.is_exposed("delete"));
    }

    #[test]
    fn tool_policy_deny_overrides_expose() {
        let policy = ToolPolicy {
            expose: vec!["search".into(), "dangerous".into()],
            deny: vec!["dangerous".into()],
        };
        assert!(policy.is_exposed("search"));
        assert!(!policy.is_exposed("dangerous"));
    }

    #[test]
    fn parse_remote_config_with_auth_token() {
        let json = r#"{
            "mcp_servers": {
                "private-mcp": {
                    "url": "https://mcp.example.com/v1",
                    "auth_token": "${MCP_AUTH_TOKEN}"
                }
            }
        }"#;
        let config = McpServersConfig::from_json(json).unwrap();
        let entry = &config.mcp_servers["private-mcp"];
        assert_eq!(entry.transport_type(), McpTransportType::Remote);
        assert_eq!(entry.auth_token.as_deref(), Some("${MCP_AUTH_TOKEN}"));
        assert!(!entry.forward_caller_auth);
    }

    #[test]
    fn resolve_env_vars_basic() {
        std::env::set_var("TEST_MCP_KEY", "resolved-value");
        let result = resolve_env_vars("prefix-${TEST_MCP_KEY}-suffix");
        assert_eq!(result, "prefix-resolved-value-suffix");
        std::env::remove_var("TEST_MCP_KEY");
    }

    #[test]
    fn resolve_env_vars_missing() {
        let result = resolve_env_vars("${NONEXISTENT_VAR_12345}");
        assert_eq!(result, "");
    }

    #[test]
    fn resolve_env_vars_no_pattern() {
        let result = resolve_env_vars("plain-string");
        assert_eq!(result, "plain-string");
    }
}
