//! Scoped operations and callback patterns
//!
//! This module provides high-level APIs for scoped operations that guarantee
//! setup and cleanup. Built on top of the RAII patterns but providing more
//! ergonomic APIs for common use cases.
//!
//! ## Key Features
//!
//! - **Scoped Callbacks**: Execute code with guaranteed setup/cleanup
//! - **Builder Pattern**: Fluent APIs for complex scoped operations
//! - **Exception Safety**: Cleanup happens even during panics
//! - **Return Values**: Scoped operations can return values from callbacks
//! - **Composable**: Scoped operations can be nested and combined

use crate::raii::Guard;

/// Execute a function with a scoped context
///
/// This is the most common scoped operation pattern. It sets up a context,
/// executes a function with that context, and automatically cleans up.
///
/// # Type Parameters
/// - `T`: The type of context
/// - `F`: The type of function to execute
/// - `R`: The return type of the function
///
/// # Arguments
/// - `context`: The context value to set up
/// - `f`: The function to execute with the context
///
/// # Example
/// ```rust
/// use foundation_utils::scoped::with_context;
///
/// let result = with_context("my_context", |ctx| {
///     println!("Working with context: {}", ctx);
///     42
/// });
/// assert_eq!(result, 42);
/// ```
pub fn with_context<T, F, R>(context: T, f: F) -> R
where
    F: FnOnce(&T) -> R,
{
    f(&context)
}

/// Execute a function with scoped setup and cleanup
///
/// This pattern provides explicit setup and cleanup functions that are
/// guaranteed to be called even if the work function panics.
///
/// # Type Parameters
/// - `S`: Setup function type
/// - `W`: Work function type  
/// - `C`: Cleanup function type
/// - `T`: Resource type returned by setup
/// - `R`: Return type of work function
///
/// # Arguments
/// - `setup`: Function to set up the resource
/// - `work`: Function to do work with the resource
/// - `cleanup`: Function to clean up the resource
///
/// # Example
/// ```rust
/// use foundation_utils::scoped::with_setup_cleanup;
///
/// let result = with_setup_cleanup(
///     || "resource",                    // Setup
///     |resource| format!("used: {}", resource), // Work
///     |resource| println!("cleanup: {}", resource) // Cleanup
/// );
/// ```
pub fn with_setup_cleanup<S, W, C, T, R>(setup: S, work: W, cleanup: C) -> R
where
    S: FnOnce() -> T,
    W: FnOnce(&T) -> R,
    C: FnOnce(T),
{
    let resource = setup();
    let _guard = Guard::new(resource, cleanup);
    work(_guard.resource())
}

/// Execute a function with optional scoped setup and cleanup
///
/// This is useful when setup/cleanup is conditional based on runtime conditions.
///
/// # Example
/// ```rust
/// use foundation_utils::scoped::with_optional_scope;
///
/// let should_setup = true;
/// let result = with_optional_scope(
///     should_setup,
///     || "resource",                    // Setup (only if condition is true)
///     |resource| format!("used: {:?}", resource), // Work
///     |resource| println!("cleanup: {}", resource) // Cleanup
/// );
/// ```
pub fn with_optional_scope<S, W, C, T, R>(condition: bool, setup: S, work: W, cleanup: C) -> R
where
    S: FnOnce() -> T,
    W: FnOnce(Option<&T>) -> R,
    C: FnOnce(T),
{
    if condition {
        let resource = setup();
        let _guard = Guard::new(resource, cleanup);
        work(Some(_guard.resource()))
    } else {
        work(None)
    }
}

/// Trait for scoped callback operations
///
/// This trait provides a standard interface for types that support
/// scoped operations with automatic cleanup.
pub trait ScopedCallback<T> {
    /// Execute a function with scoped access to the resource
    fn with_scope<F, R>(self, f: F) -> R
    where
        F: FnOnce(&T) -> R;
}

