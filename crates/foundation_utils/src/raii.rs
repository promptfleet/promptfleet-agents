//! RAII (Resource Acquisition Is Initialization) patterns
//!
//! This module provides generic RAII guard patterns that ensure automatic
//! resource cleanup when guards go out of scope. Based on the proven patterns
//! from observability_core but generalized for project-wide use.
//!
//! ## Key Features
//!
//! - **Automatic Cleanup**: Resources are cleaned up when guards drop
//! - **Exception Safety**: Cleanup happens even during panics
//! - **Zero Cost**: Pure compile-time abstractions
//! - **Type Safe**: Compile-time guarantees of correctness
//! - **WASM Compatible**: No external dependencies

use std::fmt;
use std::sync::{Arc, Mutex};

/// Generic RAII guard that owns a resource and cleans it up on drop
///
/// This is the fundamental building block for all RAII patterns in the project.
/// It ensures that resources are properly cleaned up even if panics occur.
///
/// # Type Parameters
/// - `T`: The type of resource being managed
/// - `F`: The type of cleanup function (must be `FnOnce(T)`)
///
/// # Example
/// ```rust
/// use foundation_utils::raii::Guard;
///
/// let _guard = Guard::new(42, |value| {
///     println!("Cleaning up value: {}", value);
/// });
/// // Cleanup happens automatically when guard drops
/// ```
pub struct Guard<T, F>
where
    F: FnOnce(T),
{
    resource: Option<T>,
    cleanup: Option<F>,
}

impl<T, F> Guard<T, F>
where
    F: FnOnce(T),
{
    /// Create a new RAII guard
    ///
    /// # Arguments
    /// - `resource`: The resource to manage
    /// - `cleanup`: Function to call when the guard drops
    ///
    /// # Example
    /// ```rust
    /// use foundation_utils::raii::Guard;
    ///
    /// let guard = Guard::new("resource", |r| println!("Cleanup: {}", r));
    /// ```
    pub fn new(resource: T, cleanup: F) -> Self {
        Self {
            resource: Some(resource),
            cleanup: Some(cleanup),
        }
    }

    /// Get a reference to the managed resource
    ///
    /// # Panics
    /// Panics if the guard has already been consumed (should not happen in normal use)
    pub fn resource(&self) -> &T {
        self.resource
            .as_ref()
            .expect("Guard resource already consumed")
    }

    /// Get a mutable reference to the managed resource
    ///
    /// # Panics
    /// Panics if the guard has already been consumed (should not happen in normal use)
    pub fn resource_mut(&mut self) -> &mut T {
        self.resource
            .as_mut()
            .expect("Guard resource already consumed")
    }

    /// Consume the guard and return the resource without cleanup
    ///
    /// This is useful when you want to transfer ownership without cleanup
    pub fn into_inner(mut self) -> T {
        let resource = self
            .resource
            .take()
            .expect("Guard resource already consumed");
        self.cleanup.take(); // Prevent cleanup from running
        resource
    }

    /// Manually trigger cleanup now (guard becomes invalid after this)
    ///
    /// This allows explicit cleanup before the guard would naturally drop
    pub fn cleanup_now(mut self) {
        if let (Some(resource), Some(cleanup)) = (self.resource.take(), self.cleanup.take()) {
            cleanup(resource);
        }
    }
}

impl<T, F> Drop for Guard<T, F>
where
    F: FnOnce(T),
{
    fn drop(&mut self) {
        if let (Some(resource), Some(cleanup)) = (self.resource.take(), self.cleanup.take()) {
            cleanup(resource);
        }
    }
}

impl<T, F> fmt::Debug for Guard<T, F>
where
    T: fmt::Debug,
    F: FnOnce(T),
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Guard")
            .field("resource", &self.resource)
            .field("cleanup", &"<function>")
            .finish()
    }
}

/// Trait for types that can create scoped guards
///
/// This trait provides a standard interface for creating RAII guards
/// for different types of resources.
pub trait ScopedGuard {
    type Resource;
    type Guard;

    /// Create a scoped guard for this resource
    fn scoped(resource: Self::Resource) -> Self::Guard;
}

/// Specialized guard for managing shared state
///
/// This guard is useful for managing Arc<Mutex<T>> resources where
/// you want to ensure proper locking and unlocking.
pub struct SharedGuard<T> {
    resource: Option<Arc<Mutex<T>>>,
    was_locked: bool,
}

