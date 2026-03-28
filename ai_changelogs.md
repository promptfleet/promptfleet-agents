# 📋 AI Changelogs

## 2026-03-28 — Docs: splash CardGrid alignment (remove stagger)

### Changes
- **`docs/src/content/docs/index.mdx`:** Removed `stagger` from the top `CardGrid`. Starlight’s stagger prop offsets alternate columns on purpose; it read as a broken 2×2 grid on the splash entry page.

### Files modified
- `docs/src/content/docs/index.mdx`

### Tests
- `yarn build` (in `docs/`): **passed** — 141 page(s) built; Pagefind index OK.

## 2026-03-28 — Docs: Starlight sidebar integration pass + build verify

### Changes
- **`docs/astro.config.mjs`:** Replaced manual sidebar with full navigation: Getting Started → Quickstart → Capabilities Matrix → Architecture; **Concepts** (3); **Guides** (11); **SDK / Core**; **A2A Protocol** (badges: A2A v1.0 Compliance **New**, a2a_rpc_macros **Experimental**); **LLM**; **Observability**; **Support / Integration** (pf_test_harness **Native**); **API Reference** → structured_logging subtree. Fixed inconsistent indentation under `starlight({ … })`.
- **`docs/src/content/docs/api/structured-logging/index.mdx`**, **`context-adapter.mdx`:** Replaced fenced language `rust,ignore` with `rust` so Expressive Code highlights correctly (avoids build-time `astro-expressive-code` warnings).

### Files modified
- `docs/astro.config.mjs`
- `docs/src/content/docs/api/structured-logging/index.mdx`
- `docs/src/content/docs/api/structured-logging/context-adapter.mdx`

### Tests
- `yarn build` (in `docs/`): **passed** — 58 page(s) built; Pagefind 58 HTML files; no `rust,ignore` highlighting warnings after fence fix.

### Notes
- Build may log `404.html` / Starlight 404 entry and a Vite unused-import notice from Astro internals; exit code clean.

## 2026-03-28 — Docs (WS6): A2A compliance + capabilities matrix pages

### Changes
- **`docs/src/content/docs/a2a/capabilities.mdx`:** A2A v1.0 JSON-RPC method matrix grounded in `a2a_protocol_core::A2AProtocol::register_a2a_methods` and native `SendStreamingMessage` SSE interception in `a2a_http_server`.
- **`docs/src/content/docs/capabilities.mdx`:** Workspace capability tables (runtime, LLM, A2A, serving, tools, observability, config, testing) with `agent_sdk` feature flags from `Cargo.toml`.
- **`docs/astro.config.mjs`:** sidebar **Capabilities Matrix** → `/capabilities/`; A2A section **A2A v1.0 compliance** → `/a2a/capabilities/`.

### Files modified
- `docs/src/content/docs/a2a/capabilities.mdx` (new)
- `docs/src/content/docs/capabilities.mdx` (new)
- `docs/astro.config.mjs`

### Tests
- `yarn build` (in `docs/`): **passed**

### Notes
- Internal links use `/promptfleet-agents/…` as required for GitHub Pages. `SendStreamingMessage`: protocol handler runs on WASM when `event-stream` is enabled; HTTP SSE path is native-only with `with_streaming_port`. `interactive-tools` is `agent-core`-only on `agent_sdk` (not `llm-engine`). Prometheus: enable `observability` crate feature `prometheus` separately from default `agent-observability` (OTEL-only).

## 2026-03-28 — Docs (WS2): Quickstart tutorial (A2A echo agent)

### Changes
- **`quickstart.mdx`:** “Build your first A2A agent” tutorial with Starlight `Steps`, `Tabs`, `Aside`; native `main.rs` and WASM `lib.rs` copied verbatim from `examples/a2a-echo-native` and `examples/a2a-echo-wasm`; dependency blocks and `spin.toml` match the examples; curl `SendMessage` sample; “What just happened?” ties `Agent` / `A2aApp` / Spin `http_component` + `OnceLock`; next-step links use `/promptfleet-agents/…` base.
- **`astro.config.mjs`:** sidebar entry **Quickstart** → `/quickstart/`.

