//! Context management patterns
//!
//! This module provides thread-safe context management for WASM and native environments.
//! Contexts are used to propagate information across function calls without explicit
//! parameter passing.
//!
//! ## Key Features
//!
//! - **Thread-Local Context**: WASM-compatible thread-local storage
//! - **Scoped Context**: Automatic context cleanup with RAII
//! - **Context Inheritance**: Child contexts inherit from parent contexts
//! - **Type Safety**: Strongly typed context values
//! - **Performance**: Zero-cost abstractions with compile-time optimization

use crate::raii::Guard;
use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

/// Thread-local context storage
///
/// This provides WASM-compatible thread-local context storage that automatically
/// cleans up when contexts go out of scope.
pub struct ThreadLocalContext;

thread_local! {
    static CONTEXT_STORAGE: std::cell::RefCell<HashMap<TypeId, Box<dyn Any>>> =
        std::cell::RefCell::new(HashMap::new());
}

impl ThreadLocalContext {
    /// Create a new thread-local context
    pub fn new() -> Self {
        Self
    }

    /// Set a typed value in the context
    pub fn set<T: 'static>(&self, value: T) {
        CONTEXT_STORAGE.with(|storage| {
            storage
                .borrow_mut()
                .insert(TypeId::of::<T>(), Box::new(value));
        });
    }

    /// Get a typed value from the context
    pub fn get<T: 'static + Clone>(&self) -> Option<T> {
        CONTEXT_STORAGE.with(|storage| {
            storage
                .borrow()
                .get(&TypeId::of::<T>())
                .and_then(|any| any.downcast_ref::<T>())
                .cloned()
        })
    }

    /// Remove a typed value from the context
    pub fn remove<T: 'static>(&self) -> Option<T> {
        CONTEXT_STORAGE.with(|storage| {
            storage
                .borrow_mut()
                .remove(&TypeId::of::<T>())
                .and_then(|any| any.downcast::<T>().ok())
                .map(|boxed| *boxed)
        })
    }

    /// Clear all context values
    pub fn clear(&self) {
        CONTEXT_STORAGE.with(|storage| {
            storage.borrow_mut().clear();
        });
    }

    /// Create a scoped context guard for a typed value
    pub fn scoped<T: 'static + Clone>(&self, value: T) -> Guard<T, impl FnOnce(T) + use<T>> {
        let previous = self.get::<T>();
        self.set(value.clone());

        Guard::new(value, move |_| {
            if let Some(prev) = previous {
                CONTEXT_STORAGE.with(|storage| {
                    storage
                        .borrow_mut()
                        .insert(TypeId::of::<T>(), Box::new(prev));
                });
            } else {
                CONTEXT_STORAGE.with(|storage| {
                    storage.borrow_mut().remove(&TypeId::of::<T>());
                });
            }
        })
    }
}

impl Default for ThreadLocalContext {
    fn default() -> Self {
        Self::new()
    }
}

/// Global context manager for shared state
///
/// This provides a global context that can be shared across threads
/// in environments that support it (not available in WASM).
pub struct GlobalContext {
    storage: Arc<RwLock<HashMap<TypeId, Arc<dyn Any + Send + Sync>>>>,
}

impl GlobalContext {
    /// Create a new global context
    pub fn new() -> Self {
        Self {
            storage: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Set a typed value in the global context
    pub fn set<T: 'static + Send + Sync>(&self, value: T) {
        let mut storage = self.storage.write().unwrap();
        storage.insert(TypeId::of::<T>(), Arc::new(value));
    }

    /// Get a typed value from the global context
    pub fn get<T: 'static + Send + Sync + Clone>(&self) -> Option<T> {
        let storage = self.storage.read().unwrap();
        storage
            .get(&TypeId::of::<T>())
            .and_then(|any| any.downcast_ref::<T>())
            .cloned()
    }

    /// Remove a typed value from the global context
    pub fn remove<T: 'static + Send + Sync>(&self) -> bool {
        let mut storage = self.storage.write().unwrap();
        storage.remove(&TypeId::of::<T>()).is_some()
    }

    /// Clear all global context values
    pub fn clear(&self) {
        let mut storage = self.storage.write().unwrap();
        storage.clear();
    }
}

