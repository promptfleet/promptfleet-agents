//! Performance recording rules for Prometheus to pre-compute expensive queries

use observability_core::{ObservabilityError, ObservabilityResult};
use std::collections::HashMap;

#[cfg(feature = "serde_yaml")]
use serde_yaml;

/// Recording rule definition
#[derive(Debug, Clone)]
pub struct RecordingRule {
    /// Name of the resulting metric
    pub record: String,
    /// PromQL expression to evaluate
    pub expr: String,
    /// Labels to add to the result
    pub labels: HashMap<String, String>,
}

/// Recording rule group
#[derive(Debug, Clone)]
pub struct RecordingRuleGroup {
    /// Group name
    pub name: String,
    /// Evaluation interval
    pub interval: web_time::Duration,
    /// Rules in this group
    pub rules: Vec<RecordingRule>,
}

/// Manager for recording rules
pub struct RecordingRulesManager {
    groups: Vec<RecordingRuleGroup>,
}

impl RecordingRulesManager {
    /// Create a new recording rules manager
    pub fn new() -> Self {
        Self { groups: Vec::new() }
    }

    /// Create with default SpinKube rules
    pub fn with_spinkube_defaults() -> Self {
        let mut manager = Self::new();
        manager.add_group(create_istio_rules());
        manager.add_group(create_spinkube_rules());
        manager.add_group(create_llm_rules());
        manager
    }

    /// Add a recording rule group
    pub fn add_group(&mut self, group: RecordingRuleGroup) {
        self.groups.push(group);
    }

    /// Generate YAML configuration for Prometheus
    #[cfg(feature = "serde_yaml")]
    pub fn generate_yaml(&self) -> ObservabilityResult<String> {
        let yaml_groups: Vec<serde_yaml::Value> = self
            .groups
            .iter()
            .map(|group| {
                let rules: Vec<serde_yaml::Value> = group
                    .rules
                    .iter()
                    .map(|rule| {
                        let mut rule_map = serde_yaml::Mapping::new();
                        rule_map.insert("record".into(), rule.record.clone().into());
                        rule_map.insert("expr".into(), rule.expr.clone().into());

                        if !rule.labels.is_empty() {
                            let labels: serde_yaml::Mapping = rule
                                .labels
                                .iter()
                                .map(|(k, v)| (k.clone().into(), v.clone().into()))
                                .collect();
                            rule_map.insert("labels".into(), labels.into());
                        }

                        rule_map.into()
                    })
                    .collect();

                let mut group_map = serde_yaml::Mapping::new();
                group_map.insert("name".into(), group.name.clone().into());
                group_map.insert(
                    "interval".into(),
                    format!("{}s", group.interval.as_secs()).into(),
                );
                group_map.insert("rules".into(), rules.into());

                group_map.into()
            })
            .collect();

        let mut root = serde_yaml::Mapping::new();
        root.insert("groups".into(), yaml_groups.into());

        serde_yaml::to_string(&serde_yaml::Value::Mapping(root)).map_err(|e| {
            ObservabilityError::serialization(format!("Failed to serialize rules to YAML: {}", e))
        })
    }

    /// Generate plain text configuration (without serde_yaml)
    #[cfg(not(feature = "serde_yaml"))]
    pub fn generate_yaml(&self) -> ObservabilityResult<String> {
        let mut yaml = String::new();
        yaml.push_str("groups:\n");

        for group in &self.groups {
            yaml.push_str(&format!("  - name: {}\n", group.name));
            yaml.push_str(&format!("    interval: {}s\n", group.interval.as_secs()));
            yaml.push_str("    rules:\n");

            for rule in &group.rules {
                yaml.push_str(&format!("      - record: {}\n", rule.record));
                yaml.push_str(&format!("        expr: {}\n", rule.expr));

                if !rule.labels.is_empty() {
                    yaml.push_str("        labels:\n");
                    for (key, value) in &rule.labels {
                        yaml.push_str(&format!("          {}: {}\n", key, value));
                    }
                }
            }
        }

        Ok(yaml)
    }

    /// Validate all recording rules
    pub fn validate(&self) -> ObservabilityResult<()> {
        for group in &self.groups {
            if group.name.is_empty() {
                return Err(ObservabilityError::configuration(
                    "Recording rule group name cannot be empty",
                ));
            }

            for rule in &group.rules {
                if rule.record.is_empty() {
                    return Err(ObservabilityError::configuration(
                        "Recording rule record name cannot be empty",
                    ));
                }
                if rule.expr.is_empty() {
                    return Err(ObservabilityError::configuration(
                        "Recording rule expression cannot be empty",
                    ));
                }
            }
        }

        Ok(())
    }

    /// Get all rule groups
    pub fn groups(&self) -> &[RecordingRuleGroup] {
        &self.groups
    }
}

impl Default for RecordingRulesManager {
    fn default() -> Self {
        Self::with_spinkube_defaults()
    }
}

/// Create Istio-specific recording rules
fn create_istio_rules() -> RecordingRuleGroup {
    RecordingRuleGroup {
        name: "istio.rules".to_string(),
        interval: web_time::Duration::from_secs(30),
        rules: vec![
            RecordingRule {
                record: "istio:request_rate".to_string(),
                expr: "sum(rate(istio_requests_total[5m])) by (source_app, destination_service_name, destination_service_namespace)".to_string(),
                labels: HashMap::new(),
            },
            RecordingRule {
                record: "istio:request_duration_p99".to_string(),
                expr: "histogram_quantile(0.99, sum(rate(istio_request_duration_milliseconds_bucket[5m])) by (source_app, destination_service_name, le))".to_string(),
                labels: HashMap::new(),
            },
            RecordingRule {
                record: "istio:success_rate".to_string(),
                expr: "sum(rate(istio_requests_total{response_code!~\"5.*\"}[5m])) by (source_app, destination_service_name) / sum(rate(istio_requests_total[5m])) by (source_app, destination_service_name)".to_string(),
                labels: HashMap::new(),
            },
        ],
    }
}