### Files modified
- `docs/src/content/docs/quickstart.mdx` (new)
- `docs/astro.config.mjs`

### Tests
- `yarn build` (in `docs/`): **passed**

### Notes
- Next-step targets (`/guides/`, `/concepts/a2a-protocol/`, `/concepts/wasm-vs-native/`) are placeholders until those pages exist; SDK overview today is `/sdk/`.
- WASM example `Cargo.toml` uses `workspace = true` like the repo; tutorial Asides point to path deps or copying `examples/a2a-echo-wasm`.

## 2026-03-28 — Docs (WS1): splash landing, custom CSS, Starlight config polish

### Changes
- **`index.mdx`:** splash hero with CardGrid sections; hero actions use `/promptfleet-agents/` base for internal links per GitHub Pages deploy.
- **`src/styles/custom.css`:** content width, card grid gap, hero tagline width.
- **`astro.config.mjs`:** `customCss` and `lastUpdated: true` on Starlight integration.

### Files modified
- `docs/src/content/docs/index.mdx`
- `docs/src/styles/custom.css` (new)
- `docs/astro.config.mjs`

### Tests
- `yarn build` (in `docs/`): **passed**

### Notes
- Hero “Quickstart” links to `/promptfleet-agents/quickstart/`; add that page or repoint when quickstart content exists (sidebar still uses Getting Started).

## 2026-03-26 — Docs: document `alias yarn='yarn --non-interactive'` as Yarn 4 breaker

### Changes
- **`getting-started.mdx` / `docs/README.md`:** troubleshooting now calls out the common zsh alias that expands every `yarn` to `yarn --non-interactive`; fix with `unalias yarn`, dotfile edit, or **`command yarn …`**.

### Files modified
- `docs/README.md`, `docs/src/content/docs/getting-started.mdx`

### Tests
- `yarn build` (in `docs/`): **passed**

## 2026-03-26 — Docs: Yarn 4 + `--non-interactive` prefix (Clipanion) troubleshooting

### Changes
- **Root cause (reproduced):** `yarn --non-interactive -v` and `yarn --non-interactive install` fail on Yarn 4.13.x; error traces show `While running --non-interactive …`. Some environments prepend global `--non-interactive` before the subcommand.
- **`docs/src/content/docs/getting-started.mdx`:** new Troubleshooting section — `type yarn`, `command yarn …`, external terminal, `node -p "require('./package.json').packageManager"`.
- **`docs/README.md`:** pointer to that section; removed incorrect “only use `-v` not `--version`” explanation.
- **`README.md`:** short pointer to the MDX troubleshooting section.

### Files modified
- `README.md`, `docs/README.md`, `docs/src/content/docs/getting-started.mdx`

### Tests
- `yarn build` (in `docs/`): **passed**

### Notes
- Prior note blaming `yarn --version` alone was wrong; the failure mode matches **global `--non-interactive` before subcommands**.

## 2026-03-26 — Docs: Yarn 4 uses `yarn -v`, not `yarn --version`

### Changes
- **`docs/README.md`**, **`docs/src/content/docs/getting-started.mdx`:** note that Yarn Berry reports errors for `yarn --version`; use **`yarn -v`** to print the version.

### Files modified
- `README.md`, `docs/README.md`, `docs/src/content/docs/getting-started.mdx`

### Tests
- `yarn build` (in `docs/`): **passed**

## 2026-03-26 — Docs: run Yarn only from `docs/`; ignore stray root `yarn.lock`

### Changes
- **Root `.gitignore`:** `node_modules` and `/yarn.lock` so accidental Yarn 1 runs at the repo root do not get committed.
- **`README.md`:** short “Documentation site (Starlight)” note — `cd docs`, Corepack, why root `yarn` is wrong.
- **`docs/README.md`**, **`getting-started.mdx`:** stress that only `docs/` has `package.json`.

### Files modified
- `.gitignore`, `README.md`, `docs/README.md`, `docs/src/content/docs/getting-started.mdx`

### Tests
- `yarn build` (in `docs/`): **passed**

