//! # Provider-Agnostic Web Search Abstraction
//!
//! Defines a common contract for web search and content extraction that
//! any provider (Tavily, Exa, Brave, SearXNG, ...) can implement.
//!
//! ## Design Principles
//!
//! - **Provider-agnostic**: The trait exposes behaviour, not a vendor API.
//! - **Normalized results**: Every provider returns the same [`SearchResult`]
//!   / [`ExtractedContent`] shapes so callers never parse provider-specific JSON.
//! - **Tool-category aligned**: The two methods map directly to the
//!   `web_search` and `web_extract` tool categories emitted by the runtime.
//!
//! ## Usage (future)
//!
//! ```text
//! // In an extension crate (e.g. ext_web_search_tavily):
//! struct TavilyProvider { api_key: String }
//!
//! impl WebSearchProvider for TavilyProvider {
//!     async fn search(...) -> Result<SearchResponse, WebSearchError> { ... }
//!     async fn extract(...) -> Result<Vec<ExtractedContent>, WebSearchError> { ... }
//! }
//!
//! // In user code:
//! let provider = agent.get_provider::<dyn WebSearchProvider>()?;
//! let results = provider.search("weather in Joensuu", SearchOptions::default()).await?;
//! ```

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Result types (provider-agnostic, serializable)
// ---------------------------------------------------------------------------

/// A single search result item.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    /// Page title.
    pub title: String,
    /// Canonical URL.
    pub url: String,
    /// Short text snippet / summary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snippet: Option<String>,
    /// Provider-assigned relevance score (0.0–1.0), if available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
}

/// Response envelope for a search query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResponse {
    /// The original query string.
    pub query: String,
    /// Ordered list of results (most relevant first).
    pub items: Vec<SearchResult>,
    /// Wall-clock response time from the provider, in milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_time_ms: Option<u64>,
}

/// Options for a search request.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SearchOptions {
    /// Maximum number of results to return.
    #[serde(default)]
    pub max_results: Option<u32>,
    /// Search depth / thoroughness hint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depth: Option<SearchDepth>,
    /// Restrict results to a recent time window.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_range: Option<String>,
    /// Restrict to specific domains.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub include_domains: Vec<String>,
    /// Exclude specific domains.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclude_domains: Vec<String>,
}

/// Search depth / quality hint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchDepth {
    Fast,
    Basic,
    Advanced,
}

/// Extracted content from a single URL.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedContent {
    /// The URL that was extracted.
    pub url: String,
    /// Extracted text content (markdown or plain text).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Page title if available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

/// Options for a content extraction request.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExtractOptions {
    /// Output format preference.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    /// Use advanced extraction (tables, embedded content).
    #[serde(default)]
    pub advanced: bool,
}

/// Error type for web search / extract operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSearchError {
    pub code: String,
    pub message: String,
}

impl std::fmt::Display for WebSearchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)
    }
}

impl std::error::Error for WebSearchError {}

// ---------------------------------------------------------------------------
// Provider trait
// ---------------------------------------------------------------------------

/// Provider-agnostic web search and content extraction interface.
///
/// Implementations wrap a specific provider (Tavily, Exa, Brave, etc.)
/// and normalize their output into the common result types above.
///
/// The trait is object-safe so it can be stored in the agent's service
/// container via `Arc<dyn WebSearchProvider>`.
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait WebSearchProvider: Send + Sync {
    /// Execute a web search query and return normalized results.
    async fn search(
        &self,
        query: &str,
        opts: SearchOptions,
    ) -> Result<SearchResponse, WebSearchError>;

    /// Extract content from one or more URLs.
    async fn extract(
        &self,
        urls: &[String],
        opts: ExtractOptions,
    ) -> Result<Vec<ExtractedContent>, WebSearchError>;

    /// Provider name for logging / debugging (e.g. "tavily", "exa").
    fn provider_name(&self) -> &str;
}

// ---------------------------------------------------------------------------
// Tool category classification (shared with runtime)
// ---------------------------------------------------------------------------

/// Classify a tool into a behaviour category based on its name.
///
/// This is the canonical classification function used by both the SDK and
/// the studio runtime to assign `category` fields to tool events.
pub fn tool_category(tool_name: &str) -> &'static str {
    let lower = tool_name.to_lowercase();
    if lower.contains("search") {
        "web_search"
    } else if lower.contains("extract") || lower.contains("crawl") || lower.contains("scrape") {
        "web_extract"
    } else {
        "generic"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_category_classification() {
        // Web search providers
        assert_eq!(tool_category("mcp_tavily_tavily_search"), "web_search");
        assert_eq!(tool_category("mcp_exa_search"), "web_search");
        assert_eq!(tool_category("mcp_brave_web_search"), "web_search");
        assert_eq!(tool_category("web_search"), "web_search");

        // Web extract providers
        assert_eq!(tool_category("mcp_tavily_tavily_extract"), "web_extract");
        assert_eq!(tool_category("mcp_firecrawl_crawl"), "web_extract");
        assert_eq!(tool_category("scrape_page"), "web_extract");

        // Generic tools
        assert_eq!(tool_category("mcp_postgres_query"), "generic");
        assert_eq!(tool_category("get_weather"), "generic");
        assert_eq!(tool_category("calculate"), "generic");
    }

    #[test]
    fn test_search_result_serialization() {
        let response = SearchResponse {
            query: "test query".to_string(),
            items: vec![SearchResult {
                title: "Test".to_string(),
                url: "https://example.com".to_string(),
                snippet: Some("A test result".to_string()),
                score: Some(0.95),
            }],
            response_time_ms: Some(150),
        };
        let json = serde_json::to_string(&response).unwrap();
        let parsed: SearchResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.query, "test query");
        assert_eq!(parsed.items.len(), 1);
        assert_eq!(parsed.items[0].url, "https://example.com");
    }
}
