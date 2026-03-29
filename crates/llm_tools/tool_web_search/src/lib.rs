//! # tool_web_search
//!
//! Reusable LLM tool that performs web search via the Tavily Search API.
//! Returns an `agent_sdk::agent::ToolSpec` ready to register with any agent's
//! `ToolRegistry`.
//!
//! ## Usage
//!
//! ```rust,ignore
//! let search_tool = tool_web_search::tool_spec(api_key, api_url);
//! sdk_registry.register(search_tool);
//! ```
//!
//! The tool is WASM-compatible (uses `spin_sdk::http::send` on wasm32,
//! `reqwest` on native for testing).
//!
//! ## Dependency note
//!
//! This crate lives under `llm_tools/` but depends on `agent_sdk` (not
//! `llm_tools` itself). The layering is:
//!   `tool_web_search` → `agent_sdk` → `llm_tools`
//!
//! There is no Cargo-level cycle because `llm_tools` does not depend on
//! this crate, but the physical nesting is misleading. A future refactor
//! should move `tool_web_search` to a top-level `crates/tool_web_search`
//! to clarify the actual dependency direction.

use agent_sdk::agent::tools::{ToolExecutor, ToolSpec};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

// ── Tool metadata ──────────────────────────────────────────────────────

pub const NAME: &str = "web_search";
pub const DESCRIPTION: &str = "Search the web for current, real-time information on any topic. \
     Returns relevant results with titles, URLs, and content snippets. \
     Use this when you need up-to-date facts, data, or verification \
     beyond your training knowledge.";

// ── Tavily request/response types ──────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct WebSearchArgs {
    /// The search query string
    pub query: String,
    /// Number of results to return (1-10, default 5)
    #[serde(default = "default_max_results")]
    pub max_results: u8,
    /// Search depth: "basic" (fast) or "advanced" (thorough)
    #[serde(default = "default_search_depth")]
    pub search_depth: String,
}

fn default_max_results() -> u8 {
    5
}
fn default_search_depth() -> String {
    "basic".to_string()
}

#[derive(Debug, Serialize, Deserialize)]
struct TavilyRequest {
    api_key: String,
    query: String,
    search_depth: String,
    max_results: u8,
    include_answer: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct TavilyResponse {
    #[serde(default)]
    query: String,
    #[serde(default)]
    answer: Option<String>,
    #[serde(default)]
    results: Vec<TavilyResult>,
}

#[derive(Debug, Serialize, Deserialize)]
struct TavilyResult {
    title: String,
    url: String,
    content: String,
    #[serde(default)]
    score: f64,
}

// ── Parameters JSON Schema ─────────────────────────────────────────────

pub fn parameters_schema() -> serde_json::Value {
    let schema = schemars::schema_for!(WebSearchArgs);
    serde_json::to_value(schema).unwrap_or_else(|_| {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "The search query string" },
                "max_results": { "type": "integer", "description": "Number of results (1-10, default 5)", "default": 5 },
                "search_depth": { "type": "string", "description": "basic or advanced", "default": "basic" }
            },
            "required": ["query"]
        })
    })
}

// ── Core execution (async, dual-target) ────────────────────────────────

/// Execute web search against Tavily API.
///
/// This is the core async function. On WASM it uses `spin_sdk::http::send`,
/// on native it uses `reqwest`.
pub async fn execute(
    api_key: &str,
    api_url: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let parsed: WebSearchArgs =
        serde_json::from_value(args).map_err(|e| format!("Invalid arguments: {e}"))?;

    if parsed.query.trim().is_empty() {
        return Err("Search query cannot be empty".to_string());
    }

    let max_results = parsed.max_results.clamp(1, 10);
    let search_depth = match parsed.search_depth.as_str() {
        "advanced" => "advanced",
        _ => "basic",
    };

    let tavily_req = TavilyRequest {
        api_key: api_key.to_string(),
        query: parsed.query.clone(),
        search_depth: search_depth.to_string(),
        max_results,
        include_answer: true,
    };

    let body_json =
        serde_json::to_string(&tavily_req).map_err(|e| format!("Serialization error: {e}"))?;

    let url = format!("{}/search", api_url.trim_end_matches('/'));

    log::debug!("web_search: query='{}' url={}", parsed.query, url);

    let response_body = http_post(&url, &body_json).await?;

    let tavily_resp: TavilyResponse =
        serde_json::from_str(&response_body).map_err(|e| format!("Parse error: {e}"))?;

    // Build structured output for the LLM
    let results: Vec<serde_json::Value> = tavily_resp
        .results
        .iter()
        .map(|r| {
            serde_json::json!({
                "title": r.title,
                "url": r.url,
                "content": r.content,
                "relevance_score": r.score,
            })
        })
        .collect();

    Ok(serde_json::json!({
        "query": tavily_resp.query,
        "answer": tavily_resp.answer,
        "results": results,
        "result_count": results.len(),
    }))
}