### Notes
- Removed stray root **`yarn.lock`** and **`node_modules`** created by Yarn 1 at repo root (not applicable if the user’s tree was clean).

## 2026-03-26 — Docs: Corepack + local `yarn install` (fix Ambiguous Syntax with Yarn 1)

### Changes
- **`docs/README.md`** and **`docs/src/content/docs/getting-started.mdx`:** document `corepack enable` before `yarn`, use plain `yarn install` locally, reserve `yarn install --immutable` for CI; explain Yarn 1 vs Yarn 4 “Ambiguous Syntax” failure mode.

### Files modified
- `docs/README.md`
- `docs/src/content/docs/getting-started.mdx`

### Tests
- `yarn build` (in `docs/`): **passed**

### Notes
- Root cause: global **Yarn 1** misparses Berry-only flags; **`corepack enable`** selects Yarn **4.13.0** from `packageManager`.

## 2026-03-26 — Starlight docs scaffold (`docs/`) + GitHub Pages workflow

### Changes
- **Starlight site** under `docs/`: Astro 6 + `@astrojs/starlight`, `site` + `base` for `https://promptfleet.github.io/promptfleet-agents`, sidebar for Getting Started, Architecture, and six observability pages.
- **Content:** nine MDX pages (landing, getting-started, architecture, observability overview + five crates). Crate docs are grounded in each crate’s `Cargo.toml` and `src/lib.rs` (feature tables and public API summaries), not marketing README claims.
- **CI:** `.github/workflows/deploy-docs.yml` — push to `main` with `docs/**` or workflow path → `yarn install --immutable`, `yarn build`, `upload-pages-artifact` + `deploy-pages@v4`.
- **Tooling:** Yarn 4 with `nodeLinker: node-modules` in `docs/.yarnrc.yml`; `docs/.gitignore` extended with `dist` and `.astro`.

### Files modified
- `docs/package.json`, `docs/yarn.lock`, `docs/.yarnrc.yml`, `docs/astro.config.mjs`, `docs/tsconfig.json`, `docs/.gitignore`, `docs/README.md`
- `docs/src/content.config.ts`, `docs/src/content/docs/**/*.mdx`
- `.github/workflows/deploy-docs.yml`

### Tests
- `yarn build` (in `docs/`): **passed**
- `cargo check -p observability -p observability_core -p structured_logging -p otel -p prometheus`: **passed**

### Notes
- Enable **GitHub Pages** with **GitHub Actions** as the source for deploy to succeed. Workflow branch is **`main`** (per plan); adjust if the default branch differs.

## 2026-03-26 — `agent_sdk` plan follow-up: remaining docs + A2A router tests + plan status

### Changes
- **Docs (4B/4C/4D gaps):** `host` (`AgentHostBuilder` / `AgentHost` / methods), `interaction` field docs, expanded **`agui`** module overview (native + `event-stream`), **`callable`**, **`a2a_app`** method docs; **`lib.rs`** feature matrix table + **`configure_llm_runtime`** example (`rust,ignore`); **`README`** examples link via [`examples/`](./examples/) relative to the crate.
- **Tests:** `a2a_app` **`tokio`** tests — GET **`/health`** → 200, GET **`/.well-known/agent-card.json`** → JSON with `name`; **`builder`** — invalid JSON file returns error (missing-file not asserted: **`pf_config`** may still succeed depending on loader behavior).
- **Deps:** dev-dependency **`tower`** (`util`) for `ServiceExt::oneshot` in tests.
- **Plan:** `.cursor/plans/sdk_assessment_fixes_188d9e96.plan.md` frontmatter todos set to **completed**.

### Files modified
- `crates/agent_sdk/src/host.rs`, `interaction.rs`, `agui.rs`, `callable.rs`, `a2a_app.rs`, `lib.rs`, `builder.rs`, `Cargo.toml`
- `crates/agent_sdk/README.md`
- `../promptfleet-agents-cloud/.cursor/plans/sdk_assessment_fixes_188d9e96.plan.md`

