//! Push Gateway client for batch metrics export

use observability_core::{ObservabilityError, ObservabilityResult};
use prometheus::{Encoder, Registry, TextEncoder};
use std::sync::Arc;
use web_time::Duration;

/// Push Gateway client for batch metrics export
pub struct PushGatewayClient {
    endpoint: String,
    client: reqwest::Client,
}

impl PushGatewayClient {
    /// Create a new push gateway client
    pub fn new(endpoint: &str) -> ObservabilityResult<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| {
                ObservabilityError::transport(format!("Failed to create HTTP client: {}", e))
            })?;

        Ok(Self {
            endpoint: endpoint.to_string(),
            client,
        })
    }

    /// Push metrics to the gateway
    pub async fn push_metrics(
        &self,
        registry: &Registry,
        job: &str,
        instance: &str,
    ) -> ObservabilityResult<()> {
        let metrics_text = self.gather_metrics(registry)?;

        let url = format!(
            "{}/metrics/job/{}/instance/{}",
            self.endpoint, job, instance
        );

        let response = self
            .client
            .put(&url)
            .header("Content-Type", "text/plain")
            .body(metrics_text)
            .send()
            .await
            .map_err(|e| ObservabilityError::transport(format!("Failed to push metrics: {}", e)))?;

        if !response.status().is_success() {
            return Err(ObservabilityError::transport(format!(
                "Push gateway returned error: {}",
                response.status()
            )));
        }

        Ok(())
    }

    /// Delete metrics from the gateway
    pub async fn delete_metrics(&self, job: &str, instance: &str) -> ObservabilityResult<()> {
        let url = format!(
            "{}/metrics/job/{}/instance/{}",
            self.endpoint, job, instance
        );

        let response = self.client.delete(&url).send().await.map_err(|e| {
            ObservabilityError::transport(format!("Failed to delete metrics: {}", e))
        })?;

        if !response.status().is_success() && response.status().as_u16() != 404 {
            return Err(ObservabilityError::transport(format!(
                "Push gateway delete returned error: {}",
                response.status()
            )));
        }

        Ok(())
    }

    /// Push metrics to the gateway with custom labels
    pub async fn push_metrics_with_labels(
        &self,
        registry: &Registry,
        job: &str,
        labels: &[(&str, &str)],
    ) -> ObservabilityResult<()> {
        let metrics_text = self.gather_metrics(registry)?;

        let mut url = format!("{}/metrics/job/{}", self.endpoint, job);
        for (key, value) in labels {
            url.push_str(&format!("/{}/{}", key, value));
        }

        let response = self
            .client
            .put(&url)
            .header("Content-Type", "text/plain")
            .body(metrics_text)
            .send()
            .await
            .map_err(|e| ObservabilityError::transport(format!("Failed to push metrics: {}", e)))?;

        if !response.status().is_success() {
            return Err(ObservabilityError::transport(format!(
                "Push gateway returned error: {}",
                response.status()
            )));
        }

        Ok(())
    }

    /// Gather metrics from the registry
    fn gather_metrics(&self, registry: &Registry) -> ObservabilityResult<String> {
        let metric_families = registry.gather();
        let encoder = TextEncoder::new();

        let mut buffer = Vec::new();
        encoder.encode(&metric_families, &mut buffer).map_err(|e| {
            ObservabilityError::serialization(format!("Failed to encode metrics: {}", e))
        })?;

        String::from_utf8(buffer).map_err(|e| {
            ObservabilityError::serialization(format!("Failed to convert metrics to string: {}", e))
        })
    }

    /// Check if the push gateway is healthy
    pub async fn health_check(&self) -> ObservabilityResult<bool> {
        let url = format!("{}/-/healthy", self.endpoint);

        match self.client.get(&url).send().await {
            Ok(response) => Ok(response.status().is_success()),
            Err(_) => Ok(false),
        }
    }

    /// Get push gateway metrics (for monitoring the gateway itself)
    pub async fn get_gateway_metrics(&self) -> ObservabilityResult<String> {
        let url = format!("{}/metrics", self.endpoint);

        let response = self.client.get(&url).send().await.map_err(|e| {
            ObservabilityError::transport(format!("Failed to get gateway metrics: {}", e))
        })?;

        if !response.status().is_success() {
            return Err(ObservabilityError::transport(format!(
                "Gateway metrics request failed: {}",
                response.status()
            )));
        }

        response.text().await.map_err(|e| {
            ObservabilityError::transport(format!("Failed to read gateway metrics: {}", e))
        })
    }
}

/// Batch push client for high-volume scenarios
pub struct BatchPushClient {
    inner: PushGatewayClient,
    batch_size: usize,
    pending_registries: Vec<Arc<Registry>>,
}

impl BatchPushClient {
    /// Create a new batch push client
    pub fn new(endpoint: &str, batch_size: usize) -> ObservabilityResult<Self> {
        Ok(Self {
            inner: PushGatewayClient::new(endpoint)?,
            batch_size,
            pending_registries: Vec::new(),
        })
    }

    /// Add a registry to the batch
    pub fn add_registry(&mut self, registry: Arc<Registry>) {
        self.pending_registries.push(registry);
    }

    /// Push all pending registries if batch is full or force is true
    pub async fn maybe_push(
        &mut self,
        job: &str,
        instance: &str,
        force: bool,
    ) -> ObservabilityResult<()> {
        if force || self.pending_registries.len() >= self.batch_size {
            // Push each registry individually to avoid metric name conflicts
            // In a production system, you'd want to merge compatible metrics
            for (index, registry) in self.pending_registries.iter().enumerate() {
                let instance_name = format!("{}_{}", instance, index);
                self.inner
                    .push_metrics(registry, job, &instance_name)
                    .await?;
            }

            self.pending_registries.clear();
        }

        Ok(())
    }

    /// Force push all pending registries
    pub async fn flush(&mut self, job: &str, instance: &str) -> ObservabilityResult<()> {
        self.maybe_push(job, instance, true).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use prometheus::Counter;

    #[test]
    fn test_push_client_creation() {
        let client = PushGatewayClient::new("http://localhost:9091");
        assert!(client.is_ok());
    }

    #[test]
    fn test_batch_client_creation() {
        let client = BatchPushClient::new("http://localhost:9091", 5);
        assert!(client.is_ok());
    }

    #[test]
    fn test_gather_metrics() {
        let registry = Registry::new();
        let counter = Counter::new("test_counter", "A test counter").unwrap();
        registry.register(Box::new(counter.clone())).unwrap();
        counter.inc();

        let client = PushGatewayClient::new("http://localhost:9091").unwrap();
        let metrics = client.gather_metrics(&registry);
        assert!(metrics.is_ok());

        let metrics_text = metrics.unwrap();
        assert!(metrics_text.contains("test_counter"));
    }

    #[tokio::test]
    async fn test_push_metrics() {
        let registry = Registry::new();
        let counter = Counter::new("test_push_counter", "A test push counter").unwrap();
        registry.register(Box::new(counter.clone())).unwrap();
        counter.inc();

        let client = PushGatewayClient::new("http://localhost:9091").unwrap();

        // This will fail in CI/CD without a real push gateway, but tests the interface
        let result = client
            .push_metrics(&registry, "test_job", "test_instance")
            .await;
        // Don't assert success as push gateway may not be available
        let _ = result;
    }

    #[tokio::test]
    async fn test_health_check() {
        let client = PushGatewayClient::new("http://localhost:9091").unwrap();

        // This will return false if no push gateway is running, but tests the interface
        let result = client.health_check().await;
        assert!(result.is_ok());
    }
}
