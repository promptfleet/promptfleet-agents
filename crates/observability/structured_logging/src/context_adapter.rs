//! Advanced context management for structured_logging
//!
//! This module provides domain-specific context management capabilities that build on
//! observability_core's basic trace context foundation. It includes:
//! - RAII scoped context management
//! - Domain-specific context managers (LLM, A2A, Request)
//! - Dependency injection registry
//! - Advanced correlation features

use crate::convenience::{
    clear_a2a_context, clear_llm_context, clear_request_context, set_a2a_context, set_llm_context,
    set_request_context,
};
use crate::error::{Result, StructuredLoggingError};
use observability_core::context::TraceContext;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

// ==================== ADVANCED CONTEXT MANAGEMENT PORTS ====================

/// Port (trait) for LLM context management
///
/// Extensions like structured_logging can implement this trait to provide
/// concrete context management functionality.
pub trait LlmContextManager: Send + Sync + std::fmt::Debug {
    fn set_context(&self, model: &str, component: &str);
    fn clear_context(&self);
}

/// Port (trait) for A2A context management
pub trait A2aContextManager: Send + Sync + std::fmt::Debug {
    fn set_context(&self, message_type: &str, from_agent: &str, to_agent: &str, component: &str);
    fn clear_context(&self);
}

/// Port (trait) for request context management
pub trait RequestContextManager: Send + Sync + std::fmt::Debug {
    fn set_context(&self, request_id: &str, user_id: Option<&str>, session_id: Option<&str>);
    fn clear_context(&self);
}

/// Combined context manager that coordinates all context types
pub trait ContextManagerRegistry: Send + Sync + std::fmt::Debug {
    fn get_llm_manager(&self) -> Option<Arc<dyn LlmContextManager>>;
    fn get_a2a_manager(&self) -> Option<Arc<dyn A2aContextManager>>;
    fn get_request_manager(&self) -> Option<Arc<dyn RequestContextManager>>;
}

// ==================== GLOBAL REGISTRY ====================

/// Global registry for context managers (dependency injection)
static CONTEXT_REGISTRY: OnceLock<Arc<dyn ContextManagerRegistry>> = OnceLock::new();

/// Register a context manager registry (dependency injection)
///
/// This allows structured_logging to register its context managers
pub fn register_context_managers(registry: Arc<dyn ContextManagerRegistry>) {
    CONTEXT_REGISTRY.set(registry).ok();
}

/// Get the current context manager registry
fn get_context_registry() -> Option<Arc<dyn ContextManagerRegistry>> {
    CONTEXT_REGISTRY.get().cloned()
}

// ==================== CONCRETE IMPLEMENTATIONS ====================

/// Adapter implementing LLM context management for structured_logging
#[derive(Debug)]
pub struct StructuredLlmContextManager;

impl LlmContextManager for StructuredLlmContextManager {
    fn set_context(&self, model: &str, component: &str) {
        set_llm_context(model, component);
    }

    fn clear_context(&self) {
        clear_llm_context();
    }
}

/// Adapter implementing A2A context management for structured_logging
#[derive(Debug)]
pub struct StructuredA2aContextManager;

impl A2aContextManager for StructuredA2aContextManager {
    fn set_context(&self, message_type: &str, from_agent: &str, to_agent: &str, component: &str) {
        set_a2a_context(message_type, from_agent, to_agent, component);
    }

    fn clear_context(&self) {
        clear_a2a_context();
    }
}

/// Adapter implementing request context management for structured_logging
#[derive(Debug)]
pub struct StructuredRequestContextManager;

impl RequestContextManager for StructuredRequestContextManager {
    fn set_context(&self, request_id: &str, user_id: Option<&str>, session_id: Option<&str>) {
        set_request_context(request_id, user_id, session_id);
    }

    fn clear_context(&self) {
        clear_request_context();
    }
}

/// Registry coordinating all structured_logging context managers
#[derive(Debug)]
pub struct StructuredContextRegistry {
    llm_manager: Option<Arc<dyn LlmContextManager>>,
    a2a_manager: Option<Arc<dyn A2aContextManager>>,
    request_manager: Option<Arc<dyn RequestContextManager>>,
}

