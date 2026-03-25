# `llm_client`

Provider-agnostic HTTP client for chat LLMs: **typed** [`LlmRequest`](./src/types.rs) / [`LlmResponse`](./src/types.rs), SSE streaming as [`StreamEvent`](./src/stream.rs), model [**profiles**](./src/profile.rs) and [**prepare**](./src/prepare.rs) pipelines, and a single entry point [**`LlmClient`**](./src/client.rs).

## Quick start

```rust
use llm_client::auth::ApiKeyAuth;
use llm_client::client::{LlmClient, WireFormat};
use llm_client::{ChatMessage, LlmRequest};

# async fn example() -> Result<(), llm_client::LlmError> {
let client = LlmClient::builder(WireFormat::OpenAiCompat)
    .base_url("https://api.openai.com/v1")
    .auth(ApiKeyAuth::new(std::env::var("OPENAI_API_KEY").unwrap()))
    .build()?;

let response = client
    .chat(LlmRequest {
        model: "gpt-4o-mini".into(),
        messages: vec![ChatMessage {
            role: "user".into(),
            content: Some("Hello".into()),
            ..Default::default()
        }],
        ..Default::default()
    })
    .await?;
# let _ = response;
# Ok(())
# }
```

## Wire formats (schema, not vendor)

[`WireFormat`](./src/client.rs) chooses request/response **JSON shape**:

| Variant | Typical endpoints |
|--------|-------------------|
| `OpenAiCompat` | OpenAI, Azure OpenAI (with [`AzureOpenAiAuth`](./src/auth.rs)), OpenRouter, vLLM, Ollama, LM Studio |
| `AnthropicMessages` | Anthropic API, Bedrock Claude, Vertex Claude |

Authentication is separate: implement [`AuthProvider`](./src/auth.rs) for API keys, Azure `api-key` / Bearer, or custom per-request signing (e.g. SigV4) without pulling AWS/GCP SDKs into this crate.

## Streaming: native vs WASM

- **Native**: incremental SSE via [`sse_event_stream`](./src/stream.rs).
- **WASM**: full body buffered then parsed via [`sse_event_stream_from_buffer`](./src/stream.rs) (same [`StreamEvent`](./src/stream.rs) sequence when the bytes are identical).
- Both paths emit **at most one** [`StreamEvent::StreamStart`](./src/stream.rs) per response (some gateways repeat `delta.role` every chunk).

## Tools, JSON escape hatch

- Use [`ToolChoice`](./src/types.rs) and [`ToolSchema`](./src/types.rs) on [`LlmRequest`](./src/types.rs).
- [`LlmRequest::extensions`](./src/types.rs) is an optional `serde_json` map for provider-specific knobs (parallel tool flags, templates, etc.).

## Running tests

```bash
cargo test -p llm_client
```

Live HTTP smoke tests (optional env keys) live in `tests/smoke_live.rs`.
