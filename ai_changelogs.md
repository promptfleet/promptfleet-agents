# AI Changelogs

## 2026-07-19 — A2A streaming server observability lifecycle

### Summary

Extended the native A2A server request span across `SendStreamingMessage` SSE
execution. Streaming now propagates the server trace context while the agent
stream is polled, records terminal success/error/cancellation status, emits the
standard A2A request metrics, flushes the completed span, and closes abandoned
client streams as cancelled instead of omitting the run entirely.

### Files modified

- `crates/a2a_http_server/src/native_server.rs` — shared request telemetry
  lifecycle for JSON-RPC and streaming A2A execution.
- `crates/a2a_http_server/tests/streaming_integration.rs` — regression coverage
  proving the A2A server trace context remains active during SSE stream polling.

### Tests

- `cargo test -p a2a_http_server --features 'event-stream observability' --test streaming_integration`: **passed** (4 tests).
- `cargo check --target wasm32-wasip1 -p a2a_http_server --features 'event-stream observability'`: **passed**.

## 2026-07-18 — Standard AG-UI interrupts, frontend tools, and async auth adapters

### Summary

Completed the standard AG-UI application contract: canonical tool messages and
correlation, request-declared frontend tools, state/message snapshots, and
`RUN_FINISHED.outcome` interrupt/resume semantics. Added composable
resource-bound authentication adapters for Node 20 and Python 3.11, including
async KMS signing and single-flight token refresh. Interrupt snapshots now
retain assistant tool calls and correlated pending tool results, and canonical
resume entries replace the pending result before the resumed LLM turn.

### Files modified

- `crates/agent_core` and `crates/agent_sdk` — tool roles/parts, frontend tool
  registration and collision rejection, canonical request context/state/resume,
  official interrupt outcomes, replay-complete stream snapshots, and resolved
  interaction results in resumed model history.
- `sdks/typescript` — authenticated fetch composition for standard clients and
  Node 20 support.
- `sdks/python` — Python 3.11 support plus async signer, transport, token cache,
  and authorization-header APIs.

### Tests

- `cargo test -p pf_agent_sdk --all-features --lib`: **passed** (273 tests).
- `cargo test -p pf_agent_sdk --features event-stream`: **passed** (227 unit,
  1 A2A integration, and 5 streaming seam tests).
- `cargo check --target wasm32-wasip1 -p pf_agent_core`: **passed**.
- `cargo check --target wasm32-wasip1 -p pf_agent_sdk --no-default-features --features agui-agent,wasm-optimized`: **passed**.
- `npm test` in `sdks/typescript`: **passed** (3 tests).
- Python 3.11 isolated environment, `pytest -q` in `sdks/python`: **passed** (4 tests).

## 2026-07-11 — Inline document and image inputs for agent runs

### Summary

Added a provider-neutral inline file content part, exact OpenAI Responses `input_file` mapping, and an AG-UI path that carries malware-scanned current-turn attachment bytes into LLM requests without retaining file bytes in conversation history. Centralized pre-serialization validation now applies the same MIME, detail, strict Base64, decoded-size, aggregate-size, and part-count safety contract to OpenAI, Anthropic, and Google requests.

### Files modified

- `crates/llm/llm_client/src/types.rs` — added `ChatContentPart::FileBase64` and its constructor.
- `crates/llm/llm_client/src/multimodal.rs` and `src/lib.rs` — added the allocation-free shared validator for canonical file/image MIME types, detail values, strict RFC 4648 Base64, 50 MB decoded part/aggregate limits, and five file inputs plus five image inputs per request.
- `crates/llm/llm_client/src/providers/openai.rs` — mapped image/PDF/DOCX inputs and applied shared validation to Chat Completions and Responses request paths while retaining Responses-only PDF detail.
- `crates/llm/llm_client/src/providers/anthropic.rs` and `providers/google.rs` — kept file mapping exhaustive using native document/inline-data shapes and applied shared validation before serialization.
- `crates/agent_sdk/src/agui.rs` — decoded bounded AG-UI file parts and removed file bytes from retained history.
- `crates/agent_sdk/src/agent/llm_orchestrator.rs` — mapped current-user image and document bytes to multimodal LLM content while continuing to omit historical files.
- Workspace and `pf_agent_sdk` manifests — added the WASM-compatible `base64` dependency.

### Tests

- `cargo test -p pf_llm_client`: **passed** (134 unit tests, 2 integration tests, 2 doctests; 6 live tests ignored).
- `cargo test -p pf_agent_sdk --features agui-agent file_input_tests`: **passed** (2 focused AG-UI-to-LLM seam tests).
- `cargo check --target wasm32-wasip1 -p pf_llm_client`: **passed**.
- `cargo check --target wasm32-wasip1 -p pf_agent_sdk --features llm-engine`: **passed**.

### Notes

- The broader `pf_agent_sdk --features agui-agent` run passed all 221 unit tests and all attachment-relevant tests, but still has one unrelated pre-existing `streaming_seam_harness::test_driver_with_test_pipeline` event-sequence expectation mismatch.