/// Builder for creating complex scoped operations
///
/// This builder allows you to compose multiple scoped operations
/// with different setup/cleanup phases.
///
/// # Example
/// ```rust
/// use foundation_utils::scoped::ScopedBuilder;
///
/// let result = ScopedBuilder::new()
///     .with_resource("resource1", |r| println!("cleanup: {}", r))
///     .with_resource("resource2", |r| println!("cleanup: {}", r))
///     .execute(|resources| {
///         format!("used {} resources", resources.len())
///     });
/// ```
pub struct ScopedBuilder<T> {
    resources: Vec<T>,
    cleanups: Vec<Box<dyn FnOnce(T)>>,
}

impl<T> ScopedBuilder<T> {
    /// Create a new scoped builder
    pub fn new() -> Self {
        Self {
            resources: Vec::new(),
            cleanups: Vec::new(),
        }
    }

    /// Add a resource with cleanup to the scoped operation
    ///
    /// # Arguments
    /// - `resource`: The resource to manage
    /// - `cleanup`: Function to clean up the resource when the scope ends
    pub fn with_resource<F>(mut self, resource: T, cleanup: F) -> Self
    where
        F: FnOnce(T) + 'static,
    {
        self.resources.push(resource);
        self.cleanups.push(Box::new(cleanup));
        self
    }

    /// Execute the scoped operation with all resources
    ///
    /// All cleanup functions will be called in reverse order (LIFO)
    /// even if the work function panics.
    pub fn execute<F, R>(mut self, work: F) -> R
    where
        F: FnOnce(&[T]) -> R,
    {
        struct CleanupGuard<T> {
            resources: Vec<T>,
            cleanups: Vec<Box<dyn FnOnce(T)>>,
        }

        impl<T> Drop for CleanupGuard<T> {
            fn drop(&mut self) {
                while let (Some(resource), Some(cleanup)) =
                    (self.resources.pop(), self.cleanups.pop())
                {
                    cleanup(resource);
                }
            }
        }

        let guard = CleanupGuard {
            resources: std::mem::take(&mut self.resources),
            cleanups: std::mem::take(&mut self.cleanups),
        };

        work(&guard.resources)
    }
}

impl<T> Default for ScopedBuilder<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// Scoped operation that automatically manages multiple contexts
///
/// This is a convenience function for managing multiple contexts
/// that need to be set up and torn down together.
///
/// # Example
/// ```rust
/// use foundation_utils::scoped::with_multiple_contexts;
///
/// let contexts = vec!["ctx1", "ctx2", "ctx3"];
/// let result = with_multiple_contexts(
///     contexts,
///     |ctx| println!("setup: {}", ctx),
///     |contexts| {
///         println!("Working with {} contexts", contexts.len());
///         42
///     },
///     |ctx| println!("cleanup: {}", ctx)
/// );
/// ```
pub fn with_multiple_contexts<T, S, W, C, R>(contexts: Vec<T>, setup: S, work: W, cleanup: C) -> R
where
    T: Clone,
    S: Fn(&T),
    W: FnOnce(&[T]) -> R,
    C: Fn(&T),
{
    // Set up all contexts
    for context in &contexts {
        setup(context);
    }

    // Create a guard that will clean up all contexts in reverse order
    let contexts_for_cleanup = contexts.clone();
    let _guard = Guard::new(contexts_for_cleanup, move |contexts| {
        for context in contexts.iter().rev() {
            cleanup(context);
        }
    });

    // Execute the work function
    work(&contexts)
}

/// Macro for creating scoped operations
///
/// This macro provides a convenient syntax for common scoped patterns.
///
/// # Example
/// ```rust
/// use foundation_utils::scoped_operation;
///
/// let result = scoped_operation! {
///     setup => || "resource",
///     work => |resource| format!("used: {}", resource),
///     cleanup => |resource| println!("cleanup: {}", resource)
/// };
/// ```
#[macro_export]
macro_rules! scoped_operation {
    (
        setup => $setup:expr,
        work => $work:expr,
        cleanup => $cleanup:expr
    ) => {
        $crate::scoped::with_setup_cleanup($setup, $work, $cleanup)
    };
}