impl Default for GlobalContext {
    fn default() -> Self {
        Self::new()
    }
}

/// Context manager trait for different context implementations
///
/// This trait provides a common interface for different types of context storage.
pub trait ContextManager: Send + Sync {
    /// Set a string value in the context
    fn set_string(&self, key: &str, value: String);

    /// Get a string value from the context
    fn get_string(&self, key: &str) -> Option<String>;

    /// Remove a string value from the context
    fn remove_string(&self, key: &str) -> bool;

    /// Clear all context values
    fn clear_all(&self);
}

/// Simple hash map based context manager
///
/// This is useful for basic context management where thread safety
/// is handled externally.
pub struct HashMapContext {
    storage: Arc<Mutex<HashMap<String, String>>>,
}

impl HashMapContext {
    /// Create a new hash map context
    pub fn new() -> Self {
        Self {
            storage: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl Default for HashMapContext {
    fn default() -> Self {
        Self::new()
    }
}

impl ContextManager for HashMapContext {
    fn set_string(&self, key: &str, value: String) {
        let mut storage = self.storage.lock().unwrap();
        storage.insert(key.to_string(), value);
    }

    fn get_string(&self, key: &str) -> Option<String> {
        let storage = self.storage.lock().unwrap();
        storage.get(key).cloned()
    }

    fn remove_string(&self, key: &str) -> bool {
        let mut storage = self.storage.lock().unwrap();
        storage.remove(key).is_some()
    }

    fn clear_all(&self) {
        let mut storage = self.storage.lock().unwrap();
        storage.clear();
    }
}

// Convenience functions for working with a global thread-local context
thread_local! {
    static GLOBAL_THREAD_CONTEXT: ThreadLocalContext = ThreadLocalContext::new();
}

/// Set a typed value in the global thread-local context
pub fn set_context<T: 'static>(value: T) {
    GLOBAL_THREAD_CONTEXT.with(|ctx| ctx.set(value));
}

/// Get a typed value from the global thread-local context
pub fn get_context<T: 'static + Clone>() -> Option<T> {
    GLOBAL_THREAD_CONTEXT.with(|ctx| ctx.get())
}

/// Remove a typed value from the global thread-local context
pub fn remove_context<T: 'static>() -> Option<T> {
    GLOBAL_THREAD_CONTEXT.with(|ctx| ctx.remove())
}

/// Clear all values from the global thread-local context
pub fn clear_context() {
    GLOBAL_THREAD_CONTEXT.with(|ctx| ctx.clear());
}

/// Create a scoped context guard for the global thread-local context
pub fn scoped_context<T: 'static + Clone>(value: T) -> impl Drop {
    GLOBAL_THREAD_CONTEXT.with(|ctx| ctx.scoped(value))
}

/// Execute a function with a scoped context value
pub fn with_context_value<T, F, R>(value: T, f: F) -> R
where
    T: 'static + Clone,
    F: FnOnce() -> R,
{
    let _guard = scoped_context(value);
    f()
}

/// Context key for type-safe context access
///
/// This provides a type-safe way to access context values without
/// relying on string keys.
pub struct ContextKey<T> {
    name: &'static str,
    _phantom: std::marker::PhantomData<T>,
}