impl<T> SharedGuard<T> {
    /// Create a new shared guard
    ///
    /// # Arguments
    /// - `resource`: The Arc<Mutex<T>> to manage
    ///
    /// # Example
    /// ```rust
    /// use foundation_utils::raii::SharedGuard;
    /// use std::sync::{Arc, Mutex};
    ///
    /// let shared_data = Arc::new(Mutex::new(42));
    /// let _guard = SharedGuard::new(shared_data);
    /// // Mutex is automatically handled
    /// ```
    pub fn new(resource: Arc<Mutex<T>>) -> Self {
        Self {
            resource: Some(resource),
            was_locked: false,
        }
    }

    /// Lock the mutex and get a reference to the data
    ///
    /// This is a convenience method that handles the lock/unlock pattern
    pub fn lock(
        &mut self,
    ) -> Result<std::sync::MutexGuard<T>, std::sync::PoisonError<std::sync::MutexGuard<T>>> {
        if let Some(ref resource) = self.resource {
            self.was_locked = true;
            resource.lock()
        } else {
            panic!("SharedGuard resource already consumed")
        }
    }

    /// Get the shared resource without locking
    pub fn resource(&self) -> &Arc<Mutex<T>> {
        self.resource
            .as_ref()
            .expect("SharedGuard resource already consumed")
    }
}

impl<T> Drop for SharedGuard<T> {
    fn drop(&mut self) {
        // In this case, the MutexGuard handles unlocking automatically
        // This guard is mainly for resource lifetime management
        self.resource.take();
    }
}

/// Context guard that manages thread-local or scoped context
///
/// This is a specialized guard for managing context that needs to be
/// set and cleared in a scoped manner.
pub struct ContextGuard<T, S, C>
where
    S: FnOnce(&T),
    C: FnOnce(),
{
    context: Option<T>,
    setter: Option<S>,
    clearer: Option<C>,
    was_set: bool,
}

impl<T, S, C> ContextGuard<T, S, C>
where
    S: FnOnce(&T),
    C: FnOnce(),
{
    /// Create a new context guard
    ///
    /// # Arguments
    /// - `context`: The context value to manage
    /// - `setter`: Function to set the context
    /// - `clearer`: Function to clear the context
    ///
    /// # Example
    /// ```rust
    /// use foundation_utils::raii::ContextGuard;
    ///
    /// let _guard = ContextGuard::new(
    ///     "my_context",
    ///     |ctx| set_global_context(ctx),
    ///     || clear_global_context()
    /// );
    /// // Context is automatically cleared when guard drops
    /// ```
    pub fn new(context: T, setter: S, clearer: C) -> Self {
        let mut guard = Self {
            context: Some(context),
            setter: Some(setter),
            clearer: Some(clearer),
            was_set: false,
        };

        // Set the context immediately
        if let (Some(ref context), Some(setter)) = (&guard.context, guard.setter.take()) {
            setter(context);
            guard.was_set = true;
        }

        guard
    }

    /// Get a reference to the context
    pub fn context(&self) -> &T {
        self.context
            .as_ref()
            .expect("ContextGuard context already consumed")
    }
}

impl<T, S, C> Drop for ContextGuard<T, S, C>
where
    S: FnOnce(&T),
    C: FnOnce(),
{
    fn drop(&mut self) {
        if self.was_set {
            if let Some(clearer) = self.clearer.take() {
                clearer();
            }
        }
        self.context.take();
    }
}

/// No-op guard that does nothing
///
/// This is useful as a placeholder when guards are conditionally created
/// but you still want to maintain the same interface.
pub struct NoOpGuard;

impl NoOpGuard {
    /// Create a new no-op guard
    pub fn new() -> Self {
        Self
    }
}