/// Macro for creating context-based scoped operations
///
/// # Example
/// ```rust
/// use foundation_utils::with_scoped_context;
///
/// let result = with_scoped_context!("my_context", |ctx| {
///     println!("Context: {}", ctx);
///     42
/// });
/// ```
#[macro_export]
macro_rules! with_scoped_context {
    ($context:expr, $work:expr) => {
        $crate::scoped::with_context($context, $work)
    };
}

/// Helper trait for making types support scoped operations
///
/// Implement this trait to add scoped operation support to your types.
pub trait IntoScoped {
    type Resource;

    /// Convert into a scoped resource
    fn into_scoped(self) -> Self::Resource;

    /// Execute a scoped operation with this resource
    fn scoped<F, R>(self, f: F) -> R
    where
        Self: Sized,
        F: FnOnce(&Self::Resource) -> R,
    {
        let resource = self.into_scoped();
        f(&resource)
    }
}

// Implement IntoScoped for common types
impl<T> IntoScoped for T {
    type Resource = T;

    fn into_scoped(self) -> Self::Resource {
        self
    }
}

/// Convenience function for scope-aware error handling
///
/// This function allows you to handle errors within a scoped operation
/// while still ensuring cleanup happens.
///
/// # Example
/// ```rust
/// use foundation_utils::scoped::with_error_scope;
///
/// let result = with_error_scope(
///     || Ok("resource"),
///     |resource| {
///         // Work that might fail
///         if resource.len() > 0 {
///             Ok(format!("processed: {}", resource))
///         } else {
///             Err("empty resource")
///         }
///     },
///     |resource| println!("cleanup: {}", resource)
/// );
/// ```
pub fn with_error_scope<S, W, C, T, R, E>(setup: S, work: W, cleanup: C) -> Result<R, E>
where
    S: FnOnce() -> Result<T, E>,
    W: FnOnce(&T) -> Result<R, E>,
    C: FnOnce(T),
{
    let resource = setup()?;
    let _guard = Guard::new(resource, cleanup);
    work(_guard.resource())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    #[test]
    fn test_with_context() {
        let result = with_context(42, |ctx| *ctx + 10);
        assert_eq!(result, 52);
    }

    #[test]
    fn test_with_setup_cleanup() {
        let cleanup_called = Arc::new(AtomicUsize::new(0));
        let cleanup_called_clone = cleanup_called.clone();

        let result = with_setup_cleanup(
            || 42,
            |resource| *resource + 10,
            move |resource| {
                cleanup_called_clone.store(resource, Ordering::Relaxed);
            },
        );

        assert_eq!(result, 52);
        assert_eq!(cleanup_called.load(Ordering::Relaxed), 42);
    }

    #[test]
    fn test_with_optional_scope() {
        // Test with condition true
        let result = with_optional_scope(
            true,
            || 42,
            |resource| resource.map(|r| *r + 10).unwrap_or(0),
            |_| {}, // cleanup
        );
        assert_eq!(result, 52);

        // Test with condition false
        let result = with_optional_scope(
            false,
            || 42,
            |resource| resource.map(|r| *r + 10).unwrap_or(0),
            |_| {}, // cleanup
        );
        assert_eq!(result, 0);
    }

    #[test]
    fn test_scoped_builder() {
        let result = ScopedBuilder::new()
            .with_resource("test1", |_| {})
            .with_resource("test2", |_| {})
            .execute(|resources| resources.len());

        assert_eq!(result, 2);
    }

    #[test]
    fn test_with_multiple_contexts() {
        let setup_count = Arc::new(AtomicUsize::new(0));
        let cleanup_count = Arc::new(AtomicUsize::new(0));

        let setup_count_clone = setup_count.clone();
        let cleanup_count_clone = cleanup_count.clone();

        let contexts = vec![1, 2, 3];
        let result = with_multiple_contexts(
            contexts,
            move |_| {
                setup_count_clone.fetch_add(1, Ordering::Relaxed);
            },
            |contexts| contexts.len(),
            move |_| {
                cleanup_count_clone.fetch_add(1, Ordering::Relaxed);
            },
        );

        assert_eq!(result, 3);
        assert_eq!(setup_count.load(Ordering::Relaxed), 3);
        assert_eq!(cleanup_count.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn test_with_error_scope() {
        // Test success case
        let result: Result<i32, &str> = with_error_scope(
            || Ok(42),
            |resource| Ok(*resource + 10),
            |_| {}, // cleanup
        );
        assert_eq!(result, Ok(52));

        // Test error in setup
        let result: Result<i32, &str> = with_error_scope(
            || Err("setup failed"),
            |resource| Ok(*resource + 10),
            |_: i32| {}, // cleanup
        );
        assert_eq!(result, Err("setup failed"));

        // Test error in work
        let result: Result<i32, &str> = with_error_scope(
            || Ok(42),
            |_| Err("work failed"),
            |_| {}, // cleanup should still happen
        );
        assert_eq!(result, Err("work failed"));
    }

    #[test]
    fn test_into_scoped() {
        let result = 42.scoped(|resource| *resource + 10);
        assert_eq!(result, 52);
    }

    #[test]
    fn test_panic_safety_in_scoped_operations() {
        let cleanup_called = Arc::new(AtomicUsize::new(0));
        let cleanup_called_clone = cleanup_called.clone();

        let result = std::panic::catch_unwind(|| {
            with_setup_cleanup(
                || 42,
                |_| panic!("test panic"),
                move |resource| {
                    cleanup_called_clone.store(resource, Ordering::Relaxed);
                },
            )
        });

        assert!(result.is_err());
        assert_eq!(cleanup_called.load(Ordering::Relaxed), 42);
    }

    #[test]
    fn test_scoped_builder_cleanups_called() {
        let counter = Arc::new(AtomicUsize::new(0));
        let c1 = counter.clone();
        let c2 = counter.clone();
        let c3 = counter.clone();

        let result = ScopedBuilder::new()
            .with_resource(10, move |_| { c1.fetch_add(1, Ordering::SeqCst); })
            .with_resource(20, move |_| { c2.fetch_add(1, Ordering::SeqCst); })
            .with_resource(30, move |_| { c3.fetch_add(1, Ordering::SeqCst); })
            .execute(|resources| {
                assert_eq!(resources, &[10, 20, 30]);
                resources.iter().sum::<i32>()
            });

        assert_eq!(result, 60);
        assert_eq!(counter.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn test_scoped_builder_lifo_cleanup_order() {
        let order = Arc::new(Mutex::new(Vec::new()));
        let o1 = order.clone();
        let o2 = order.clone();
        let o3 = order.clone();

        ScopedBuilder::new()
            .with_resource("first", move |r| { o1.lock().unwrap().push(r); })
            .with_resource("second", move |r| { o2.lock().unwrap().push(r); })
            .with_resource("third", move |r| { o3.lock().unwrap().push(r); })
            .execute(|_| {});

        let cleaned = order.lock().unwrap();
        assert_eq!(&*cleaned, &["third", "second", "first"]);
    }

    #[test]
    fn test_scoped_builder_panic_safety() {
        let counter = Arc::new(AtomicUsize::new(0));
        let c1 = counter.clone();
        let c2 = counter.clone();

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            ScopedBuilder::new()
                .with_resource(1, move |_| { c1.fetch_add(1, Ordering::SeqCst); })
                .with_resource(2, move |_| { c2.fetch_add(1, Ordering::SeqCst); })
                .execute(|_| panic!("work panicked"))
        }));

        assert!(result.is_err());
        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }
}