impl<T> ContextKey<T> {
    /// Create a new context key
    pub const fn new(name: &'static str) -> Self {
        Self {
            name,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Get the key name
    pub fn name(&self) -> &'static str {
        self.name
    }
}

impl<T> Clone for ContextKey<T> {
    fn clone(&self) -> Self {
        Self {
            name: self.name,
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<T> Copy for ContextKey<T> {}

/// Macro for creating context keys
///
/// This macro creates a typed context key that can be used for type-safe
/// context access.
///
/// # Example
/// ```rust
/// use foundation_utils::context_key;
///
/// context_key!(USER_ID, String);
/// context_key!(REQUEST_ID, String);
/// context_key!(TRACE_ID, String);
/// ```
#[macro_export]
macro_rules! context_key {
    ($name:ident, $type:ty) => {
        pub const $name: $crate::context::ContextKey<$type> =
            $crate::context::ContextKey::new(stringify!($name));
    };
}

/// Macro for scoped context operations
///
/// This macro provides a convenient way to set context values for a scope.
///
/// # Example
/// ```rust
/// use foundation_utils::{with_context_scoped, context_key};
///
/// context_key!(USER_ID, String);
///
/// let result = with_context_scoped!(USER_ID, "user123".to_string(), {
///     // Work with context
///     42
/// });
/// ```
#[macro_export]
macro_rules! with_context_scoped {
    ($key:expr_2021, $value:expr_2021, $block:block) => {
        $crate::context::with_context_value($value, || $block)
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_thread_local_context() {
        let ctx = ThreadLocalContext::new();

        // Test setting and getting values
        ctx.set(42i32);
        ctx.set("hello".to_string());

        assert_eq!(ctx.get::<i32>(), Some(42));
        assert_eq!(ctx.get::<String>(), Some("hello".to_string()));
        assert_eq!(ctx.get::<f64>(), None);

        // Test removing values
        assert_eq!(ctx.remove::<i32>(), Some(42));
        assert_eq!(ctx.get::<i32>(), None);

        // Test clearing
        ctx.clear();
        assert_eq!(ctx.get::<String>(), None);
    }

    #[test]
    fn test_global_context() {
        let ctx = GlobalContext::new();

        // Test setting and getting values
        ctx.set(42i32);
        ctx.set("hello".to_string());

        assert_eq!(ctx.get::<i32>(), Some(42));
        assert_eq!(ctx.get::<String>(), Some("hello".to_string()));
        assert_eq!(ctx.get::<f64>(), None);

        // Test removing values
        assert!(ctx.remove::<i32>());
        assert_eq!(ctx.get::<i32>(), None);
        assert!(!ctx.remove::<i32>()); // Already removed

        // Test clearing
        ctx.clear();
        assert_eq!(ctx.get::<String>(), None);
    }

    #[test]
    fn test_hashmap_context() {
        let ctx = HashMapContext::new();

        // Test setting and getting values
        ctx.set_string("key1", "value1".to_string());
        ctx.set_string("key2", "value2".to_string());

        assert_eq!(ctx.get_string("key1"), Some("value1".to_string()));
        assert_eq!(ctx.get_string("key2"), Some("value2".to_string()));
        assert_eq!(ctx.get_string("key3"), None);

        // Test removing values
        assert!(ctx.remove_string("key1"));
        assert_eq!(ctx.get_string("key1"), None);
        assert!(!ctx.remove_string("key1")); // Already removed

        // Test clearing
        ctx.clear_all();
        assert_eq!(ctx.get_string("key2"), None);
    }

    #[test]
    fn test_scoped_context() {
        // Set initial value
        set_context(42i32);
        assert_eq!(get_context::<i32>(), Some(42));

        {
            // Create scoped context
            let _guard = scoped_context(100i32);
            assert_eq!(get_context::<i32>(), Some(100));
        } // Guard drops here

        // Should restore previous value
        assert_eq!(get_context::<i32>(), Some(42));

        // Clean up
        clear_context();
        assert_eq!(get_context::<i32>(), None);
    }

    #[test]
    fn test_with_context_value() {
        // Test scoped context with function
        let result = with_context_value(42i32, || get_context::<i32>().unwrap() + 10);

        assert_eq!(result, 52);

        // Context should be cleared after function
        assert_eq!(get_context::<i32>(), None);
    }

    #[test]
    fn test_nested_scoped_context() {
        set_context(10i32);

        let result = with_context_value(20i32, || {
            let inner_result = with_context_value(30i32, || get_context::<i32>().unwrap());

            assert_eq!(inner_result, 30);
            get_context::<i32>().unwrap()
        });

        assert_eq!(result, 20);
        assert_eq!(get_context::<i32>(), Some(10));

        clear_context();
    }

    #[test]
    fn test_context_key() {
        context_key!(USER_ID, String);
        context_key!(SESSION_ID, i64);

        assert_eq!(USER_ID.name(), "USER_ID");
        assert_eq!(SESSION_ID.name(), "SESSION_ID");
    }

    #[test]
    fn test_panic_safety() {
        set_context(42i32);

        let result = std::panic::catch_unwind(|| {
            with_context_value(100i32, || {
                panic!("test panic");
            })
        });

        assert!(result.is_err());

        // Original context should be restored
        assert_eq!(get_context::<i32>(), Some(42));

        clear_context();
    }
}
