use std::future::Future;
use std::pin::Pin;

use serde_json::Value;

use crate::SdkResult;

#[cfg(target_arch = "wasm32")]
type CallableFuture = dyn Future<Output = SdkResult<Value>>;
#[cfg(not(target_arch = "wasm32"))]
type CallableFuture = dyn Future<Output = SdkResult<Value>> + Send;

/// Protocol-neutral async callable that accepts JSON input and returns JSON output.
pub type CallableSkill = Box<dyn Fn(Value) -> Pin<Box<CallableFuture>> + Send + Sync>;
