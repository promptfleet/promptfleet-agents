//! Hierarchical federation strategies for production Prometheus deployments

use crate::PrometheusConfig;
use observability_core::{ObservabilityError, ObservabilityResult};
use std::collections::HashMap;

/// Configuration for hierarchical federation
#[derive(Debug, Clone)]
pub struct FederationConfig {
    /// Federation levels (e.g., cluster -> region -> global)
    pub levels: Vec<FederationLevel>,
    /// Global federation endpoint
    pub global_endpoint: Option<String>,
    /// Regional federation endpoints
    pub regional_endpoints: HashMap<String, String>,
    /// Match expressions for federation
    pub match_expressions: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct FederationLevel {
    /// Level name (e.g., "cluster", "region", "global")
    pub name: String,
    /// Endpoint for this federation level
    pub endpoint: String,
    /// Honor labels from this level
    pub honor_labels: bool,
    /// Scrape interval for federation
    pub scrape_interval: web_time::Duration,
}

impl Default for FederationConfig {
    fn default() -> Self {
        Self {
            levels: vec![
                FederationLevel {
                    name: "cluster".to_string(),
                    endpoint: "http://prometheus-cluster:9090".to_string(),
                    honor_labels: true,
                    scrape_interval: web_time::Duration::from_secs(15),
                },
                FederationLevel {
                    name: "region".to_string(),
                    endpoint: "http://prometheus-region:9090".to_string(),
                    honor_labels: true,
                    scrape_interval: web_time::Duration::from_secs(30),
                },
            ],
            global_endpoint: Some("http://prometheus-global:9090".to_string()),
            regional_endpoints: HashMap::new(),
            match_expressions: vec![
                r#"{__name__=~"istio:.*"}"#.to_string(),
                r#"{__name__=~"spinkube:.*"}"#.to_string(),
                r#"{__name__=~"llm_.*"}"#.to_string(),
            ],
        }
    }
}

/// Hierarchical federation manager
pub struct HierarchicalFederation {
    config: FederationConfig,
}

impl HierarchicalFederation {
    /// Create a new hierarchical federation manager
    pub fn new(config: FederationConfig) -> Self {
        Self { config }
    }

    /// Generate federation configuration for Prometheus
    pub fn generate_prometheus_config(&self) -> ObservabilityResult<String> {
        let mut config = String::new();

        config.push_str("global:\n");
        config.push_str("  scrape_interval: 15s\n");
        config.push_str("  evaluation_interval: 15s\n\n");

        config.push_str("rule_files:\n");
        config.push_str("  - \"istio_recording_rules.yml\"\n");
        config.push_str("  - \"spinkube_recording_rules.yml\"\n\n");

        config.push_str("scrape_configs:\n");

        // Add federation jobs for each level
        for level in &self.config.levels {
            config.push_str(&format!("  - job_name: '{}-federation'\n", level.name));
            config.push_str(&format!(
                "    scrape_interval: {}s\n",
                level.scrape_interval.as_secs()
            ));
            config.push_str(&format!("    honor_labels: {}\n", level.honor_labels));
            config.push_str("    metrics_path: '/federate'\n");
            config.push_str("    params:\n");
            config.push_str("      'match[]':\n");

            for expr in &self.config.match_expressions {
                config.push_str(&format!("        - '{}'\n", expr));
            }

            config.push_str("    static_configs:\n");
            config.push_str(&format!(
                "      - targets:\n        - '{}'\n\n",
                level.endpoint
            ));
        }

        Ok(config)
    }

    /// Get federation targets for a specific level
    pub fn get_federation_targets(&self, level: &str) -> Vec<String> {
        self.config
            .levels
            .iter()
            .filter(|l| l.name == level)
            .map(|l| l.endpoint.clone())
            .collect()
    }

    /// Validate federation configuration
    pub fn validate(&self) -> ObservabilityResult<()> {
        if self.config.levels.is_empty() {
            return Err(ObservabilityError::configuration(
                "No federation levels configured",
            ));
        }

        for level in &self.config.levels {
            if level.name.is_empty() {
                return Err(ObservabilityError::configuration(
                    "Federation level name cannot be empty",
                ));
            }
            if level.endpoint.is_empty() {
                return Err(ObservabilityError::configuration(
                    "Federation level endpoint cannot be empty",
                ));
            }
        }

        Ok(())
    }
}

/// Convert PrometheusConfig to federation config
impl From<&PrometheusConfig> for FederationConfig {
    fn from(config: &PrometheusConfig) -> Self {
        let mut federation_config = FederationConfig::default();

        // Add job-specific configuration
        federation_config.levels[0].name = config.job_name.clone();

        federation_config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_federation_config_default() {
        let config = FederationConfig::default();
        assert_eq!(config.levels.len(), 2);
        assert_eq!(config.levels[0].name, "cluster");
        assert_eq!(config.levels[1].name, "region");
        assert!(!config.match_expressions.is_empty());
    }

    #[test]
    fn test_hierarchical_federation_creation() {
        let config = FederationConfig::default();
        let federation = HierarchicalFederation::new(config);
        assert!(federation.validate().is_ok());
    }

    #[test]
    fn test_prometheus_config_generation() {
        let config = FederationConfig::default();
        let federation = HierarchicalFederation::new(config);
        let prometheus_config = federation.generate_prometheus_config();
        assert!(prometheus_config.is_ok());

        let config_str = prometheus_config.unwrap();
        assert!(config_str.contains("cluster-federation"));
        assert!(config_str.contains("region-federation"));
        assert!(config_str.contains("istio:.*"));
    }

    #[test]
    fn test_federation_targets() {
        let config = FederationConfig::default();
        let federation = HierarchicalFederation::new(config);

        let cluster_targets = federation.get_federation_targets("cluster");
        assert_eq!(cluster_targets.len(), 1);
        assert_eq!(cluster_targets[0], "http://prometheus-cluster:9090");
    }
}
