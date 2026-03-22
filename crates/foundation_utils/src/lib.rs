//! # Foundation Utils
//!
//! Core foundational utilities for the PromptFleet Agents project.
//! Provides zero-dependency, WASM-compatible patterns for:
//!
//! - **RAII Guards**: Automatic resource cleanup with Drop trait
//! - **Scoped Operations**: Guaranteed setup/teardown patterns  
//! - **Context Management**: Thread-safe context propagation
//! - **Resource Management**: Safe resource acquisition/release
//!
//! ## Design Principles
//!
//! - **Zero Dependencies**: Pure Rust stdlib only
//! - **WASM Compatible**: Designed for WebAssembly environments
//! - **Zero Cost**: Compile-time abstractions with no runtime overhead
//! - **Exception Safe**: Automatic cleanup even during panics
//! - **Type Safe**: Compiler-enforced correctness
//!
//! ## Usage
//!
//! ```rust
//! use foundation_utils::raii::Guard;
//! use foundation_utils::scoped::with_context;
//!
//! // RAII guard for automatic cleanup
//! let _guard = Guard::new(resource, |r| cleanup(r));
//!
//! // Scoped operation with guaranteed cleanup
//! let result = with_context(setup_value, |ctx| {
//!     // Work with context
//!     process(ctx)
//! }); // Context automatically cleaned up
//! ```

pub mod context;
pub mod raii;
pub mod resource;
pub mod scoped;

// Re-export commonly used types
pub use context::{ContextManager, ThreadLocalContext};
pub use raii::{Guard, ScopedGuard};
pub use resource::{ResourceGuard, ResourcePool};
pub use scoped::{with_context, ScopedBuilder, ScopedCallback};

/// Version of the foundation utils
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Create a simple RAII guard with cleanup function
///
/// This is the most basic guard pattern - useful for any resource that needs cleanup
///
/// # Example
/// ```rust
/// use foundation_utils::guard;
///
/// let _guard = guard(resource, |r| {
///     println!("Cleaning up: {:?}", r);
/// });
/// // Cleanup happens automatically when guard drops
/// ```
pub fn guard<T, F>(resource: T, cleanup: F) -> Guard<T, F>
where
    F: FnOnce(T),
{
    Guard::new(resource, cleanup)
}

/// Execute a function with scoped setup and cleanup
///
/// This pattern guarantees that cleanup happens even if the function panics
///
/// # Example
/// ```rust
/// use foundation_utils::with_scoped;
///
/// let result = with_scoped(
///     || setup_resource(),           // Setup
///     |resource| process(resource),  // Work  
///     |resource| cleanup(resource)   // Cleanup
/// );
/// ```
pub fn with_scoped<S, W, C, T, R>(setup: S, work: W, cleanup: C) -> R
where
    S: FnOnce() -> T,
    W: FnOnce(&T) -> R,
    C: FnOnce(T),
{
    let resource = setup();
    let _guard = guard(resource, cleanup);
    work(&_guard.resource())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn test_basic_guard() {
        let cleanup_called = Arc::new(Mutex::new(false));
        let cleanup_called_clone = cleanup_called.clone();

        {
            let _guard = guard("test_resource", move |_| {
                *cleanup_called_clone.lock().unwrap() = true;
            });

            // Guard is active
            assert!(!*cleanup_called.lock().unwrap());
        } // Guard drops here

        // Cleanup should have been called
        assert!(*cleanup_called.lock().unwrap());
    }

    #[test]
    fn test_with_scoped() {
        let cleanup_called = Arc::new(Mutex::new(false));
        let cleanup_called_clone = cleanup_called.clone();

        let result = with_scoped(
            || "test_resource",
            |resource| format!("processed: {}", resource),
            move |_| {
                *cleanup_called_clone.lock().unwrap() = true;
            },
        );

        assert_eq!(result, "processed: test_resource");
        assert!(*cleanup_called.lock().unwrap());
    }
}
