//! Resource management patterns
//!
//! This module provides patterns for safe resource acquisition, management, and release.
//! Includes resource pools, connection management, and other resource patterns common
//! in distributed systems and agent architectures.
//!
//! ## Key Features
//!
//! - **Resource Pools**: Thread-safe resource pooling with automatic cleanup
//! - **Resource Guards**: RAII-based resource acquisition and release
//! - **Lease Management**: Time-based and usage-based resource leasing
//! - **Health Checking**: Automatic resource health validation
//! - **Graceful Degradation**: Fallback patterns when resources are unavailable

use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

/// Resource guard that manages the lifetime of a resource
///
/// This guard ensures that resources are properly returned to their pool
/// or cleaned up when they're no longer needed.
pub struct ResourceGuard<T> {
    resource: Option<T>,
    return_fn: Option<Box<dyn FnOnce(T) + Send>>,
}

impl<T> ResourceGuard<T> {
    /// Create a new resource guard
    ///
    /// # Arguments
    /// - `resource`: The resource to manage
    /// - `return_fn`: Function to call when returning the resource
    pub fn new<F>(resource: T, return_fn: F) -> Self
    where
        F: FnOnce(T) + Send + 'static,
    {
        Self {
            resource: Some(resource),
            return_fn: Some(Box::new(return_fn)),
        }
    }

    /// Get a reference to the managed resource
    pub fn resource(&self) -> &T {
        self.resource.as_ref().expect("Resource already returned")
    }

    /// Get a mutable reference to the managed resource
    pub fn resource_mut(&mut self) -> &mut T {
        self.resource.as_mut().expect("Resource already returned")
    }

    /// Manually return the resource (consumes the guard)
    pub fn return_resource(mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        match (self.resource.take(), self.return_fn.take()) {
            (Some(resource), Some(return_fn)) => {
                return_fn(resource);
                Ok(())
            }
            _ => Err("Resource already returned".into()),
        }
    }

    /// Take ownership of the resource without returning it to the pool
    pub fn take_ownership(mut self) -> T {
        self.return_fn.take(); // Prevent automatic return
        self.resource.take().expect("Resource already taken")
    }
}

impl<T> Drop for ResourceGuard<T> {
    fn drop(&mut self) {
        if let (Some(resource), Some(return_fn)) = (self.resource.take(), self.return_fn.take()) {
            return_fn(resource);
        }
    }
}

/// Configuration for resource pools
#[derive(Debug, Clone)]
pub struct PoolConfig {
    /// Maximum number of resources in the pool
    pub max_size: usize,

    /// Minimum number of resources to keep in the pool
    pub min_size: usize,

    /// Maximum time to wait for a resource to become available
    pub acquire_timeout: Duration,

    /// Maximum idle time before a resource is considered expired
    pub max_idle_time: Duration,

    /// Interval for health checking idle resources
    pub health_check_interval: Duration,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            max_size: 10,
            min_size: 1,
            acquire_timeout: Duration::from_secs(30),
            max_idle_time: Duration::from_mins(10), // 10 minutes
            health_check_interval: Duration::from_mins(1), // 1 minute
        }
    }
}

/// A resource pool that manages the lifecycle of expensive resources
///
/// This pool provides thread-safe access to resources with automatic
/// cleanup, health checking, and graceful degradation.
pub struct ResourcePool<T> {
    inner: Arc<Mutex<PoolInner<T>>>,
    not_empty: Arc<Condvar>,
    config: PoolConfig,
    factory: Arc<dyn Fn() -> Result<T, Box<dyn std::error::Error + Send + Sync>> + Send + Sync>,
    health_checker: Option<Arc<dyn Fn(&T) -> bool + Send + Sync>>,
}

struct PoolInner<T> {
    resources: VecDeque<PooledResource<T>>,
    total_count: usize,
    active_count: usize,
}

struct PooledResource<T> {
    resource: T,
    created_at: Instant,
    last_used: Instant,
}

