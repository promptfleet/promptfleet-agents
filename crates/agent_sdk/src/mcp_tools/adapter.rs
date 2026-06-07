//! Outbound adapter: MCP tools → `agent_sdk::ToolSpec`.
//!
//! Converts MCP tool definitions from external servers into [`ToolSpec`] entries
//! that can be registered in a [`ToolRegistry`] and used by the LLM orchestrator.
//!
//! Tool naming convention: `mcp_<server_id>_<tool_name>`.
//! PromptFleet capability gateway tools are already globally scoped by the
//! gateway, so they keep their server-provided names.
//!
//! Underscores are used as separators (not dots) because LLM providers
//! like OpenAI restrict tool names to `^[a-zA-Z0-9_-]+$`.

use std::sync::Arc;

use crate::agent::tool_context::ToolContext;
use crate::agent::tools::{ToolExecutor, ToolKind, ToolRegistry, ToolSpec};
use crate::mcp_tools::config::ToolPolicy;
use crate::mcp_tools::error::McpToolError;
use crate::mcp_tools::types::{McpToolDescriptor, McpToolSource};

/// Adapter that converts MCP tools into ToolSpec entries for ToolRegistry.
pub struct McpToolAdapter;

const PROMPTFLEET_CAPABILITY_GATEWAY_SERVER_ID: &str = "promptfleet.capability_gateway";

impl McpToolAdapter {
    /// Connect to all configured MCP servers, list their tools, and register
    /// them in the given [`ToolRegistry`].
    ///
    /// Each tool is registered as `mcp.<server_id>.<tool_name>` with an async
    /// executor that calls the MCP server via the provided source.
    pub async fn register_mcp_tools(
        registry: &mut ToolRegistry,
        source: Arc<dyn McpToolSource>,
    ) -> Result<(), McpToolError> {
        let tools = source.list_all_tools().await?;

        for tool in &tools {
            let spec = Self::tool_to_spec(source.clone(), tool, &ToolPolicy::default());
            if let Some(spec) = spec {
                registry.register(spec);
            }
        }

        Ok(())
    }

    /// Connect and register with a per-server tool policy.
    pub async fn register_mcp_tools_with_policy(
        registry: &mut ToolRegistry,
        source: Arc<dyn McpToolSource>,
        policies: &std::collections::HashMap<String, ToolPolicy>,
    ) -> Result<(), McpToolError> {
        let tools = source.list_all_tools().await?;

        for tool in &tools {
            let policy = policies.get(&tool.server_id).cloned().unwrap_or_default();
            let spec = Self::tool_to_spec(source.clone(), tool, &policy);
            if let Some(spec) = spec {
                registry.register(spec);
            }
        }

        Ok(())
    }

    /// Convert a single MCP tool descriptor into a ToolSpec.
    ///
    /// Returns `None` if the tool is filtered out by the policy.
    pub fn tool_to_spec(
        source: Arc<dyn McpToolSource>,
        tool: &McpToolDescriptor,
        policy: &ToolPolicy,
    ) -> Option<ToolSpec> {
        // Apply exposure policy
        if !policy.is_exposed(&tool.name) {
            return None;
        }

        let server_id = tool.server_id.clone();
        let tool_name = tool.name.clone();
        // OpenAI requires tool names to match ^[a-zA-Z0-9_-]+$, so use
        // underscores instead of dots as separators.
        let canonical_name = canonical_tool_name(&server_id, &tool_name);

        let source_for_exec = source.clone();
        let sid_for_exec = server_id.clone();
        let tname_for_exec = tool_name.clone();

        Some(ToolSpec {
            name: canonical_name,
            description: tool.description.clone(),
            parameters: tool.input_schema.clone(),
            kind: ToolKind::Mcp,
            strict: false,
            parallel_ok: true,
            executor: ToolExecutor::WithContext(Arc::new(move |args, ctx: ToolContext| {
                let source = source_for_exec.clone();
                let sid = sid_for_exec.clone();
                let tname = tname_for_exec.clone();
                Box::pin(async move {
                    source
                        .call_tool_with_headers(&sid, &tname, args, Some(ctx.request_headers()))
                        .await
                        .map(|r| r.to_json())
                        .map_err(|e| e.to_string())
                })
            })),
        })
    }
}

