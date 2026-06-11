//! PromptFleet service instrumentation standard.
//!
//! This module is intentionally small and low-cardinality. It gives platform
//! services one shared contract for resource identity, request metrics,
//! dependency metrics, background jobs, health, and build metadata.

use std::collections::HashMap;

use observability_core::traits::LogLevel;

use crate::{ObsHandle, SpanGuard, SpanStatus, attr, metric, value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceIdentity {
    pub name: String,
    pub namespace: String,
    pub version: String,
    pub instance_id: Option<String>,
    pub deployment_environment: Option<String>,
}

impl ServiceIdentity {
    pub fn new(
        name: impl Into<String>,
        namespace: impl Into<String>,
        version: impl Into<String>,
    ) -> Self {
        Self {
            name: non_empty_or(name.into(), "unknown"),
            namespace: non_empty_or(namespace.into(), "default"),
            version: non_empty_or(version.into(), "unknown"),
            instance_id: env_non_empty("HOSTNAME")
                .or_else(|| env_non_empty("POD_NAME"))
                .or_else(|| env_non_empty("SERVICE_INSTANCE_ID")),
            deployment_environment: env_non_empty("DEPLOYMENT_ENVIRONMENT_NAME")
                .or_else(|| env_non_empty("OTEL_DEPLOYMENT_ENVIRONMENT"))
                .or_else(|| env_non_empty("PF_DEPLOYMENT_ENVIRONMENT")),
        }
    }

    pub fn from_env(default_name: &str, default_namespace: &str, default_version: &str) -> Self {
        Self::new(
            env_non_empty("OTEL_SERVICE_NAME").unwrap_or_else(|| default_name.to_string()),
            env_non_empty("OTEL_SERVICE_NAMESPACE")
                .or_else(|| env_non_empty("PF_NAMESPACE"))
                .unwrap_or_else(|| default_namespace.to_string()),
            env_non_empty("OTEL_SERVICE_VERSION").unwrap_or_else(|| default_version.to_string()),
        )
    }

    pub fn with_instance_id(mut self, instance_id: impl Into<String>) -> Self {
        self.instance_id = Some(non_empty_or(instance_id.into(), "unknown"));
        self
    }

    pub fn with_deployment_environment(mut self, environment: impl Into<String>) -> Self {
        self.deployment_environment = Some(non_empty_or(environment.into(), "unknown"));
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceTier {
    Control,
    Data,
    Runtime,
    Edge,
    Observability,
}

impl ServiceTier {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Control => "control",
            Self::Data => "data",
            Self::Runtime => "runtime",
            Self::Edge => "edge",
            Self::Observability => "observability",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceCriticality {
    Critical,
    High,
    Medium,
    Low,
}

impl ServiceCriticality {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Critical => "critical",
            Self::High => "high",
            Self::Medium => "medium",
            Self::Low => "low",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceKind {
    Api,
    Worker,
    Controller,
    Frontend,
    Datastore,
    Gateway,
}

impl ServiceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Api => "api",
            Self::Worker => "worker",
            Self::Controller => "controller",
            Self::Frontend => "frontend",
            Self::Datastore => "datastore",
            Self::Gateway => "gateway",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceProfile {
    pub identity: ServiceIdentity,
    pub owner: String,
    pub tier: ServiceTier,
    pub criticality: ServiceCriticality,
    pub kind: ServiceKind,
    pub system: bool,
}

impl ServiceProfile {
    pub fn new(
        identity: ServiceIdentity,
        owner: impl Into<String>,
        tier: ServiceTier,
        criticality: ServiceCriticality,
        kind: ServiceKind,
    ) -> Self {
        Self {
            identity,
            owner: non_empty_or(owner.into(), "unknown"),
            tier,
            criticality,
            kind,
            system: true,
        }
    }

    pub fn resource_attributes(&self) -> HashMap<String, String> {
        let mut attrs = HashMap::new();
        attrs.insert(attr::SERVICE_NAME.to_string(), self.identity.name.clone());
        attrs.insert(
            attr::SERVICE_NAMESPACE.to_string(),
            self.identity.namespace.clone(),
        );
        attrs.insert(
            attr::SERVICE_VERSION.to_string(),
            self.identity.version.clone(),
        );
        if let Some(instance_id) = &self.identity.instance_id {
            attrs.insert(attr::SERVICE_INSTANCE_ID.to_string(), instance_id.clone());
        }
        if let Some(environment) = &self.identity.deployment_environment {
            attrs.insert(
                attr::DEPLOYMENT_ENVIRONMENT_NAME.to_string(),
                environment.clone(),
            );
        }
        attrs.insert(attr::PF_SERVICE_OWNER.to_string(), self.owner.clone());
        attrs.insert(
            attr::PF_SERVICE_TIER.to_string(),
            self.tier.as_str().to_string(),
        );
        attrs.insert(
            attr::PF_SERVICE_CRITICALITY.to_string(),
            self.criticality.as_str().to_string(),
        );
        attrs.insert(
            attr::PF_SERVICE_KIND.to_string(),
            self.kind.as_str().to_string(),
        );
        attrs.insert(attr::PF_SYSTEM.to_string(), self.system.to_string());
        attrs
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceStatus {
    Ok,
    Error,
    Timeout,
    Cancelled,
}

impl ServiceStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ok => value::STATUS_OK,
            Self::Error => value::STATUS_ERROR,
            Self::Timeout => value::STATUS_TIMEOUT,
            Self::Cancelled => value::STATUS_CANCELLED,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceProtocol {
    Http,
    Graphql,
    Grpc,
    JsonRpc,
    Worker,
}

impl ServiceProtocol {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Http => value::PROTOCOL_HTTP,
            Self::Graphql => value::PROTOCOL_GRAPHQL,
            Self::Grpc => value::PROTOCOL_GRPC,
            Self::JsonRpc => value::PROTOCOL_JSONRPC,
            Self::Worker => value::PROTOCOL_WORKER,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DependencyKind {
    Database,
    Http,
    Queue,
    Cache,
    Llm,
    Kubernetes,
    Observability,
}

impl DependencyKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Database => "database",
            Self::Http => "http",
            Self::Queue => "queue",
            Self::Cache => "cache",
            Self::Llm => "llm",
            Self::Kubernetes => "k8s",
            Self::Observability => "observability",
        }
    }
}

pub trait ServiceInstrumentationExt: ObsHandle {
    fn service_request_span(
        &self,
        component: &str,
        operation: &str,
        protocol: ServiceProtocol,
        attrs: &[(&str, &str)],
    ) -> SpanGuard {
        let mut span_attrs = vec![
            (attr::COMPONENT, component),
            (attr::OPERATION, operation),
            ("protocol", protocol.as_str()),
            (attr::PF_OUTCOME, value::OUTCOME_OK),
        ];
        span_attrs.extend_from_slice(attrs);
        self.span("pf.service.request", &span_attrs)
    }

    fn record_service_request(
        &self,
        component: &str,
        operation: &str,
        protocol: ServiceProtocol,
        status: ServiceStatus,
        duration_ms: f64,
    ) {
        let labels = [
            (attr::COMPONENT, component),
            (attr::OPERATION, operation),
            (attr::STATUS, status.as_str()),
            ("protocol", protocol.as_str()),
        ];
        self.metric(metric::PF_SERVICE_REQUESTS_TOTAL, 1.0, &labels);
        self.metric(
            metric::PF_SERVICE_REQUEST_DURATION_MS,
            duration_ms,
            &labels,
        );
    }

    fn record_dependency_call(
        &self,
        component: &str,
        operation: &str,
        dependency_kind: DependencyKind,
        dependency: &str,
        status: ServiceStatus,
        duration_ms: f64,
    ) {
        let labels = [
            (attr::COMPONENT, component),
            (attr::OPERATION, operation),
            (attr::STATUS, status.as_str()),
            ("dependency_kind", dependency_kind.as_str()),
            ("dependency", dependency),
        ];
        self.metric(metric::PF_SERVICE_DEPENDENCY_REQUESTS_TOTAL, 1.0, &labels);
        self.metric(
            metric::PF_SERVICE_DEPENDENCY_DURATION_MS,
            duration_ms,
            &labels,
        );
    }

    fn record_background_job(
        &self,
        component: &str,
        operation: &str,
        status: ServiceStatus,
        duration_ms: f64,
    ) {
        let labels = [
            (attr::COMPONENT, component),
            (attr::OPERATION, operation),
            (attr::STATUS, status.as_str()),
        ];
        self.metric(metric::PF_SERVICE_BACKGROUND_JOBS_TOTAL, 1.0, &labels);
        self.metric(
            metric::PF_SERVICE_BACKGROUND_JOB_DURATION_MS,
            duration_ms,
            &labels,
        );
    }

    fn record_health_state(&self, component: &str, healthy: bool) {
        let status = if healthy {
            ServiceStatus::Ok
        } else {
            ServiceStatus::Error
        };
        self.metric(
            metric::PF_SERVICE_HEALTH_STATE,
            if healthy { 1.0 } else { 0.0 },
            &[(attr::COMPONENT, component), (attr::STATUS, status.as_str())],
        );
    }

    fn record_build_info(&self, version: &str) {
        self.metric(metric::PF_SERVICE_BUILD_INFO, 1.0, &[("version", version)]);
    }

    fn log_service_event(
        &self,
        level: LogLevel,
        message: &str,
        component: &str,
        operation: &str,
        status: ServiceStatus,
    ) {
        self.log_kv(
            level,
            message,
            &[
                (attr::COMPONENT, component),
                (attr::OPERATION, operation),
                (attr::STATUS, status.as_str()),
            ],
        );
    }
}

impl<T: ObsHandle + ?Sized> ServiceInstrumentationExt for T {}

pub fn finish_service_span(span: &SpanGuard, status: ServiceStatus) {
    span.add_attribute(attr::STATUS, status.as_str());
    span.add_attribute(
        attr::PF_OUTCOME,
        match status {
            ServiceStatus::Ok => value::OUTCOME_OK,
            ServiceStatus::Error => value::OUTCOME_ERROR,
            ServiceStatus::Timeout => value::OUTCOME_TIMEOUT,
            ServiceStatus::Cancelled => value::OUTCOME_CANCELLED,
        },
    );
    if status == ServiceStatus::Ok {
        span.set_status(SpanStatus::Ok);
    } else {
        span.set_status(SpanStatus::Error);
    }
}

fn env_non_empty(key: &str) -> Option<String> {
    std::env::var(key).ok().and_then(|value| {
        let value = value.trim().to_string();
        if value.is_empty() { None } else { Some(value) }
    })
}

fn non_empty_or(value: String, fallback: &str) -> String {
    let value = value.trim().to_string();
    if value.is_empty() {
        fallback.to_string()
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter_metric_labels;

    #[test]
    fn service_profile_resource_attributes_include_otel_and_pf_identity() {
        let identity =
            ServiceIdentity::new("console-control-plane", "promptfleet-system", "dev")
                .with_instance_id("pod-1")
                .with_deployment_environment("development");
        let profile = ServiceProfile::new(
            identity,
            "platform",
            ServiceTier::Control,
            ServiceCriticality::Critical,
            ServiceKind::Api,
        );

        let attrs = profile.resource_attributes();

        assert_eq!(attrs[attr::SERVICE_NAME], "console-control-plane");
        assert_eq!(attrs[attr::SERVICE_NAMESPACE], "promptfleet-system");
        assert_eq!(attrs[attr::SERVICE_VERSION], "dev");
        assert_eq!(attrs[attr::SERVICE_INSTANCE_ID], "pod-1");
        assert_eq!(attrs[attr::DEPLOYMENT_ENVIRONMENT_NAME], "development");
        assert_eq!(attrs[attr::PF_SERVICE_OWNER], "platform");
        assert_eq!(attrs[attr::PF_SERVICE_TIER], "control");
        assert_eq!(attrs[attr::PF_SERVICE_CRITICALITY], "critical");
        assert_eq!(attrs[attr::PF_SERVICE_KIND], "api");
        assert_eq!(attrs[attr::PF_SYSTEM], "true");
    }

    #[test]
    fn service_identity_defaults_empty_values() {
        let identity = ServiceIdentity::new("", "", "");

        assert_eq!(identity.name, "unknown");
        assert_eq!(identity.namespace, "default");
        assert_eq!(identity.version, "unknown");
    }

    #[test]
    fn standard_metric_labels_are_allowed_and_unknown_labels_drop() {
        let labels = [
            (attr::COMPONENT, "api"),
            (attr::OPERATION, "list"),
            (attr::STATUS, "ok"),
            ("protocol", "http"),
            ("dependency_kind", "database"),
            ("dependency", "postgres"),
            ("tenant_id", "high-cardinality"),
        ];

        let filtered = filter_metric_labels(&labels);
        let keys = filtered.iter().map(|(key, _)| *key).collect::<Vec<_>>();

        assert_eq!(
            keys,
            vec![
                attr::COMPONENT,
                attr::OPERATION,
                attr::STATUS,
                "protocol",
                "dependency_kind",
                "dependency",
            ]
        );
    }
}