impl<T> ResourcePool<T>
where
    T: Send + 'static,
{
    /// Create a new resource pool
    ///
    /// # Arguments
    /// - `config`: Pool configuration
    /// - `factory`: Function to create new resources
    ///
    /// # Example
    /// ```rust
    /// use foundation_utils::resource::{ResourcePool, PoolConfig};
    ///
    /// let pool = ResourcePool::new(
    ///     PoolConfig::default(),
    ///     || Ok("new_resource".to_string())
    /// ).unwrap();
    /// ```
    pub fn new<F>(
        config: PoolConfig,
        factory: F,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>>
    where
        F: Fn() -> Result<T, Box<dyn std::error::Error + Send + Sync>> + Send + Sync + 'static,
    {
        let pool = Self {
            inner: Arc::new(Mutex::new(PoolInner {
                resources: VecDeque::new(),
                total_count: 0,
                active_count: 0,
            })),
            not_empty: Arc::new(Condvar::new()),
            config: config.clone(),
            factory: Arc::new(factory),
            health_checker: None,
        };

        // Pre-populate with minimum number of resources
        pool.ensure_min_resources()?;

        Ok(pool)
    }

    /// Set a health checker function for resources
    ///
    /// The health checker is called periodically to validate that
    /// pooled resources are still healthy.
    pub fn with_health_checker<F>(mut self, health_checker: F) -> Self
    where
        F: Fn(&T) -> bool + Send + Sync + 'static,
    {
        self.health_checker = Some(Arc::new(health_checker));
        self
    }

    /// Acquire a resource from the pool
    ///
    /// This method will either return an existing resource from the pool
    /// or create a new one if the pool is not at capacity.
    ///
    /// # Returns
    /// A `ResourceGuard` that automatically returns the resource to the pool
    /// when dropped.
    pub fn acquire(&self) -> Result<ResourceGuard<T>, Box<dyn std::error::Error + Send + Sync>> {
        let start_time = Instant::now();

        loop {
            // Try to get a resource from the pool
            if let Some(resource) = self.try_acquire_existing()? {
                return Ok(resource);
            }

            // Try to create a new resource if under capacity
            if let Some(resource) = self.try_create_new()? {
                return Ok(resource);
            }

            // Wait for a resource to become available
            if start_time.elapsed() >= self.config.acquire_timeout {
                return Err("Timeout waiting for resource".into());
            }

            // Wait for notification that a resource is available
            let inner = self.inner.lock().unwrap();
            let _guard = self
                .not_empty
                .wait_timeout(inner, Duration::from_millis(100))
                .unwrap();
        }
    }

    /// Try to acquire an existing resource from the pool
    fn try_acquire_existing(
        &self,
    ) -> Result<Option<ResourceGuard<T>>, Box<dyn std::error::Error + Send + Sync>> {
        let mut inner = self.inner.lock().unwrap();

        while let Some(mut pooled) = inner.resources.pop_front() {
            // Check if resource is still healthy
            if let Some(ref health_checker) = self.health_checker {
                if !health_checker(&pooled.resource) {
                    inner.total_count -= 1;
                    continue; // Try next resource
                }
            }

            // Check if resource is expired
            if pooled.last_used.elapsed() > self.config.max_idle_time {
                inner.total_count -= 1;
                continue; // Try next resource
            }

            // Resource is good, update usage time and return it
            pooled.last_used = Instant::now();
            inner.active_count += 1;

            let pool_ref = Arc::clone(&self.inner);
            let not_empty_ref = Arc::clone(&self.not_empty);

            let guard = ResourceGuard::new(pooled.resource, move |resource| {
                let mut inner = pool_ref.lock().unwrap();
                inner.resources.push_back(PooledResource {
                    resource,
                    created_at: pooled.created_at,
                    last_used: Instant::now(),
                });
                inner.active_count -= 1;
                not_empty_ref.notify_one();
            });

            return Ok(Some(guard));
        }

        Ok(None)
    }

    /// Try to create a new resource if under capacity
    fn try_create_new(
        &self,
    ) -> Result<Option<ResourceGuard<T>>, Box<dyn std::error::Error + Send + Sync>> {
        let inner = self.inner.lock().unwrap();

        if inner.total_count >= self.config.max_size {
            return Ok(None);
        }

        // Create new resource outside the lock
        drop(inner);
        let resource = (self.factory)()?;

        // Re-acquire lock and update counters
        let mut inner = self.inner.lock().unwrap();
        inner.total_count += 1;
        inner.active_count += 1;

        let pool_ref = Arc::clone(&self.inner);
        let not_empty_ref = Arc::clone(&self.not_empty);
        let created_at = Instant::now();

        let guard = ResourceGuard::new(resource, move |resource| {
            let mut inner = pool_ref.lock().unwrap();
            inner.resources.push_back(PooledResource {
                resource,
                created_at,
                last_used: Instant::now(),
            });
            inner.active_count -= 1;
            not_empty_ref.notify_one();
        });

        Ok(Some(guard))
    }

    /// Ensure the pool has at least the minimum number of resources
    fn ensure_min_resources(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        loop {
            // Check if we need more resources
            let need_more = {
                let inner = self.inner.lock().unwrap();
                if inner.total_count >= self.config.max_size {
                    false
                } else {
                    inner.resources.len() + inner.active_count < self.config.min_size
                }
            };

            if !need_more {
                break;
            }

            // Create resource outside the lock
            let resource = (self.factory)()?;

            // Add resource to pool
            {
                let mut inner = self.inner.lock().unwrap();
                inner.resources.push_back(PooledResource {
                    resource,
                    created_at: Instant::now(),
                    last_used: Instant::now(),
                });
                inner.total_count += 1;
            }
        }

        Ok(())
    }

    /// Get pool statistics
    pub fn stats(&self) -> PoolStats {
        let inner = self.inner.lock().unwrap();
        PoolStats {
            total_resources: inner.total_count,
            available_resources: inner.resources.len(),
            active_resources: inner.active_count,
            max_size: self.config.max_size,
            min_size: self.config.min_size,
        }
    }

    /// Perform health check on all idle resources
    pub fn health_check(&self) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
        let Some(ref health_checker) = self.health_checker else {
            return Ok(0);
        };

        let mut inner = self.inner.lock().unwrap();
        let mut removed_count = 0;
        let mut healthy_resources = VecDeque::new();

        while let Some(pooled) = inner.resources.pop_front() {
            if health_checker(&pooled.resource)
                && pooled.last_used.elapsed() <= self.config.max_idle_time
            {
                healthy_resources.push_back(pooled);
            } else {
                removed_count += 1;
                inner.total_count -= 1;
            }
        }

        inner.resources = healthy_resources;

        Ok(removed_count)
    }
}

