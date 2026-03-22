//! Service Container for Dependency Injection
//!
//! Provides a minimal, type-safe service container for injecting
//! optional services into agents without hardcoding dependencies.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::Arc;

/// Minimal service container for dependency injection
///
/// Allows agents to accept optional services through clean dependency injection
/// without hardcoding specific service fields in the Agent struct.
///
/// # Examples
///
/// ```rust
/// use agent_sdk::services::ServiceContainer;
/// use std::sync::Arc;
///
/// #[derive(Clone)]
/// struct MyService {
///     name: String,
/// }
///
/// let mut container = ServiceContainer::new();
/// container.register(MyService { name: "test".to_string() });
///
/// let service: Option<Arc<MyService>> = container.get();
/// assert!(service.is_some());
/// ```
#[derive(Clone)]
pub struct ServiceContainer {
    services: HashMap<TypeId, Arc<dyn Any + Send + Sync>>,
}

impl ServiceContainer {
    /// Create a new empty service container
    pub fn new() -> Self {
        Self {
            services: HashMap::new(),
        }
    }

    /// Register a service in the container
    ///
    /// Services are stored by their type and can be retrieved later
    /// using the same type signature.
    ///
    /// # Arguments
    ///
    /// * `service` - The service instance to register
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use agent_sdk::services::ServiceContainer;
    /// # #[derive(Clone)]
    /// # struct DatabaseService;
    /// let mut container = ServiceContainer::new();
    /// container.register(DatabaseService);
    /// ```
    pub fn register<T: Send + Sync + 'static>(&mut self, service: T) {
        let type_id = TypeId::of::<T>();
        self.services.insert(type_id, Arc::new(service));
    }

    /// Get a service from the container
    ///
    /// Returns an Arc-wrapped service if found, or None if the service
    /// type is not registered.
    ///
    /// # Returns
    ///
    /// * `Some(Arc<T>)` - The service if found
    /// * `None` - If no service of type T is registered
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use agent_sdk::services::ServiceContainer;
    /// # use std::sync::Arc;
    /// # #[derive(Clone)]
    /// # struct DatabaseService { pub name: String }
    /// # let mut container = ServiceContainer::new();
    /// # container.register(DatabaseService { name: "test".to_string() });
    /// let service: Option<Arc<DatabaseService>> = container.get();
    /// if let Some(db) = service {
    ///     println!("Database: {}", db.name);
    /// }
    /// ```
    pub fn get<T: Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        let type_id = TypeId::of::<T>();
        self.services.get(&type_id)?.clone().downcast::<T>().ok()
    }

    /// Check if a service type is registered
    ///
    /// # Returns
    ///
    /// * `true` - If a service of type T is registered
    /// * `false` - If no service of type T is found
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use agent_sdk::services::ServiceContainer;
    /// # #[derive(Clone)]
    /// # struct CacheService;
    /// let container = ServiceContainer::new();
    /// assert!(!container.has::<CacheService>());
    /// ```
    pub fn has<T: 'static>(&self) -> bool {
        self.services.contains_key(&TypeId::of::<T>())
    }

    /// Get the number of registered services
    pub fn len(&self) -> usize {
        self.services.len()
    }

    /// Check if the container is empty
    pub fn is_empty(&self) -> bool {
        self.services.is_empty()
    }

    /// Remove all services from the container
    pub fn clear(&mut self) {
        self.services.clear();
    }
}

impl Default for ServiceContainer {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for ServiceContainer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServiceContainer")
            .field("service_count", &self.services.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Debug, PartialEq)]
    struct TestService {
        name: String,
        value: i32,
    }

    #[derive(Clone)]
    struct AnotherService {
        data: String,
    }

    #[test]
    fn test_service_registration_and_retrieval() {
        let mut container = ServiceContainer::new();

        let service = TestService {
            name: "test".to_string(),
            value: 42,
        };

        container.register(service.clone());

        let retrieved: Option<Arc<TestService>> = container.get();
        assert!(retrieved.is_some());
        let service = retrieved.unwrap();
        assert_eq!(service.name, "test");
        assert_eq!(service.value, 42);
    }

    #[test]
    fn test_service_not_found() {
        let container = ServiceContainer::new();

        let service: Option<Arc<TestService>> = container.get();
        assert!(service.is_none());
    }

    #[test]
    fn test_has_service() {
        let mut container = ServiceContainer::new();

        assert!(!container.has::<TestService>());

        container.register(TestService {
            name: "test".to_string(),
            value: 1,
        });

        assert!(container.has::<TestService>());
        assert!(!container.has::<AnotherService>());
    }

    #[test]
    fn test_multiple_services() {
        let mut container = ServiceContainer::new();

        container.register(TestService {
            name: "test".to_string(),
            value: 1,
        });

        container.register(AnotherService {
            data: "another".to_string(),
        });

        assert!(container.has::<TestService>());
        assert!(container.has::<AnotherService>());
        assert_eq!(container.len(), 2);

        let test_service: Option<Arc<TestService>> = container.get();
        let another_service: Option<Arc<AnotherService>> = container.get();

        assert!(test_service.is_some());
        assert!(another_service.is_some());
    }

    #[test]
    fn test_container_operations() {
        let mut container = ServiceContainer::new();

        assert!(container.is_empty());
        assert_eq!(container.len(), 0);

        container.register(TestService {
            name: "test".to_string(),
            value: 1,
        });

        assert!(!container.is_empty());
        assert_eq!(container.len(), 1);

        container.clear();

        assert!(container.is_empty());
        assert_eq!(container.len(), 0);
    }
}
