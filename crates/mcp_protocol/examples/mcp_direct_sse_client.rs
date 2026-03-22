// examples/mcp_direct_sse_client.rs
//! MCP Direct SSE Client Example
//!
//! Demonstrates direct SSE connection to MCP server without proxy hop:
//! - **Direct Connection**: Client → MCP Server (SSE)
//! - **No Proxy**: Eliminates proxy hop for performance
//! - **Feature Flag**: Requires "sse-client" feature

#[cfg(feature = "sse-client")]
use mcp_protocol::McpClientBuilder;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 MCP Direct SSE Client Demo");
    println!("⚡ Direct connection - No proxy hop!");

    // 1. Direct SSE client configuration
    println!("\n🔧 Direct SSE Client Configuration:");

    #[cfg(feature = "sse-client")]
    {
        // Simple direct connection
        let _direct_client = McpClientBuilder::new()
            .with_sse_server("https://api.fastmcp.com/sse")
            .build();

        println!("✅ Direct SSE client (no auth)");

        // Direct connection with authentication
        let _auth_client = McpClientBuilder::new()
            .with_sse_server("https://api.openai.com/mcp/sse")
            .with_auth_token("your-api-key")
            .build();

        println!("✅ Direct SSE client (with auth)");
    }

    #[cfg(not(feature = "sse-client"))]
    {
        println!("❌ SSE client features not enabled");
        println!("💡 Run with: cargo run --example mcp_direct_sse_client --features sse-client");
    }

    // 2. Architecture comparison
    println!("\n📊 Architecture Comparison:");
    println!(
        "
🔄 PROXY APPROACH (Multiple Hops):
┌─────────────┐    HTTP/JSON-RPC    ┌─────────────┐    SSE/JSON-RPC    ┌─────────────┐
│   Client    │ ──────────────────► │ MCP Proxy   │ ──────────────────► │ MCP Server  │
│   (Agent)   │                     │ (Internal)  │                     │ (External)  │
└─────────────┘ ◄────────────────── └─────────────┘ ◄────────────────── └─────────────┘
    "
    );

    println!(
        "
⚡ DIRECT SSE APPROACH (Single Hop):
┌─────────────┐         SSE/JSON-RPC        ┌─────────────┐
│   Client    │ ──────────────────────────► │ MCP Server  │
│   (Agent)   │                             │ (Direct)    │
└─────────────┘ ◄────────────────────────── └─────────────┘
    "
    );

    // 3. Performance benefits
    println!("\n⚡ Performance Benefits:");
    println!("✅ 50% fewer network hops");
    println!("✅ Reduced latency (no proxy processing)");
    println!("✅ Lower memory usage (no proxy state)");
    println!("✅ Simplified debugging (direct connection)");
    println!("✅ Better for single-server scenarios");

    // 4. Usage examples
    println!("\n🎯 Usage Examples:");

    // Example 1: Direct tool listing
    println!(
        "
📋 Direct Tool Listing:
let client = McpClientBuilder::new()
    .with_sse_server(\"https://api.fastmcp.com/sse\")
    .with_auth_token(\"api-key\")
    .build();

let tools = client.list_tools_async().await?;
println!(\"Found {{}} tools\", tools.len());
    "
    );

    // Example 2: Direct tool calling
    println!(
        "
🔧 Direct Tool Calling:
let result = client.call_tool_async(\"web_search\", Some(json!({{
    \"query\": \"rust wasm spinkube\"
}}))).await?;

println!(\"Result: {{}}\", result.content[0].text);
    "
    );

    // Example 3: Health monitoring
    println!(
        "
🏥 Health Monitoring:
match client.health_check().await {{
    Ok(()) => println!(\"✅ Server healthy\"),
    Err(e) => println!(\"❌ Server unhealthy: {{}}\", e),
}}
    "
    );

    // 5. When to use direct vs proxy
    println!("\n🤔 When to Use Direct SSE vs Proxy:");

    println!("\n✅ Use Direct SSE Client When:");
    println!("  • Single MCP server connection");
    println!("  • Performance is critical");
    println!("  • Simple agent architectures");
    println!("  • Direct server access available");
    println!("  • Minimal latency required");

    println!("\n✅ Use Proxy When:");
    println!("  • Multiple MCP servers");
    println!("  • Complex routing logic");
    println!("  • Centralized authentication");
    println!("  • Load balancing needed");
    println!("  • Tool name conflict resolution");

    // 6. Feature flag configuration
    println!("\n🏗️ Feature Flag Configuration:");
    println!("
# Cargo.toml
[dependencies]
mcp_protocol = {{{{ path = \"../mcp_protocol\", features = [\"sse-client\"] }}}}

# Or enable all features
mcp_protocol = {{{{ path = \"../mcp_protocol\", features = [\"client\", \"server\", \"proxy\", \"sse-client\"] }}}}
    ");

    // 7. Error handling
    println!("\n🚨 Error Handling:");
    println!(
        "
// Direct SSE client provides detailed error information
match client.call_tool_async(\"invalid_tool\", None).await {{
    Ok(result) => println!(\"Success: {{:?}}\", result),
    Err(ProtocolError::Parsing(msg)) => println!(\"Parse error: {{}}\", msg),
    Err(ProtocolError::Transport(msg)) => println!(\"Transport error: {{}}\", msg),
    Err(e) => println!(\"Other error: {{:?}}\", e),
}}
    "
    );

    println!("\n🎉 Direct SSE Client Architecture Demonstrated!");
    println!("✅ Single hop: Client → MCP Server");
    println!("✅ Feature flagged: sse-client");
    println!("✅ Performance optimized: No proxy overhead");
    println!("✅ Clean API: Same pattern as proxy");
    println!("✅ WASM compatible: Async methods available");

    Ok(())
}