/// Statistics about a resource pool
#[derive(Debug, Clone)]
pub struct PoolStats {
    /// Total number of resources (active + available)
    pub total_resources: usize,

    /// Number of available resources in the pool
    pub available_resources: usize,

    /// Number of actively used resources
    pub active_resources: usize,

    /// Maximum pool size
    pub max_size: usize,

    /// Minimum pool size
    pub min_size: usize,
}

impl PoolStats {
    /// Get the pool utilization as a percentage (0.0 to 1.0)
    pub fn utilization(&self) -> f64 {
        if self.max_size == 0 {
            0.0
        } else {
            self.active_resources as f64 / self.max_size as f64
        }
    }

    /// Check if the pool is at capacity
    pub fn is_at_capacity(&self) -> bool {
        self.total_resources >= self.max_size
    }

    /// Check if the pool is below minimum size
    pub fn is_below_minimum(&self) -> bool {
        self.total_resources < self.min_size
    }
}

/// Convenience function to create a simple resource guard
///
/// This function creates a guard that will call the cleanup function
/// when the resource is no longer needed.
pub fn managed_resource<T, F>(resource: T, cleanup: F) -> ResourceGuard<T>
where
    F: FnOnce(T) + Send + 'static,
{
    ResourceGuard::new(resource, cleanup)
}

/// Trait for types that can be managed as resources
pub trait ManagedResource: Sized + Send {
    type Error: std::error::Error + Send + Sync + 'static;

    /// Create a new instance of this resource
    fn create() -> Result<Self, Self::Error>;

    /// Check if this resource is healthy
    fn is_healthy(&self) -> bool {
        true
    }

    /// Clean up this resource (called when resource is dropped)
    fn cleanup(self) -> Result<(), Self::Error> {
        Ok(())
    }