impl StructuredContextRegistry {
    /// Create a new registry with all structured_logging context managers
    pub fn new() -> Self {
        Self {
            llm_manager: Some(Arc::new(StructuredLlmContextManager)),
            a2a_manager: Some(Arc::new(StructuredA2aContextManager)),
            request_manager: Some(Arc::new(StructuredRequestContextManager)),
        }
    }

    /// Register this registry with the global context system
    pub fn register(&self) {
        let registry = Arc::new(Self {
            llm_manager: self.llm_manager.clone(),
            a2a_manager: self.a2a_manager.clone(),
            request_manager: self.request_manager.clone(),
        });

        register_context_managers(registry);
    }
}

impl Default for StructuredContextRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ContextManagerRegistry for StructuredContextRegistry {
    fn get_llm_manager(&self) -> Option<Arc<dyn LlmContextManager>> {
        self.llm_manager.clone()
    }

    fn get_a2a_manager(&self) -> Option<Arc<dyn A2aContextManager>> {
        self.a2a_manager.clone()
    }

    fn get_request_manager(&self) -> Option<Arc<dyn RequestContextManager>> {
        self.request_manager.clone()
    }
}

// ==================== RAII SCOPED CONTEXT MANAGEMENT ====================

/// RAII guard for LLM context
///
/// When this guard is dropped, the LLM context is automatically cleared.
/// This ensures exception-safe cleanup and prevents forgetting to clear context.
///
/// # Example
/// ```rust
/// {
///     let _guard = set_llm_context_scoped("gpt-4", "openai_client");
///     log::info!("This will have LLM context");
///     // Guard automatically clears context when it goes out of scope
/// }
/// log::info!("This will NOT have LLM context");
/// ```
pub struct LlmContextGuard {
    _phantom: std::marker::PhantomData<()>,
}

impl Drop for LlmContextGuard {
    fn drop(&mut self) {
        // Use dependency injection to clear context
        if let Some(registry) = get_context_registry() {
            if let Some(manager) = registry.get_llm_manager() {
                manager.clear_context();
            }
        }
    }
}

/// RAII guard for A2A context
///
/// Automatically clears A2A context when dropped, providing exception-safe cleanup.
pub struct A2aContextGuard {
    _phantom: std::marker::PhantomData<()>,
}

impl Drop for A2aContextGuard {
    fn drop(&mut self) {
        // Use dependency injection to clear context
        if let Some(registry) = get_context_registry() {
            if let Some(manager) = registry.get_a2a_manager() {
                manager.clear_context();
            }
        }
    }
}

/// RAII guard for request context
///
/// Automatically clears request context when dropped, providing exception-safe cleanup.
pub struct RequestContextGuard {
    _phantom: std::marker::PhantomData<()>,
}

impl Drop for RequestContextGuard {
    fn drop(&mut self) {
        // Use dependency injection to clear context
        if let Some(registry) = get_context_registry() {
            if let Some(manager) = registry.get_request_manager() {
                manager.clear_context();
            }
        }
    }
}

/// Combined RAII guard for all contexts
///
/// When dropped, clears all contexts in the correct order.
/// This is the safest option for complex operations.
pub struct AllContextsGuard {
    _phantom: std::marker::PhantomData<()>,
}

impl Drop for AllContextsGuard {
    fn drop(&mut self) {
        // Use dependency injection to clear all contexts
        if let Some(registry) = get_context_registry() {
            // Clear in reverse order (LIFO) for proper cleanup
            if let Some(manager) = registry.get_request_manager() {
                manager.clear_context();
            }
            if let Some(manager) = registry.get_a2a_manager() {
                manager.clear_context();
            }
            if let Some(manager) = registry.get_llm_manager() {
                manager.clear_context();
            }
        }
    }
}

// ==================== SCOPED CONTEXT SETTING APIS ====================

/// Set LLM context with RAII guard
///
/// Returns a guard that will automatically clear the context when dropped.
/// This is the safest way to set LLM context as cleanup is guaranteed.
///
/// # Example
/// ```rust
/// let _guard = set_llm_context_scoped("gpt-4", "openai_client");
/// // Context is active for the lifetime of _guard
/// log::info!("Processing LLM request");
/// // Context automatically cleared when _guard is dropped
/// ```
pub fn set_llm_context_scoped(model: &str, component: &str) -> LlmContextGuard {
    if let Some(registry) = get_context_registry() {
        if let Some(manager) = registry.get_llm_manager() {
            manager.set_context(model, component);
        }
    }
    LlmContextGuard {
        _phantom: std::marker::PhantomData,
    }
}