impl Default for NoOpGuard {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for NoOpGuard {
    fn drop(&mut self) {
        // No-op
    }
}

/// Macro to create a simple RAII guard
///
/// This macro provides a convenient way to create guards with inline cleanup logic.
///
/// # Example
/// ```rust
/// use foundation_utils::raii_guard;
///
/// let _guard = raii_guard!(resource, |r| {
///     println!("Cleaning up: {:?}", r);
/// });
/// ```
#[macro_export]
macro_rules! raii_guard {
    ($resource:expr, $cleanup:expr) => {
        $crate::raii::Guard::new($resource, $cleanup)
    };
}

/// Macro to create a context guard
///
/// This macro provides a convenient way to create context guards with
/// setup and cleanup logic.
///
/// # Example
/// ```rust
/// use foundation_utils::context_guard;
///
/// let _guard = context_guard!(
///     "my_context",
///     |ctx| set_context(ctx),
///     || clear_context()
/// );
/// ```
#[macro_export]
macro_rules! context_guard {
    ($context:expr, $setter:expr, $clearer:expr) => {
        $crate::raii::ContextGuard::new($context, $setter, $clearer)
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};

    #[test]
    fn test_basic_guard() {
        let cleanup_called = Arc::new(AtomicBool::new(false));
        let cleanup_called_clone = cleanup_called.clone();

        {
            let guard = Guard::new(42, move |value| {
                assert_eq!(value, 42);
                cleanup_called_clone.store(true, Ordering::Relaxed);
            });

            assert_eq!(*guard.resource(), 42);
            assert!(!cleanup_called.load(Ordering::Relaxed));
        } // Guard drops here

        assert!(cleanup_called.load(Ordering::Relaxed));
    }

    #[test]
    fn test_guard_into_inner() {
        let cleanup_called = Arc::new(AtomicBool::new(false));
        let cleanup_called_clone = cleanup_called.clone();

        let guard = Guard::new(42, move |_| {
            cleanup_called_clone.store(true, Ordering::Relaxed);
        });

        let value = guard.into_inner();
        assert_eq!(value, 42);

        // Cleanup should NOT have been called
        assert!(!cleanup_called.load(Ordering::Relaxed));
    }

    #[test]
    fn test_guard_manual_cleanup() {
        let cleanup_called = Arc::new(AtomicBool::new(false));
        let cleanup_called_clone = cleanup_called.clone();

        let guard = Guard::new(42, move |_| {
            cleanup_called_clone.store(true, Ordering::Relaxed);
        });

        guard.cleanup_now();

        // Cleanup should have been called
        assert!(cleanup_called.load(Ordering::Relaxed));
    }

    #[test]
    fn test_shared_guard() {
        let data = Arc::new(Mutex::new(42));
        let mut guard = SharedGuard::new(data.clone());

        {
            let locked_data = guard.lock().unwrap();
            assert_eq!(*locked_data, 42);
        } // MutexGuard drops here, automatically unlocking

        // Guard is still valid
        let another_lock = guard.lock().unwrap();
        assert_eq!(*another_lock, 42);
    }

    #[test]
    fn test_context_guard() {
        let context_set = Arc::new(AtomicBool::new(false));
        let context_cleared = Arc::new(AtomicBool::new(false));

        let context_set_clone = context_set.clone();
        let context_cleared_clone = context_cleared.clone();

        {
            let _guard = ContextGuard::new(
                "test_context",
                move |_| {
                    context_set_clone.store(true, Ordering::Relaxed);
                },
                move || {
                    context_cleared_clone.store(true, Ordering::Relaxed);
                },
            );

            // Context should be set
            assert!(context_set.load(Ordering::Relaxed));
            assert!(!context_cleared.load(Ordering::Relaxed));
        } // Guard drops here

        // Context should be cleared
        assert!(context_cleared.load(Ordering::Relaxed));
    }

    #[test]
    fn test_panic_safety() {
        let cleanup_called = Arc::new(AtomicBool::new(false));
        let cleanup_called_clone = cleanup_called.clone();

        let result = std::panic::catch_unwind(|| {
            let _guard = Guard::new(42, move |_| {
                cleanup_called_clone.store(true, Ordering::Relaxed);
            });

            panic!("Test panic");
        });

        assert!(result.is_err());
        // Cleanup should still have been called
        assert!(cleanup_called.load(Ordering::Relaxed));
    }

    #[test]
    fn test_no_op_guard() {
        let _guard = NoOpGuard::new();
        // Should not panic or do anything
    }

    #[test]
    fn test_macros() {
        let cleanup_called = Arc::new(AtomicBool::new(false));
        let cleanup_called_clone = cleanup_called.clone();

        {
            let _guard = raii_guard!(42, move |_| {
                cleanup_called_clone.store(true, Ordering::Relaxed);
            });
        }

        assert!(cleanup_called.load(Ordering::Relaxed));
    }
}