### Tests
- `cargo doc -p agent_sdk --no-deps --all-features`: **0 warnings**
- `cargo test -p agent_sdk --all-features`: **all passed** (lib + integration + doctests)

### Notes
- **`AgUiConfig` / `AgUiApp`** links in `agui.rs` use explicit `crate::agui::...` paths for rustdoc on all targets.

## 2026-03-26 — `agent_sdk` Phase 4: rustdoc (zero warnings) + crate/README docs

### Changes
- **Rustdoc**: resolved intra-doc links in `llm_orchestrator`, `trace`, `skill`, `lib`; MCP `mod`/`config`/`web_search`; `LlmPolicy` / `LlmRequestDefaults` field docs; avoided private-module links.
- **Module docs**: `host.rs`, `interaction.rs`, `callable.rs`, `a2a_app.rs` (`agui.rs` already had a module banner).
- **`lib.rs`**: quick start uses **`AgentBuilder::new`**, **`add_skill`**, **`handler`**; migration bullets use **`crate::Agent::...`**; **`a2a_serve`** examples use **`AgentBuilder`** and a correct `build()` + `skill().register()` expansion sketch.
- **`crates/agent_sdk/README.md`**: core contract bullets (builder + skill APIs), fluent skill sample, short **Which features?** guide.

### Files modified
- `crates/agent_sdk/src/agent/llm_orchestrator.rs`, `trace.rs`, `llm_invoker.rs`, `skill.rs`, `lib.rs`, `host.rs`, `interaction.rs`, `callable.rs`, `a2a_app.rs`, `mcp_tools/mod.rs`, `mcp_tools/config.rs`, `mcp_tools/web_search.rs`
- `crates/agent_sdk/README.md`

### Tests
- `cargo doc -p agent_sdk --no-deps --all-features`: **0 warnings**
- `cargo test -p agent_sdk --all-features`: **all passed** (including doctests)

### Notes
- `mcp_tools` rust example remains `rust,ignore` (doctest skipped by design).

## 2026-03-26 — `agent_sdk` assessment: `SkillEntryBuilder`, `configure_llm_runtime`, `AgentBuilder` constructors

### Changes
- **`SkillRegistry` / `Agent`**: `add_skill(id)` returns **`SkillEntryBuilder`** (optional `.handler()`); metadata-only skills no longer need a stub handler. **`skill(name, handler)`** delegates to `add_skill` + `.handler()`. Removed generic **`SkillBuilder`** in favor of the unified builder.
- **LLM wiring**: **`configure_llm_runtime`** replaces `set_llm_tools_message_handler_configured`; removed **`set_llm_tools_message_handler_with`** and the public low-level **`set_llm_tools_message_handler`** (internal `set_llm_tools_handler` wrapper removed from `MessageHandlerManager`).
- **`AgentBuilder`**: added **`from_config(AgentConfig)`** and **`new(name)`**; `from_config_path` unchanged.
- **`lib.rs`**: removed crate-root **`new` / `new_runtime`**; documented migration and `configure_llm_runtime`.
- **Conversions**: removed unused **`runtime_artifact_from_a2a`**.
- **`history_policy`**: `cfg_attr(not(context-window), allow(dead_code, unused_imports))` so minimal-feature builds stay warning-clean.
- **Tests**: `builder`, `runtime_vars`, `observability_runtime`, `a2a_app`, `llm_invoker`, `client` (`SkillValidationError`), `mcp_tools::error`; env tests use **`unsafe`** for `set_var`/`remove_var` (Rust 2024).
- **Examples / README**: `add_skill` fluent registration; **`crates/agent_sdk/examples/`** path in README.
- **`promptfleet-agents-cloud` / `universal_agent`**: `register_skills` uses **`add_skill`** without stub handler; **`configure_llm_runtime`** in `wiring.rs`.

