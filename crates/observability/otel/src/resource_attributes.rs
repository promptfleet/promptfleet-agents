//! Resource attribute management for OpenTelemetry

use std::collections::HashMap;

/// Manager for OpenTelemetry resource attributes
pub struct ResourceAttributeManager {
    service_name: String,
    service_version: String,
    service_namespace: String,
    custom_attributes: HashMap<String, String>,
}

impl ResourceAttributeManager {
    /// Create a new resource attribute manager
    pub fn new(
        service_name: &str,
        service_version: &str,
        service_namespace: &str,
        custom_attributes: HashMap<String, String>,
    ) -> Self {
        Self {
            service_name: service_name.to_string(),
            service_version: service_version.to_string(),
            service_namespace: service_namespace.to_string(),
            custom_attributes,
        }
    }

    /// Get all resource attributes
    pub fn get_all_attributes(&self) -> HashMap<String, String> {
        let mut attributes = HashMap::new();

        // Standard OpenTelemetry resource attributes
        attributes.insert("service.name".to_string(), self.service_name.clone());
        attributes.insert("service.version".to_string(), self.service_version.clone());
        attributes.insert(
            "service.namespace".to_string(),
            self.service_namespace.clone(),
        );

        // Add custom attributes
        for (key, value) in &self.custom_attributes {
            attributes.insert(key.clone(), value.clone());
        }

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

    /// Add a custom attribute
    pub fn add_attribute(&mut self, key: String, value: String) {
        self.custom_attributes.insert(key, value);
    }

    /// Remove a custom attribute
    pub fn remove_attribute(&mut self, key: &str) {
        self.custom_attributes.remove(key);
    }
}