// ── WASM HTTP implementation ───────────────────────────────────────────

#[cfg(target_arch = "wasm32")]
async fn http_post(url: &str, body: &str) -> Result<String, String> {
    use spin_sdk::http::{Method, Request as SpinRequest, Response as SpinResponse};

    let req = SpinRequest::builder()
        .method(Method::Post)
        .uri(url)
        .header("Content-Type", "application/json")
        .body(body.to_string())
        .build();

    let resp: SpinResponse = spin_sdk::http::send(req)
        .await
        .map_err(|e| format!("HTTP request failed: {e}"))?;

    let status = *resp.status();
    let body_bytes = resp.body().to_vec();
    let body_str =
        String::from_utf8(body_bytes).map_err(|e| format!("Invalid UTF-8 response: {e}"))?;

    if status >= 400 {
        return Err(format!(
            "Tavily API error (HTTP {status}): {}",
            truncate(&body_str, 500)
        ));
    }

    Ok(body_str)
}

// ── Native HTTP implementation (for testing) ───────────────────────────

#[cfg(not(target_arch = "wasm32"))]
async fn http_post(url: &str, body: &str) -> Result<String, String> {
    let client = reqwest::Client::new();
    let resp = client
        .post(url)
        .header("Content-Type", "application/json")
        .body(body.to_string())
        .send()
        .await
        .map_err(|e| format!("HTTP request failed: {e}"))?;

    let status = resp.status().as_u16();
    let body_str = resp
        .text()
        .await
        .map_err(|e| format!("Failed to read response: {e}"))?;

    if status >= 400 {
        return Err(format!(
            "Tavily API error (HTTP {status}): {}",
            truncate(&body_str, 500)
        ));
    }

    Ok(body_str)
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        s
    } else {
        &s[..s.floor_char_boundary(max)]
    }
}

// ── One-liner ToolSpec factory ─────────────────────────────────────────

/// Create a `ToolSpec` for the web search tool, ready to register with
/// any agent's `ToolRegistry`.
///
/// ```rust,ignore
/// let search_tool = tool_web_search::tool_spec(api_key, api_url);
/// my_registry.register(search_tool);
/// ```
pub fn tool_spec(api_key: String, api_url: String) -> ToolSpec {
    ToolSpec {
        name: NAME.to_string(),
        description: Some(DESCRIPTION.to_string()),
        parameters: parameters_schema(),
        strict: true,
        parallel_ok: false,
        executor: ToolExecutor::Simple(Arc::new(move |args| {
            let key = api_key.clone();
            let url = api_url.clone();
            Box::pin(async move { execute(&key, &url, args).await })
        })),
    }
}

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parameters_schema_is_valid() {
        let schema = parameters_schema();
        assert!(schema.is_object(), "Schema should be an object");
        // schemars wraps in a root object with "properties" or "$schema"
        let props = schema.get("properties").or_else(|| {
            schema
                .get("definitions")
                .and_then(|d| d.get("WebSearchArgs"))
                .and_then(|s| s.get("properties"))
        });
        assert!(props.is_some(), "Schema should have properties");
    }

    #[test]
    fn test_tool_spec_name_and_description() {
        let spec = tool_spec("test-key".into(), "https://api.tavily.com".into());
        assert_eq!(spec.name, "web_search");
        assert!(spec.description.is_some());
    }

    #[test]
    fn test_parse_args_valid() {
        let args = serde_json::json!({"query": "rust wasm"});
        let parsed: Result<WebSearchArgs, _> = serde_json::from_value(args);
        assert!(parsed.is_ok());
        let p = parsed.unwrap();
        assert_eq!(p.query, "rust wasm");
        assert_eq!(p.max_results, 5);
        assert_eq!(p.search_depth, "basic");
    }

    #[test]
    fn test_parse_args_with_options() {
        let args = serde_json::json!({
            "query": "WASM runtime benchmarks",
            "max_results": 10,
            "search_depth": "advanced"
        });
        let parsed: WebSearchArgs = serde_json::from_value(args).unwrap();
        assert_eq!(parsed.max_results, 10);
        assert_eq!(parsed.search_depth, "advanced");
    }

    #[tokio::test]
    async fn test_execute_empty_query_rejected() {
        let result = execute(
            "key",
            "http://localhost:9999",
            serde_json::json!({"query": ""}),
        )
        .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("empty"));
    }

    #[tokio::test]
    async fn test_execute_invalid_args_rejected() {
        let result = execute("key", "http://localhost:9999", serde_json::json!(42)).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid arguments"));
    }
}
