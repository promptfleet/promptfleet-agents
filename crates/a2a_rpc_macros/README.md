# a2a_rpc_macros

> **Experimental** — Not integrated into the SDK pipeline. API is unstable. Do not use in production.

Proc-macro crate for automatic A2A JSON-RPC command registration. Generates compile-time registration code with zero runtime overhead — no `linkme`, no global state at runtime.

## Intended usage (not yet stable)

```rust
use a2a_rpc_macros::{a2a_rpc, generate_a2a_commands, extension, generate_extensions};
use serde_json::{json, Value};

// Sync handler
#[a2a_rpc("SendMessage")]
fn send_message_handler(params: Value) -> Result<Value, Box<dyn std::error::Error>> {
    Ok(json!({"status": "ok"}))
}

// Async handler (WASM-compatible)
#[a2a_rpc("GetTask")]
async fn get_task_handler(params: Value) -> Result<Value, Box<dyn std::error::Error>> {
    Ok(json!({"task": {}}))
}

// Extension registration
#[extension("my-extension")]
pub struct MyExtension {}

// Emit the generated registration glue
generate_a2a_commands!();
generate_extensions!();
```

## Known limitations

- Uses a compile-time `static Mutex` for command collection. This is a known issue — across separate proc-macro invocations the registry may not accumulate correctly in all build configurations.
- The generated code targets a specific internal API that is still evolving.
- Not tested end-to-end with the WASM spin entry point.

## Feature flags

| Flag | Enables |
|------|---------|
| `build-script-gen` | Generates `build.rs` scaffolding automatically |

## Status

Tracked as experimental. The planned replacement is a simpler inventory-based approach that avoids global state entirely.
