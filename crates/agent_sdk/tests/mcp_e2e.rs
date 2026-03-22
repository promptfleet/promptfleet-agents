//! End-to-end MCP integration test against a live Tavily MCP server.
//!
//! This test is **ignored by default** — it requires:
//! - `TAVILY_API_KEY` environment variable
//! - `npx` available in PATH
//! - Network access to npm registry (first run downloads tavily-mcp)
//!
//! Run explicitly:
//! ```bash
//! TAVILY_API_KEY=tvly-... cargo test -p agent_sdk --features mcp-e2e --test mcp_e2e -- --ignored
//! ```

#[cfg(feature = "mcp-e2e")]
mod e2e {
    use agent_sdk::agent::tools::ToolRegistry;
    use agent_sdk::mcp_tools::{McpServersConfig, McpToolAdapter, McpToolSource, NativeMcpBackend};
    use std::sync::Arc;

    fn tavily_key() -> Option<String> {
        std::env::var("TAVILY_API_KEY")
            .ok()
            .filter(|k| !k.is_empty())
    }

    fn tavily_config(api_key: &str) -> McpServersConfig {
        let json = format!(
            r#"{{
                "mcp_servers": {{
                    "tavily": {{
                        "command": "npx",
                        "args": ["-y", "tavily-mcp@0.2.15"],
                        "env": {{ "TAVILY_API_KEY": "{api_key}" }}
                    }}
                }}
            }}"#
        );
        McpServersConfig::from_json(&json).expect("config should parse")
    }

    // ── Test 1: Connect + list tools ────────────────────────────────────────

    #[tokio::test]
    #[ignore = "requires TAVILY_API_KEY and npx"]
    async fn tavily_connect_and_list_tools() {
        let api_key = tavily_key().expect("TAVILY_API_KEY must be set");
        let config = tavily_config(&api_key);

        let backend = NativeMcpBackend::connect(&config)
            .await
            .expect("should connect to Tavily MCP server");

        // Should have exactly one server
        let ids = backend.server_ids();
        assert_eq!(ids.len(), 1, "expected 1 server, got {:?}", ids);
        assert!(ids.contains(&"tavily".to_string()));

        // List tools
        let tools = backend
            .list_tools("tavily")
            .await
            .expect("should list tools");

        println!("Tavily tools ({}):", tools.len());
        for t in &tools {
            println!(
                "  - {} : {}",
                t.name,
                t.description.as_deref().unwrap_or("(no description)")
            );
        }

        // Tavily MCP server should expose at least tavily_search
        assert!(!tools.is_empty(), "Tavily should expose at least 1 tool");

        let has_search = tools.iter().any(|t| t.name.contains("search"));
        assert!(
            has_search,
            "Expected a search tool, found: {:?}",
            tools.iter().map(|t| &t.name).collect::<Vec<_>>()
        );
    }

    // ── Test 2: Call tavily search ──────────────────────────────────────────

    #[tokio::test]
    #[ignore = "requires TAVILY_API_KEY and npx"]
    async fn tavily_search_call() {
        let api_key = tavily_key().expect("TAVILY_API_KEY must be set");
        let config = tavily_config(&api_key);

        let backend = NativeMcpBackend::connect(&config)
            .await
            .expect("should connect to Tavily MCP server");

        // Find the search tool name
        let tools = backend
            .list_tools("tavily")
            .await
            .expect("should list tools");

        let search_tool = tools
            .iter()
            .find(|t| t.name.contains("search"))
            .expect("should have a search tool");

        println!("Using tool: {}", search_tool.name);

        // Call the search tool
        let result = backend
            .call_tool(
                "tavily",
                &search_tool.name,
                serde_json::json!({
                    "query": "what is the MCP protocol by Anthropic"
                }),
            )
            .await
            .expect("search tool call should succeed");

        assert!(!result.is_error, "search should not return error");
        assert!(!result.content.is_empty(), "search should return content");

        // Print first content item
        let json = result.to_json();
        let pretty = serde_json::to_string_pretty(&json).unwrap();
        // Truncate for readability
        let truncated = if pretty.len() > 2000 {
            format!("{}...(truncated)", &pretty[..2000])
        } else {
            pretty
        };
        println!("Search result:\n{truncated}");
    }

    // ── Test 3: Full pipeline — config → connect → register → execute via ToolRegistry ──

    #[tokio::test]
    #[ignore = "requires TAVILY_API_KEY and npx"]
    async fn tavily_full_pipeline_through_tool_registry() {
        let api_key = tavily_key().expect("TAVILY_API_KEY must be set");
        let config = tavily_config(&api_key);

        let backend = NativeMcpBackend::connect(&config)
            .await
            .expect("should connect to Tavily MCP server");

        let source: Arc<dyn McpToolSource> = Arc::new(backend);

        // Register MCP tools into ToolRegistry (same path as Studio main.rs)
        let mut registry = ToolRegistry::new();
        McpToolAdapter::register_mcp_tools(&mut registry, source)
            .await
            .expect("should register MCP tools");

        println!("Registered {} tool(s) in ToolRegistry:", registry.len());
        for tool in registry.list() {
            println!("  - {}", tool.name);
        }

        assert!(!registry.is_empty(), "registry should have tools");

        // Find the search tool (should be mcp.tavily.<search_tool_name>)
        let search_name = registry
            .list()
            .iter()
            .find(|t| t.name.contains("search"))
            .map(|t| t.name.clone())
            .expect("should have a search tool in registry");

        println!("Executing tool via ToolRegistry: {search_name}");

        // Execute through ToolRegistry (same path the LLM orchestrator uses)
        let result = registry
            .execute(
                &search_name,
                serde_json::json!({
                    "query": "Rust WebAssembly SpinKube serverless"
                }),
            )
            .await
            .expect("tool execution should succeed");

        println!("Tool execution result name: {}", result.name);
        assert_eq!(result.name, search_name);

        // The output should be valid JSON (not an error)
        assert!(
            result.output.get("error").is_none(),
            "tool should not return error: {:?}",
            result.output
        );

        let pretty = serde_json::to_string_pretty(&result.output).unwrap();
        let truncated = if pretty.len() > 1500 {
            format!("{}...(truncated)", &pretty[..1500])
        } else {
            pretty
        };
        println!("ToolRegistry execution result:\n{truncated}");
    }

    // ── Test 4: Health check ────────────────────────────────────────────────

    #[tokio::test]
    #[ignore = "requires TAVILY_API_KEY and npx"]
    async fn tavily_health_check() {
        let api_key = tavily_key().expect("TAVILY_API_KEY must be set");
        let config = tavily_config(&api_key);

        let backend = NativeMcpBackend::connect(&config)
            .await
            .expect("should connect to Tavily MCP server");

        let healthy = backend
            .health_check("tavily")
            .await
            .expect("health check should not fail");

        assert!(healthy, "Tavily server should be healthy");

        // Non-existent server should return error
        let missing = backend.health_check("nonexistent").await;
        assert!(missing.is_err(), "non-existent server should error");
    }
}