/// Set A2A context with RAII guard
///
/// Returns a guard that will automatically clear the context when dropped.
pub fn set_a2a_context_scoped(
    message_type: &str,
    from_agent: &str,
    to_agent: &str,
    component: &str,
) -> A2aContextGuard {
    if let Some(registry) = get_context_registry() {
        if let Some(manager) = registry.get_a2a_manager() {
            manager.set_context(message_type, from_agent, to_agent, component);
        }
    }
    A2aContextGuard {
        _phantom: std::marker::PhantomData,
    }
}

/// Set request context with RAII guard
///
/// Returns a guard that will automatically clear the context when dropped.
pub fn set_request_context_scoped(
    request_id: &str,
    user_id: Option<&str>,
    session_id: Option<&str>,
) -> RequestContextGuard {
    if let Some(registry) = get_context_registry() {
        if let Some(manager) = registry.get_request_manager() {
            manager.set_context(request_id, user_id, session_id);
        }
    }
    RequestContextGuard {
        _phantom: std::marker::PhantomData,
    }
}

/// Set all contexts with combined RAII guard
///
/// Returns a guard that will automatically clear all contexts when dropped.
/// This is the most comprehensive option for complex operations.
pub fn set_all_contexts_scoped(
    // LLM context
    model: &str,
    component: &str,
    // A2A context
    message_type: &str,
    from_agent: &str,
    to_agent: &str,
    a2a_component: &str,
    // Request context
    request_id: &str,
    user_id: Option<&str>,
    session_id: Option<&str>,
) -> AllContextsGuard {
    if let Some(registry) = get_context_registry() {
        if let Some(manager) = registry.get_llm_manager() {
            manager.set_context(model, component);
        }
        if let Some(manager) = registry.get_a2a_manager() {
            manager.set_context(message_type, from_agent, to_agent, a2a_component);
        }
        if let Some(manager) = registry.get_request_manager() {
            manager.set_context(request_id, user_id, session_id);
        }
    }

    AllContextsGuard {
        _phantom: std::marker::PhantomData,
    }
}

// ==================== SCOPED CALLBACK APIS ====================

/// Execute a closure with LLM context, automatically clearing when done
///
/// This is the most secure pattern as it guarantees context cleanup
/// even if the closure panics or returns early.
///
/// # Example
/// ```rust
/// let result = with_llm_context("gpt-4", "openai_client", || {
///     log::info!("This has LLM context");
///     process_llm_request()
/// });
/// // Context is automatically cleared here
/// ```
pub fn with_llm_context<F, R>(model: &str, component: &str, f: F) -> R
where
    F: FnOnce() -> R,
{
    let _guard = set_llm_context_scoped(model, component);
    f()
}

/// Execute a closure with A2A context, automatically clearing when done
pub fn with_a2a_context<F, R>(
    message_type: &str,
    from_agent: &str,
    to_agent: &str,
    component: &str,
    f: F,
) -> R
where
    F: FnOnce() -> R,
{
    let _guard = set_a2a_context_scoped(message_type, from_agent, to_agent, component);
    f()
}

/// Execute a closure with request context, automatically clearing when done  
pub fn with_request_context<F, R>(
    request_id: &str,
    user_id: Option<&str>,
    session_id: Option<&str>,
    f: F,
) -> R
where
    F: FnOnce() -> R,
{
    let _guard = set_request_context_scoped(request_id, user_id, session_id);
    f()
}

/// Execute a closure with all contexts, automatically clearing when done
///
/// This is the most comprehensive scoped API that sets all contexts
/// and guarantees cleanup regardless of how the closure exits.
pub fn with_all_contexts<F, R>(
    // LLM context
    model: &str,
    component: &str,
    // A2A context
    message_type: &str,
    from_agent: &str,
    to_agent: &str,
    a2a_component: &str,
    // Request context
    request_id: &str,
    user_id: Option<&str>,
    session_id: Option<&str>,
    // Closure to execute
    f: F,
) -> R
where
    F: FnOnce() -> R,
{
    let _guard = set_all_contexts_scoped(
        model,
        component,
        message_type,
        from_agent,
        to_agent,
        a2a_component,
        request_id,
        user_id,
        session_id,
    );
    f()
}

