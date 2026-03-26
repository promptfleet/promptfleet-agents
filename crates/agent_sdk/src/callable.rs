//! JSON-in / JSON-out async callable used by protocol-neutral tooling.
//!
//! Prefer [`CallableSkill`] when you need a type-erased async function object (for example
//! registering dynamic tools) without tying call sites to a specific concrete future type.

use std::pin::Pin;

use serde_json::Value;

use crate::SdkResult;

#[cfg(target_arch = "wasm32")]
type CallableFuture = dyn Future<Output = SdkResult<Value>>;
#[cfg(not(target_arch = "wasm32"))]
type CallableFuture = dyn Future<Output = SdkResult<Value>> + Send;

/// Boxed async callable: `serde_json::Value` in, [`SdkResult`] of JSON out.
///
/// On native targets the future is `Send`; on WASM it is single-threaded.
pub type CallableSkill = Box<dyn Fn(Value) -> Pin<Box<CallableFuture>> + Send + Sync>;
