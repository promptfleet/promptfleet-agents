//! Live smoke tests for llm_client against real provider APIs.
//!
//! All tests are `#[ignore]` — they only run when explicitly requested:
//!
//! ```bash
//! infisical run -- cargo test -p llm_client --test smoke_live -- --ignored --nocapture
//! ```
//!
//! Required env vars (injected by Infisical):
//! - `OPENROUTER_API_KEY` — for OpenAI-compatible tests via OpenRouter
//! - `CLAUDE_API_KEY` — for Anthropic tests

#![cfg(not(target_arch = "wasm32"))]

use futures::StreamExt;
use llm_client::{
    providers::{AnthropicClient, OpenAIClient},
    ClientConfig, LlmRequest, ChatMessage, StreamEvent,
};
use std::env;

fn openrouter_client() -> OpenAIClient {
    let api_key = env::var("OPENROUTER_API_KEY").expect("OPENROUTER_API_KEY must be set");
    OpenAIClient::new(ClientConfig {
        base_url: "https://openrouter.ai/api".to_string(),
        api_key: Some(api_key),
        ..ClientConfig::default()
    })
}

fn anthropic_client() -> AnthropicClient {
    let api_key = env::var("CLAUDE_API_KEY").expect("CLAUDE_API_KEY must be set");
    AnthropicClient::from_api_key("https://api.anthropic.com", &api_key)
}

fn hello_request(model: &str) -> LlmRequest {
    LlmRequest {
        model: model.to_string(),
        messages: vec![ChatMessage {
            role: "user".to_string(),
            content: Some("Say hello in exactly one word.".to_string()),
            ..Default::default()
        }],
        max_tokens: Some(32),
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------
// OpenRouter (OpenAI-compatible) — non-streaming
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore]
async fn smoke_openrouter_chat_non_streaming() {
    let client = openrouter_client();
    let req = hello_request("openai/gpt-4o-mini");

    let resp = client.llm(req).await.expect("OpenRouter chat request failed");

    eprintln!("[openrouter/non-stream] response: {resp:#?}");

    assert!(!resp.choices.is_empty(), "expected at least one choice");
    let text = resp.choices[0]
        .message
        .content
        .as_deref()
        .expect("expected content in response");
    assert!(!text.is_empty(), "expected non-empty content");
    eprintln!("[openrouter/non-stream] reply: {text}");
}

// ---------------------------------------------------------------------------
// OpenRouter (OpenAI-compatible) — streaming
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore]
async fn smoke_openrouter_chat_streaming() {
    let client = openrouter_client();
    let req = hello_request("openai/gpt-4o-mini");

    let mut stream = client.llm_stream(req).await.expect("OpenRouter stream failed");

    let mut got_start = false;
    let mut got_content = false;
    let mut got_done = false;
    let mut full_text = String::new();

    while let Some(event) = stream.next().await {
        let event = event.expect("stream event error");
        match &event {
            StreamEvent::StreamStart { .. } => got_start = true,
            StreamEvent::ContentDelta { delta } => {
                got_content = true;
                full_text.push_str(delta);
            }
            StreamEvent::Done { .. } => got_done = true,
            _ => {}
        }
        eprintln!("[openrouter/stream] {event:?}");
    }

    assert!(got_start, "expected StreamStart event");
    assert!(got_content, "expected at least one ContentDelta");
    assert!(got_done, "expected Done event");
    eprintln!("[openrouter/stream] full reply: {full_text}");
}

// ---------------------------------------------------------------------------
// Anthropic — non-streaming
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore]
async fn smoke_anthropic_non_streaming() {
    let client = anthropic_client();
    let req = hello_request("claude-sonnet-4-20250514");

    let resp = client.llm(req).await.expect("Anthropic chat request failed");

    eprintln!("[anthropic/non-stream] response: {resp:#?}");

    assert!(!resp.choices.is_empty(), "expected at least one choice");
    let text = resp.choices[0]
        .message
        .content
        .as_deref()
        .expect("expected content in response");
    assert!(!text.is_empty(), "expected non-empty content");
    eprintln!("[anthropic/non-stream] reply: {text}");
}