### Files modified
- `crates/agent_sdk/src/agent/skill.rs`, `core.rs`, `builder.rs`, `lib.rs`, `conversions.rs`, `agent/history_policy.rs`, `agent/message_handlers.rs`, `agent/llm_invoker.rs`, `a2a_app.rs`, `agui.rs`, `client.rs`, `agent/response_builders.rs`, `runtime_vars.rs`, `observability_runtime.rs`, `mcp_tools/error.rs`, `Cargo.toml`
- `README.md`, `crates/agent_sdk/README.md`
- `examples/a2a-echo-wasm/src/lib.rs`, `examples/a2a-echo-native/src/main.rs`
- `../promptfleet-agents-cloud/src/pf_cloud/universal_agent/src/wiring.rs`

### Tests
- `cargo test -p agent_sdk --all-features`: **237 lib + integration + doctests passed** (237 + 1 + 3 + … per crate targets; full run reported **all pass**)

### Notes
- Extended rustdoc + README pass is recorded in the **`agent_sdk` Phase 4** entry above.

## 2026-03-26 — `a2a_app_ports` README: server delegation vs protocol layer

### Changes
- Clarified that **`GetAgentCard`** is delegated via **`build_agent_card`** (not only **`SendMessage`**), and that task / extended-card RPCs use the protocol stack without per-method port hooks.
- Design note distinguishes **inbound** server seam from **`a2a_http_client`** when calling other agents.

### Files modified
- `crates/a2a_app_ports/README.md`

### Tests
- `cargo check -p a2a_app_ports`: **ok**

### Notes
- Aligns docs with `a2a_http_server` routing (`native_server.rs` / `wasm_server.rs`).

## 2026-03-26 — `pf_test_harness` OpenAI mock + `LlmClient` scenario integration tests

### Changes
- **`pf_test_harness`**: new [`scenario_openai_http`](crates/pf_test_harness/src/scenario_openai_http.rs) — encodes scenario turns as OpenAI Chat Completions **JSON** or **SSE**, and **`OpenAiScenarioMock`** (`/v1/chat/completions`) for local HTTP.
- **`scenario` feature** now also enables **`axum`** (mock server).
- **`fold_stream_events_to_llm_response`**: public API (was private `turn_to_llm_response`) for reuse by the wire encoder.
- **`llm_client`**: dev-dependency on **`pf_test_harness`** with `scenario`; integration tests [`tests/client_scenario_facade.rs`](crates/llm/llm_client/tests/client_scenario_facade.rs) cover **`LlmClient::chat`** and **`chat_stream`** against the mock.
- **`llm_client` README**: documents the facade + harness test.

### Files modified
- `crates/pf_test_harness/Cargo.toml`, `src/lib.rs`, `src/scenario.rs`, `src/scenario_openai_http.rs` (new)
- `crates/llm/llm_client/Cargo.toml`, `tests/client_scenario_facade.rs` (new), `README.md`

### Tests
- `cargo test -p pf_test_harness --features scenario`: **5 passed**
- `cargo test -p llm_client`: **105 lib + 2 client_scenario_facade + 6 ignored smoke + 2 doctests, all pass**

### Notes
- Mock is **OpenAI Chat Completions only** (`WireFormat::OpenAiCompat`, `ApiMode::Chat`); Anthropic / Responses API not covered here.

## 2026-03-26 — `llm_client` dedupe `StreamStart` in OpenAI SSE drivers

### Changes
- Added [`dedupe_stream_starts`](crates/llm/llm_client/src/stream.rs): at most one [`StreamEvent::StreamStart`](crates/llm/llm_client/src/stream.rs) per HTTP streaming response when aggregating SSE lines (fixes gateways that repeat `delta.role` every chunk, e.g. some OpenRouter-style streams).
- Applied in [`sse_event_stream`](crates/llm/llm_client/src/stream.rs) and [`sse_event_stream_from_buffer`](crates/llm/llm_client/src/stream.rs); [`parse_chat_chunk`](crates/llm/llm_client/src/stream.rs) unchanged (still stateless per line).
- New fixture [`openai_repeat_role_each_chunk`](crates/llm/llm_client/src/stream.rs) and tests `sse_dedupes_stream_start_when_role_repeated_per_chunk`, `buffer_vs_native_sse_parity_repeat_role_chunks`.
- README streaming section notes the guarantee.

