#![cfg(not(target_arch = "wasm32"))]

#[cfg(feature = "a2a-http")]
pub mod a2a_http;
#[cfg(feature = "a2a-mock")]
pub mod a2a_mock;
#[cfg(feature = "agent-sdk")]
pub mod perf;
#[cfg(feature = "agent-sdk")]
pub mod pipeline;
#[cfg(feature = "scenario")]
pub mod scenario;
#[cfg(feature = "sse")]
pub mod sse;