/// Create SpinKube-specific recording rules
fn create_spinkube_rules() -> RecordingRuleGroup {
    RecordingRuleGroup {
        name: "spinkube.rules".to_string(),
        interval: web_time::Duration::from_secs(15),
        rules: vec![
            RecordingRule {
                record: "spinkube:cold_start_rate".to_string(),
                expr: "sum(rate(spin_cold_starts_total[1m])) by (app, namespace)".to_string(),
                labels: HashMap::new(),
            },
            RecordingRule {
                record: "spinkube:memory_usage_avg".to_string(),
                expr: "avg(spin_memory_usage_bytes) by (app, namespace)".to_string(),
                labels: HashMap::new(),
            },
            RecordingRule {
                record: "spinkube:execution_time_p95".to_string(),
                expr: "histogram_quantile(0.95, sum(rate(spin_execution_duration_seconds_bucket[5m])) by (app, namespace, le))".to_string(),
                labels: HashMap::new(),
            },
            RecordingRule {
                record: "spinkube:active_instances".to_string(),
                expr: "sum(spin_active_instances) by (app, namespace)".to_string(),
                labels: HashMap::new(),
            },
        ],
    }
}

/// Create LLM-specific recording rules
fn create_llm_rules() -> RecordingRuleGroup {
    RecordingRuleGroup {
        name: "llm.rules".to_string(),
        interval: web_time::Duration::from_secs(15),
        rules: vec![
            RecordingRule {
                record: "llm:request_rate_by_model".to_string(),
                expr: "sum(rate(llm_requests_total[5m])) by (model, provider, namespace)".to_string(),
                labels: HashMap::new(),
            },
            RecordingRule {
                record: "llm:tokens_per_second".to_string(),
                expr: "sum(rate(llm_tokens_total[5m])) by (model, provider, namespace)".to_string(),
                labels: HashMap::new(),
            },
            RecordingRule {
                record: "llm:error_rate".to_string(),
                expr: "sum(rate(llm_requests_total{status=\"error\"}[5m])) by (model, provider) / sum(rate(llm_requests_total[5m])) by (model, provider)".to_string(),
                labels: HashMap::new(),
            },
            RecordingRule {
                record: "llm:avg_latency_by_model".to_string(),
                expr: "sum(rate(llm_request_duration_seconds_sum[5m])) by (model, provider) / sum(rate(llm_request_duration_seconds_count[5m])) by (model, provider)".to_string(),
                labels: HashMap::new(),
            },
            RecordingRule {
                record: "llm:cost_per_hour".to_string(),
                expr: "sum(increase(llm_cost_total[1h])) by (model, provider, namespace)".to_string(),
                labels: {
                    let mut labels = HashMap::new();
                    labels.insert("unit".to_string(), "usd".to_string());
                    labels
                },
            },
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_recording_rule_creation() {
        let rule = RecordingRule {
            record: "test:metric".to_string(),
            expr: "sum(rate(test_metric[5m]))".to_string(),
            labels: HashMap::new(),
        };

        assert_eq!(rule.record, "test:metric");
        assert!(rule.expr.contains("rate"));
    }

    #[test]
    fn test_recording_rule_group() {
        let group = RecordingRuleGroup {
            name: "test.rules".to_string(),
            interval: web_time::Duration::from_secs(30),
            rules: vec![],
        };

        assert_eq!(group.name, "test.rules");
        assert_eq!(group.interval.as_secs(), 30);
    }

    #[test]
    fn test_recording_rules_manager() {
        let manager = RecordingRulesManager::with_spinkube_defaults();
        assert_eq!(manager.groups().len(), 3); // istio, spinkube, llm
        assert!(manager.validate().is_ok());
    }

    #[test]
    fn test_yaml_generation() {
        let mut manager = RecordingRulesManager::new();
        let rule = RecordingRule {
            record: "test:rate".to_string(),
            expr: "rate(test[5m])".to_string(),
            labels: HashMap::new(),
        };
        let group = RecordingRuleGroup {
            name: "test".to_string(),
            interval: web_time::Duration::from_secs(30),
            rules: vec![rule],
        };
        manager.add_group(group);

        let yaml = manager.generate_yaml();
        assert!(yaml.is_ok());

        let yaml_str = yaml.unwrap();
        assert!(yaml_str.contains("groups:"));
        assert!(yaml_str.contains("test:rate"));
        assert!(yaml_str.contains("rate(test[5m])"));
    }

    #[test]
    fn test_default_rules_content() {
        let manager = RecordingRulesManager::with_spinkube_defaults();

        // Check that we have expected rule groups
        let group_names: Vec<&str> = manager.groups().iter().map(|g| g.name.as_str()).collect();
        assert!(group_names.contains(&"istio.rules"));
        assert!(group_names.contains(&"spinkube.rules"));
        assert!(group_names.contains(&"llm.rules"));

        // Check that LLM rules exist
        let llm_group = manager
            .groups()
            .iter()
            .find(|g| g.name == "llm.rules")
            .unwrap();
        assert!(!llm_group.rules.is_empty());

        let rate_rule = llm_group
            .rules
            .iter()
            .find(|r| r.record == "llm:request_rate_by_model");
        assert!(rate_rule.is_some());
    }
}