### Files modified
- `crates/llm/llm_client/src/stream.rs` — dedupe helper, driver wiring, tests
- `crates/llm/llm_client/README.md` — one bullet on `StreamStart`

### Tests
- `cargo test -p llm_client`: **105 passed** (was 103, +2)

### Notes
- Anthropic path uses `sse_event_stream_anthropic` in `providers/anthropic.rs`; unchanged (separate event model).

## 2026-03-25 — `llm_client` OSS hardening follow-up (parity, WASM Send, docs)

### Changes
- Added native **`buffer_vs_native_sse_event_parity`** test: chunked `reqwest::Response` through [`sse_event_stream`](crates/llm/llm_client/src/stream.rs) matches [`sse_event_stream_from_buffer`](crates/llm/llm_client/src/stream.rs) and the shared OpenAI SSE fixtures.
- Added **`test_post_sse_anthropic_shaped_error_maps_transport`**: HTTP 502 with Anthropic-style error JSON on the SSE (`post_sse`) path maps to `LlmError::Transport` with body preserved (mirrors JSON path coverage).
- **`LlmProvider::chat`**: introduced [`ChatFuture`](crates/llm/llm_client/src/provider.rs) alias — **`Send` only on native**, fixing **`wasm32-wasip1` build** (`Spin`/outgoing body is not `Send`).
- Moved **`HttpModelClient::post_sse_buffered`** to **`#[cfg(target_arch = "wasm32")]`** only (native uses `post_sse`); restored error **`headers: Some(resp.headers)`** on WASM buffered SSE errors for parity with JSON errors.
- Removed unused **`AnthropicClient::from_api_key`**; dropped unused imports.
- New **[`crates/llm/llm_client/README.md`](crates/llm/llm_client/README.md)** (quick start, wire formats, native vs WASM streaming).
- **`HttpModelClient`**: `#[cfg_attr(wasm32, allow(dead_code))]` on `streaming` (native-only knob).
- **`Cargo.toml`**: dev-deps `http`, `bytes` for parity test body construction.

### Files modified
- `crates/llm/llm_client/src/stream.rs` — `native_sse_parity_tests` module
- `crates/llm/llm_client/src/model_client.rs` — WASM-only `post_sse_buffered`, transport tests fix + Anthropic SSE error test
- `crates/llm/llm_client/src/provider.rs` — `ChatFuture` alias
- `crates/llm/llm_client/src/providers/openai.rs`, `anthropic.rs` — `ChatFuture` return type, import cleanup
- `crates/llm/llm_client/Cargo.toml` — dev-dependencies
- `crates/llm/llm_client/README.md` — **new**

### Tests
- `cargo test -p llm_client`: **103 lib + 6 ignored smoke + 2 doctests, all pass**
- `cargo check --target wasm32-wasip1 -p llm_client`: **pass**
- `cargo test -p agent_sdk --features llm-engine --no-run`: **pass** (compile)

### Notes
- Cloud consumers already compile against the typed `LlmClient` surface; no cloud repo changes in this entry.

## 2026-03-24 — Observability stack test stabilization and API hardening

### Changes
- **Fixed `structured_logging` string interning bug**: `intern_string()` called `.to_string()` on the symbol index (returning `"0"`) instead of resolving the actual string via `interner.resolve(sym)`. Also fixed hit/miss stats race condition with double-checked locking under write lock.
- **Fixed circular feature flags** in `structured_logging/Cargo.toml`: `fast-paths → performance-optimized → fast-paths` and `scoped-context → correlation-enhanced → scoped-context` cycles removed.
- **Fixed panic handler idempotency**: `PerformanceExtension::new()` now silently succeeds if the panic handler is already installed (OnceLock), making it test-safe across multiple instances.
- **Fixed fast-path buffer cloning**: `log_llm_request_fast` and `log_a2a_message_fast` now clone the result before returning the buffer to the pool, instead of cloning the buffer for the pool and returning the original (wasted allocation).
- **Hardened `ResourceAttributeManager`**: Custom attributes can no longer overwrite reserved `service.*` keys. Reserved keys are filtered at construction, `add_attribute()`, and `remove_attribute()`.
- **Added tests for `observability_core/src/error.rs`**: All 10 error variants, constructors, Display, Clone, Debug, From<serde_json::Error>, Result type alias.
- **Added tests for `observability_core/src/ports.rs`**: TransportPort batch default, MetricsPort batch routing, ContextPort CRUD, FormatterPort JSON output, BatchingPort lifecycle, StandardLoggingPort init/enabled.
- **Added tests for `observability/src/semconv.rs`**: Allowlist filtering (keep, drop, order preservation, empty input), allowlist content assertions (expected keys present, high-cardinality keys absent), stability tests for all span/attr/metric/value constants.
- **Added tests for `otel/src/resource_attributes.rs`**: Standard attributes always present, custom merge, reserved key protection at construction and mutation, accessor correctness, empty custom attributes.
- **Added tests for `structured_logging/src/performance.rs`**: Zero-denominator stats ratios, buffer pool exhaustion, A2A fast-path (with and without duration), string interning hit/miss stats, process_entry field interning, PerformanceManager stats aggregation and reset.