    /// Create a managed resource guard
    fn managed(self) -> ResourceGuard<Self> {
        managed_resource(self, |resource| {
            let _ = resource.cleanup();
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Debug)]
    struct TestResource {
        _id: usize,
        closed: Arc<AtomicUsize>,
    }

    impl TestResource {
        fn new(id: usize, closed: Arc<AtomicUsize>) -> Self {
            Self { _id: id, closed }
        }
    }

    impl Drop for TestResource {
        fn drop(&mut self) {
            self.closed.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[test]
    fn test_resource_guard() {
        let cleanup_called = Arc::new(AtomicUsize::new(0));
        let cleanup_called_clone = cleanup_called.clone();

        {
            let guard = ResourceGuard::new(42, move |value| {
                cleanup_called_clone.store(value, Ordering::Relaxed);
            });

            assert_eq!(*guard.resource(), 42);
        } // Guard drops here

        assert_eq!(cleanup_called.load(Ordering::Relaxed), 42);
    }

    #[test]
    fn test_resource_pool_basic() {
        let closed_count = Arc::new(AtomicUsize::new(0));
        let closed_count_clone = closed_count.clone();

        let pool = ResourcePool::new(
            PoolConfig {
                max_size: 3,
                min_size: 1,
                ..Default::default()
            },
            move || {
                let id = 1;
                Ok(TestResource::new(id, closed_count_clone.clone()))
            },
        )
        .unwrap();

        // Test acquiring and releasing resources
        {
            let _resource1 = pool.acquire().unwrap();
            let _resource2 = pool.acquire().unwrap();

            let stats = pool.stats();
            assert_eq!(stats.active_resources, 2);
            assert!(stats.available_resources <= 1);
        } // Resources should be returned to pool

        let stats = pool.stats();
        assert_eq!(stats.active_resources, 0);
        assert!(stats.available_resources > 0);
    }

    #[test]
    fn test_resource_pool_capacity() {
        let pool = ResourcePool::new(
            PoolConfig {
                max_size: 2,
                min_size: 0,
                acquire_timeout: Duration::from_millis(100),
                ..Default::default()
            },
            || Ok("resource".to_string()),
        )
        .unwrap();

        // Acquire all resources
        let _r1 = pool.acquire().unwrap();
        let _r2 = pool.acquire().unwrap();

        // This should timeout since pool is at capacity
        let result = pool.acquire();
        assert!(result.is_err());
    }

    #[test]
    fn test_resource_pool_health_check() {
        let pool = ResourcePool::new(
            PoolConfig {
                max_size: 5,
                min_size: 2, // Pre-populate with 2 resources
                ..Default::default()
            },
            || Ok("resource".to_string()),
        )
        .unwrap()
        .with_health_checker(|_| false); // Mark all resources as unhealthy

        // Check that we have some available resources
        let stats_before = pool.stats();
        println!("Stats before health check: {:?}", stats_before);

        // Perform health check - should remove unhealthy resources
        let removed = pool.health_check().unwrap();

        let stats_after = pool.stats();
        println!("Stats after health check: {:?}", stats_after);

        // We should have removed some resources since they were all marked unhealthy
        // and we had min_size=2 resources initially
        assert_eq!(removed, 2); // Should have removed the 2 initial resources
    }

    #[test]
    fn test_managed_resource() {
        let cleanup_called = Arc::new(AtomicUsize::new(0));
        let cleanup_called_clone = cleanup_called.clone();

        {
            let _guard = managed_resource(42, move |value| {
                cleanup_called_clone.store(value, Ordering::Relaxed);
            });
        }

        assert_eq!(cleanup_called.load(Ordering::Relaxed), 42);
    }

    #[test]
    fn test_pool_stats() {
        let pool = ResourcePool::new(
            PoolConfig {
                max_size: 10,
                min_size: 2,
                ..Default::default()
            },
            || Ok("resource".to_string()),
        )
        .unwrap();

        let stats = pool.stats();
        assert_eq!(stats.max_size, 10);
        assert_eq!(stats.min_size, 2);
        assert!(stats.utilization() <= 1.0);
        assert!(!stats.is_at_capacity());
    }
}