// ---------------------------------------------------------------------------
// Anthropic — streaming
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore]
async fn smoke_anthropic_streaming() {
    let client = anthropic_client();
    let req = hello_request("claude-sonnet-4-20250514");

    let mut stream = client.llm_stream(req).await.expect("Anthropic stream failed");

    let mut got_content = false;
    let mut got_done = false;
    let mut full_text = String::new();

    while let Some(event) = stream.next().await {
        let event = event.expect("stream event error");
        match &event {
            StreamEvent::ContentDelta { delta } => {
                got_content = true;
                full_text.push_str(delta);
            }
            StreamEvent::Done { .. } => got_done = true,
            _ => {}
        }
        eprintln!("[anthropic/stream] {event:?}");
    }

    assert!(got_content, "expected at least one ContentDelta");
    assert!(got_done, "expected Done event");
    eprintln!("[anthropic/stream] full reply: {full_text}");
}

// ---------------------------------------------------------------------------
// Anthropic — tool calling round-trip
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore]
async fn smoke_anthropic_tool_call() {
    use llm_client::ToolSchema;

    let client = anthropic_client();
    let req = LlmRequest {
        model: "claude-sonnet-4-20250514".to_string(),
        messages: vec![ChatMessage {
            role: "user".to_string(),
            content: Some("What is the weather in Paris?".to_string()),
            ..Default::default()
        }],
        tools: Some(vec![ToolSchema {
            name: "get_weather".to_string(),
            description: Some("Get current weather for a city".to_string()),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "city": { "type": "string", "description": "City name" }
                },
                "required": ["city"]
            }),
            strict: None,
        }]),
        max_tokens: Some(256),
        ..Default::default()
    };

    let resp = client.llm(req).await.expect("Anthropic tool-call request failed");

    eprintln!("[anthropic/tool-call] response: {resp:#?}");

    let tool_calls = &resp.tool_calls;
    assert!(
        tool_calls.is_some(),
        "expected tool_calls in response"
    );
    let calls = tool_calls.as_ref().unwrap();
    assert!(!calls.is_empty(), "expected at least one tool call");
    assert_eq!(calls[0].name, "get_weather");
    eprintln!(
        "[anthropic/tool-call] tool: {} args: {}",
        calls[0].name, calls[0].arguments
    );
}

// ---------------------------------------------------------------------------
// OpenRouter — tool calling round-trip
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore]
async fn smoke_openrouter_tool_call() {
    use llm_client::ToolSchema;

    let client = openrouter_client();
    let req = LlmRequest {
        model: "openai/gpt-4o-mini".to_string(),
        messages: vec![ChatMessage {
            role: "user".to_string(),
            content: Some("What is the weather in Paris?".to_string()),
            ..Default::default()
        }],
        tools: Some(vec![ToolSchema {
            name: "get_weather".to_string(),
            description: Some("Get current weather for a city".to_string()),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "city": { "type": "string", "description": "City name" }
                },
                "required": ["city"]
            }),
            strict: None,
        }]),
        max_tokens: Some(256),
        ..Default::default()
    };

    let resp = client.llm(req).await.expect("OpenRouter tool-call request failed");

    eprintln!("[openrouter/tool-call] response: {resp:#?}");

    let tool_calls = &resp.tool_calls;
    assert!(
        tool_calls.is_some(),
        "expected tool_calls in response"
    );
    let calls = tool_calls.as_ref().unwrap();
    assert!(!calls.is_empty(), "expected at least one tool call");
    assert_eq!(calls[0].name, "get_weather");
    eprintln!(
        "[openrouter/tool-call] tool: {} args: {}",
        calls[0].name, calls[0].arguments
    );
}