### Files modified
- `crates/observability/structured_logging/src/performance.rs` — Fixed interning bug, buffer cloning, added 7 new tests
- `crates/observability/structured_logging/Cargo.toml` — Removed circular feature cycles
- `crates/observability/structured_logging/src/extension.rs` — Made panic handler install idempotent
- `crates/observability/observability_core/src/error.rs` — Added 5 tests
- `crates/observability/observability_core/src/ports.rs` — Added 8 tests for all port traits
- `crates/observability/observability/src/semconv.rs` — Added 11 tests for allowlist and constant stability
- `crates/observability/otel/src/resource_attributes.rs` — Fixed reserved key overwrite, added 8 tests

### Tests
- `cargo test -p observability_core --all-features`: **50 passed** (was 37, +13 new)
- `cargo test -p structured_logging --all-features`: **37 passed** (was 28 pass + 2 fail, +7 new, 2 bugs fixed)
- `cargo test -p prometheus@0.1.0 --all-features`: **25 passed** (unchanged, already well-covered)
- `cargo test -p observability --no-default-features --features serde,config,logging`: **30 passed**
- `cargo test -p observability --all-features`: **32 passed** (was 21, +11 new)
- `cargo test -p otel --features otel-2025,auto-instrumentation,structured-logging,grpc-tonic`: **34 passed** (was 26, +8 new)

### Notes
- Total new tests added: **47** across 5 crates
- Total bugs fixed: **4** (string interning, circular features, panic idempotency, buffer cloning)
- Total API hardening: **1** (ResourceAttributeManager reserved key protection)
- Prometheus crate already had strong coverage (25 tests); no additional tests needed per plan guidance ("stop once high-risk public behavior is locked")

## 2026-03-23 — `llm_client` pre-OSS hardening (all workstreams A-E)

### Summary
Complete implementation of the `llm_client` pre-OSS hardening plan: provider-neutral message model, Anthropic provider with full parity, comprehensive test coverage, WASM SSE support, and agent_sdk seam cleanup.

**Workstream B — Provider-neutral message model:**
- `ChatMessage.content` changed from `String` to `Option<String>`
- Added `ToolCallRequest { id, name, arguments: Value }` type
- Added `tool_calls`, `tool_call_id`, `name` fields to `ChatMessage`
- All downstream code updated for the enriched type

**Workstream E — Anthropic provider (full parity):**
- New `AnthropicClient` with Messages API support (`/v1/messages`)
- `to_messages_payload`: system extraction, `input_schema`, tool result batching, max_tokens default
- `normalize_messages_json`: text/tool_use extraction, stop_reason mapping, usage mapping
- `parse_anthropic_chunk`: typed SSE event parser for Anthropic's event format
- Native SSE streaming via `sse_event_stream_anthropic`
- `SseParser` extended with `next_typed_event()` for `event:` field tracking

**Workstream D — WASM buffer-then-parse SSE:**
- `HttpModelClient::post_sse_buffered` for dual-target HTTP
- `sse_event_stream_from_buffer` for OpenAI-style SSE bodies
- WASM `llm_stream`/`llm_stream_raw` on both `OpenAIClient` and `AnthropicClient`
- `ClientCapabilities::streaming` set to `true` on all targets

