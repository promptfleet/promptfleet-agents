//! Resource attribute management for OpenTelemetry

use std::collections::HashMap;

const RESERVED_KEYS: &[&str] = &["service.name", "service.version", "service.namespace"];

/// Manager for OpenTelemetry resource attributes.
///
/// Reserved keys (`service.name`, `service.version`, `service.namespace`) are
/// always set from the constructor parameters and cannot be overwritten by
/// custom attributes.
pub struct ResourceAttributeManager {
    service_name: String,
    service_version: String,
    service_namespace: String,
    custom_attributes: HashMap<String, String>,
}

impl ResourceAttributeManager {
    /// Create a new resource attribute manager.
    ///
    /// Any `custom_attributes` whose key collides with a reserved `service.*`
    /// key will be silently dropped.
    pub fn new(
        service_name: &str,
        service_version: &str,
        service_namespace: &str,
        custom_attributes: HashMap<String, String>,
    ) -> Self {
        let filtered: HashMap<String, String> = custom_attributes
            .into_iter()
            .filter(|(k, _)| !RESERVED_KEYS.contains(&k.as_str()))
            .collect();

        Self {
            service_name: service_name.to_string(),
            service_version: service_version.to_string(),
            service_namespace: service_namespace.to_string(),
            custom_attributes: filtered,
        }
    }

    /// Get all resource attributes.
    ///
    /// Reserved `service.*` keys are always present and cannot be overridden.
    pub fn get_all_attributes(&self) -> HashMap<String, String> {
        let mut attributes = HashMap::new();

        // Custom attributes first (cannot contain reserved keys)
        for (key, value) in &self.custom_attributes {
            attributes.insert(key.clone(), value.clone());
        }

        // Reserved keys always win
        attributes.insert("service.name".to_string(), self.service_name.clone());
        attributes.insert("service.version".to_string(), self.service_version.clone());
        attributes.insert(
            "service.namespace".to_string(),
            self.service_namespace.clone(),
        );

        attributes
    }

    /// Get service name
    pub fn service_name(&self) -> &str {
        &self.service_name
    }

    /// Get service version
    pub fn service_version(&self) -> &str {
        &self.service_version
    }

    /// Get service namespace
    pub fn service_namespace(&self) -> &str {
        &self.service_namespace
    }

    /// Add a custom attribute.
    ///
    /// Attempting to set a reserved key (`service.name`, `service.version`,
    /// `service.namespace`) is a no-op.
    pub fn add_attribute(&mut self, key: String, value: String) {
        if !RESERVED_KEYS.contains(&key.as_str()) {
            self.custom_attributes.insert(key, value);
        }
    }

    /// Remove a custom attribute.
    ///
    /// Reserved keys cannot be removed.
    pub fn remove_attribute(&mut self, key: &str) {
        if !RESERVED_KEYS.contains(&key) {
            self.custom_attributes.remove(key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manager_with_defaults() -> ResourceAttributeManager {
        ResourceAttributeManager::new("my-agent", "0.1.0", "pf-mesh", HashMap::new())
    }

    #[test]
    fn test_standard_attributes_always_present() {
        let mgr = manager_with_defaults();
        let attrs = mgr.get_all_attributes();
        assert_eq!(attrs["service.name"], "my-agent");
        assert_eq!(attrs["service.version"], "0.1.0");
        assert_eq!(attrs["service.namespace"], "pf-mesh");
    }

    #[test]
    fn test_custom_attributes_merged() {
        let mut custom = HashMap::new();
        custom.insert("deployment.environment".to_string(), "staging".to_string());
        custom.insert("team".to_string(), "platform".to_string());

        let mgr = ResourceAttributeManager::new("svc", "1.0", "ns", custom);
        let attrs = mgr.get_all_attributes();

        assert_eq!(attrs["deployment.environment"], "staging");
        assert_eq!(attrs["team"], "platform");
        assert_eq!(attrs["service.name"], "svc");
    }

    #[test]
    fn test_custom_cannot_overwrite_reserved_keys() {
        let mut custom = HashMap::new();
        custom.insert("service.name".to_string(), "hacked".to_string());
        custom.insert("service.version".to_string(), "999".to_string());
        custom.insert("service.namespace".to_string(), "evil".to_string());
        custom.insert("safe.key".to_string(), "safe.value".to_string());

        let mgr = ResourceAttributeManager::new("real-svc", "1.0", "real-ns", custom);
        let attrs = mgr.get_all_attributes();

        assert_eq!(attrs["service.name"], "real-svc");
        assert_eq!(attrs["service.version"], "1.0");
        assert_eq!(attrs["service.namespace"], "real-ns");
        assert_eq!(attrs["safe.key"], "safe.value");
    }

    #[test]
    fn test_add_attribute_respects_reserved() {
        let mut mgr = manager_with_defaults();
        mgr.add_attribute("service.name".to_string(), "sneaky".to_string());
        assert_eq!(mgr.service_name(), "my-agent");
        assert_eq!(mgr.get_all_attributes()["service.name"], "my-agent");
    }

    #[test]
    fn test_add_and_remove_custom_attribute() {
        let mut mgr = manager_with_defaults();
        mgr.add_attribute("env".to_string(), "prod".to_string());
        assert_eq!(mgr.get_all_attributes()["env"], "prod");

        mgr.remove_attribute("env");
        assert!(!mgr.get_all_attributes().contains_key("env"));
    }

    #[test]
    fn test_remove_reserved_key_is_noop() {
        let mut mgr = manager_with_defaults();
        mgr.remove_attribute("service.name");
        assert_eq!(mgr.get_all_attributes()["service.name"], "my-agent");
    }

    #[test]
    fn test_accessors() {
        let mgr = manager_with_defaults();
        assert_eq!(mgr.service_name(), "my-agent");
        assert_eq!(mgr.service_version(), "0.1.0");
        assert_eq!(mgr.service_namespace(), "pf-mesh");
    }

    #[test]
    fn test_empty_custom_attributes() {
        let mgr = manager_with_defaults();
        let attrs = mgr.get_all_attributes();
        assert_eq!(attrs.len(), 3);
    }
}