fn canonical_tool_name(server_id: &str, tool_name: &str) -> String {
    if server_id == PROMPTFLEET_CAPABILITY_GATEWAY_SERVER_ID {
        return tool_name.to_string();
    }
    format!("mcp_{}_{}", server_id, tool_name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp_tools::types::{McpCallResult, McpContent};

    /// Mock MCP source for testing
    struct MockMcpSource {
        tools: Vec<McpToolDescriptor>,
    }

    #[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
    #[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
    impl McpToolSource for MockMcpSource {
        async fn list_tools(
            &self,
            server_id: &str,
        ) -> Result<Vec<McpToolDescriptor>, McpToolError> {
            Ok(self
                .tools
                .iter()
                .filter(|t| t.server_id == server_id)
                .cloned()
                .collect())
        }

        async fn call_tool(
            &self,
            _server_id: &str,
            _tool_name: &str,
            args: serde_json::Value,
        ) -> Result<McpCallResult, McpToolError> {
            Ok(McpCallResult {
                content: vec![McpContent::Text(
                    serde_json::json!({ "echo": args }).to_string(),
                )],
                is_error: false,
            })
        }

        async fn health_check(&self, _server_id: &str) -> Result<bool, McpToolError> {
            Ok(true)
        }

        fn server_ids(&self) -> Vec<String> {
            self.tools
                .iter()
                .map(|t| t.server_id.clone())
                .collect::<std::collections::HashSet<_>>()
                .into_iter()
                .collect()
        }
    }

    #[tokio::test]
    async fn register_mcp_tools_basic() {
        let source = Arc::new(MockMcpSource {
            tools: vec![
                McpToolDescriptor {
                    server_id: "tavily".into(),
                    name: "search".into(),
                    description: Some("Search the web".into()),
                    input_schema: serde_json::json!({
                        "type": "object",
                        "properties": { "query": { "type": "string" } }
                    }),
                },
                McpToolDescriptor {
                    server_id: "tavily".into(),
                    name: "extract".into(),
                    description: Some("Extract content from URL".into()),
                    input_schema: serde_json::json!({
                        "type": "object",
                        "properties": { "url": { "type": "string" } }
                    }),
                },
            ],
        });

        let mut registry = ToolRegistry::new();
        McpToolAdapter::register_mcp_tools(&mut registry, source)
            .await
            .unwrap();

        assert_eq!(registry.len(), 2);
        assert!(registry.get("mcp_tavily_search").is_some());
        assert!(registry.get("mcp_tavily_extract").is_some());

        let search = registry.get("mcp_tavily_search").unwrap();
        assert_eq!(search.description.as_deref(), Some("Search the web"));
        assert_eq!(search.kind, ToolKind::Mcp);
    }

    #[tokio::test]
    async fn register_with_policy_filtering() {
        let source = Arc::new(MockMcpSource {
            tools: vec![
                McpToolDescriptor {
                    server_id: "test".into(),
                    name: "allowed".into(),
                    description: None,
                    input_schema: serde_json::json!({}),
                },
                McpToolDescriptor {
                    server_id: "test".into(),
                    name: "denied".into(),
                    description: None,
                    input_schema: serde_json::json!({}),
                },
            ],
        });

        let mut policies = std::collections::HashMap::new();
        policies.insert(
            "test".into(),
            ToolPolicy {
                expose: vec!["allowed".into()],
                deny: vec![],
            },
        );

        let mut registry = ToolRegistry::new();
        McpToolAdapter::register_mcp_tools_with_policy(&mut registry, source, &policies)
            .await
            .unwrap();

        assert_eq!(registry.len(), 1);
        assert!(registry.get("mcp_test_allowed").is_some());
        assert!(registry.get("mcp_test_denied").is_none());
    }

    #[tokio::test]
    async fn mcp_tool_execution() {
        let source = Arc::new(MockMcpSource {
            tools: vec![McpToolDescriptor {
                server_id: "mock".into(),
                name: "echo".into(),
                description: None,
                input_schema: serde_json::json!({}),
            }],
        });

        let mut registry = ToolRegistry::new();
        McpToolAdapter::register_mcp_tools(&mut registry, source)
            .await
            .unwrap();

        let result = registry
            .execute("mcp_mock_echo", serde_json::json!({ "hello": "world" }))
            .await
            .unwrap();

        assert_eq!(result.name, "mcp_mock_echo");
        // The mock returns {"echo": args}
        assert_eq!(result.output["echo"]["hello"], "world");
    }

    #[test]
    fn capability_gateway_tools_keep_gateway_names() {
        assert_eq!(
            canonical_tool_name(
                PROMPTFLEET_CAPABILITY_GATEWAY_SERVER_ID,
                "toolconn_01ksh73kkw204at2rtvw7bv2s4__tavily_search"
            ),
            "toolconn_01ksh73kkw204at2rtvw7bv2s4__tavily_search"
        );
    }
}