**Workstream C — agent_sdk seam cleanup:**
- `IntoLlmInvoker`/`IntoLlmStreamInvoker` for `AnthropicClient`
- Core tool loop uses typed `ChatMessage` + `ToolCallRequest` instead of raw `json!`

**Workstreams A1-A3 — Test coverage:**
- `prepare.rs`: 0% → 96% (13 tests)
- `openai.rs`: 22% → 90% (22 tests total, 20 new)
- `model_client.rs`: 56% → 68% (7 tests)
- `anthropic.rs`: new file → 84% (20 tests)
- `stream.rs`: 88% → 89% (4 new tests)
- **Overall llm_client: 53% → 87.3%**

### Files modified
- `crates/llm/llm_client/src/types.rs` — ChatMessage enrichment, ToolCallRequest
- `crates/llm/llm_client/src/providers/openai.rs` — enriched type support, 20 new tests
- `crates/llm/llm_client/src/providers/anthropic.rs` — **new file**, full Anthropic provider
- `crates/llm/llm_client/src/providers/mod.rs` — export AnthropicClient
- `crates/llm/llm_client/src/stream.rs` — typed events, buffer-parse, Anthropic chunk parser
- `crates/llm/llm_client/src/model_client.rs` — post_sse_buffered, streaming=true, tests
- `crates/llm/llm_client/src/prepare.rs` — 13 unit tests
- `crates/llm/llm_client/src/lib.rs` — export parse_anthropic_chunk
- `crates/agent_sdk/src/agent/llm_invoker.rs` — Anthropic invoker impls
- `crates/agent_sdk/src/agent/engine/core_loop.rs` — typed message construction

### Tests
- `cargo test -p llm_client`: **95 unit + 3 integration + 1 doctest = 99 total, all pass**
- `cargo test -p agent_sdk --features llm-engine`: **190 unit + 6 integration + 13 doctests = 209 total, all pass**
- `cargo check --target wasm32-wasip1 -p llm_client`: **pass**
- `cargo llvm-cov -p llm_client`: **87.3% line coverage** (target 80%+)

## 2026-03-23 — `Duration::from_mins` / `from_hours` for minute/hour literals (Rust 1.91+)

### Summary
Replaced selected `Duration::from_secs(...)` with `from_mins` / `from_hours` for exact 60s, 600s, and 3600s literals.

### Files modified
- `crates/foundation_utils/src/resource.rs` — `600` → `from_mins(10)`, `60` → `from_mins(1)`
- `crates/a2a_http_client/src/activation.rs` — `60` → `from_mins(1)` (default + test assert)
- `crates/observability/observability_core/src/batching.rs` — `3600` → `from_hours(1)`

### Tests
- `cargo test -p a2a_http_client -p foundation_utils -p observability_core`: **passed** (incl. foundation_utils doctests)

### Notes
- Companion edits in `promptfleet-agents-cloud` are recorded in that repo’s `ai_changelogs.md`.

## 2026-03-23 — Remove redundant `Future` prelude imports (Rust 2024)

### Summary
Dropped standalone `use std::future::Future` / `use core::future::Future`; Rust 2024 prelude already includes `Future`.

### Files modified
- `crates/agent_sdk/src/agent/skill.rs`
- `crates/agent_sdk/src/callable.rs`
- `crates/agent_sdk/src/sub_agent/adapter.rs`
- `crates/a2a_app_ports/src/lib.rs` (`core::future::Future`)
- `crates/llm_context_core/src/history.rs`
- `crates/llm_context_core/src/memory.rs`
- `crates/observability/observability_core/src/context.rs`

### Tests
- `CARGO_TARGET_DIR=/tmp/pf-future-import-check cargo check -p agent_sdk -p observability_core -p llm_context_core -p a2a_app_ports`: **passed**

### Notes
- Companion edits in `promptfleet-agents-cloud` are recorded in that repo’s `ai_changelogs.md`.