// ==================== NESTED CONTEXT SUPPORT ====================

/// Scoped context builder for complex nested operations
///
/// Allows building up contexts step by step with automatic cleanup.
/// Each context level gets its own guard for fine-grained control.
pub struct ScopedContextBuilder {
    guards: Vec<Box<dyn std::any::Any + Send>>,
}

impl ScopedContextBuilder {
    pub fn new() -> Self {
        Self { guards: Vec::new() }
    }

    pub fn with_llm_context(mut self, model: &str, component: &str) -> Self {
        let guard = set_llm_context_scoped(model, component);
        self.guards.push(Box::new(guard));
        self
    }

    pub fn with_a2a_context(
        mut self,
        message_type: &str,
        from_agent: &str,
        to_agent: &str,
        component: &str,
    ) -> Self {
        let guard = set_a2a_context_scoped(message_type, from_agent, to_agent, component);
        self.guards.push(Box::new(guard));
        self
    }

    pub fn with_request_context(
        mut self,
        request_id: &str,
        user_id: Option<&str>,
        session_id: Option<&str>,
    ) -> Self {
        let guard = set_request_context_scoped(request_id, user_id, session_id);
        self.guards.push(Box::new(guard));
        self
    }

    /// Execute a closure with all built contexts active
    pub fn execute<F, R>(&self, f: F) -> R
    where
        F: FnOnce() -> R,
    {
        // All guards are held by self, so contexts remain active
        // until ScopedContextBuilder is dropped
        f()
    }
}

impl Drop for ScopedContextBuilder {
    fn drop(&mut self) {
        // Guards will be dropped in reverse order (LIFO)
        // which provides correct cleanup semantics
        self.guards.clear();
    }
}

impl Default for ScopedContextBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// ==================== CONVENIENCE MACROS ====================

/// Macro for easy scoped LLM context
///
/// # Example
/// ```rust
/// with_llm_context_scoped!("gpt-4", "openai_client" => {
///     log::info!("Processing with LLM context");
/// });
/// ```
#[macro_export]
macro_rules! with_llm_context_scoped {
    ($model:expr, $component:expr => $block:block) => {
        $crate::context_adapter::with_llm_context($model, $component, || $block)
    };
}

/// Macro for easy scoped A2A context
#[macro_export]
macro_rules! with_a2a_context_scoped {
    ($message_type:expr, $from_agent:expr, $to_agent:expr, $component:expr => $block:block) => {
        $crate::context_adapter::with_a2a_context(
            $message_type,
            $from_agent,
            $to_agent,
            $component,
            || $block,
        )
    };
}

/// Macro for easy scoped request context
#[macro_export]
macro_rules! with_request_context_scoped {
    ($request_id:expr, $user_id:expr, $session_id:expr => $block:block) => {
        $crate::context_adapter::with_request_context($request_id, $user_id, $session_id, || $block)
    };
}

/// Initialize structured_logging context management integration
///
/// Call this once during application startup to enable advanced RAII context
/// management features in structured_logging.
///
/// # Example
/// ```rust
/// use structured_logging::context_adapter::init_context_integration;
///
/// // During application startup
/// init_context_integration();
///
/// // Now RAII contexts work automatically
/// let _guard = set_llm_context_scoped("gpt-4", "my_component");
/// log::info!("This log will have LLM context automatically");
/// // Context cleared when _guard drops
/// ```
pub fn init_context_integration() {
    let registry = StructuredContextRegistry::new();
    registry.register();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_context_integration() {
        init_context_integration();

        // Test that RAII guards work with structured_logging backend
        {
            let _guard = set_llm_context_scoped("gpt-4", "test_component");
            // Context should be set now
        }
        // Context should be cleared when guard is dropped
    }

    #[test]
    fn test_scoped_callback_integration() {
        init_context_integration();

        // Test that scoped callbacks work with structured_logging backend
        let result = with_llm_context("gpt-4", "test_component", || "test_result");

        assert_eq!(result, "test_result");
        // Context should be cleared automatically
    }

    #[test]
    fn test_scoped_builder() {
        init_context_integration();

        let builder = ScopedContextBuilder::new()
            .with_llm_context("gpt-4", "test_component")
            .with_request_context("req-123", Some("user-456"), None);

        let result = builder.execute(|| "builder_test");

        assert_eq!(result, "builder_test");
    }
}
