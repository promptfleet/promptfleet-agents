//! Axum helpers for the PromptFleet service instrumentation standard.
//!
//! Services keep their own route-to-operation normalization, while this module
//! centralizes timing, status classification, request metrics, and outcome logs.

use std::sync::Arc;
use std::time::Instant;

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::Response;

use crate::{
    LogLevel, Obs, ServiceInstrumentationExt, ServiceProtocol, ServiceStatus, finish_service_span,
};

pub type OperationNormalizer = fn(method: &str, path: &str) -> String;

#[derive(Clone)]
pub struct AxumHttpServiceTelemetry {
    obs: Obs,
    component: Arc<str>,
    operation_normalizer: OperationNormalizer,
    request_log_message: Arc<str>,
}

impl AxumHttpServiceTelemetry {
    pub fn new(
        obs: Obs,
        component: impl Into<Arc<str>>,
        operation_normalizer: OperationNormalizer,
    ) -> Self {
        Self {
            obs,
            component: component.into(),
            operation_normalizer,
            request_log_message: Arc::from("service request completed"),
        }
    }

    pub fn with_request_log_message(mut self, message: impl Into<Arc<str>>) -> Self {
        self.request_log_message = message.into();
        self
    }
}

pub async fn axum_http_service_telemetry(
    State(telemetry): State<AxumHttpServiceTelemetry>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let method = request.method().as_str().to_string();
    let path = request.uri().path().to_string();
    let operation = (telemetry.operation_normalizer)(&method, &path);
    let started = Instant::now();
    let span = telemetry.obs.service_request_span(
        telemetry.component.as_ref(),
        &operation,
        ServiceProtocol::Http,
        &[],
    );
    let response = next.run(request).await;
    let status = response.status();
    let latency_ms = started.elapsed().as_secs_f64() * 1000.0;
    let status_label = service_status_from_http(status);
    finish_service_span(&span, status_label);
    span.add_attribute("http.status_code", status.as_u16().to_string().as_str());
    telemetry.obs.record_service_request(
        telemetry.component.as_ref(),
        &operation,
        ServiceProtocol::Http,
        status_label,
        latency_ms,
    );
    telemetry.obs.log_service_http_event(
        if status.is_server_error() {
            LogLevel::Warn
        } else {
            LogLevel::Info
        },
        telemetry.request_log_message.as_ref(),
        telemetry.component.as_ref(),
        &operation,
        status_label,
        status.as_u16(),
    );
    response
}

pub fn service_status_from_http(status: StatusCode) -> ServiceStatus {
    if status.is_server_error() {
        ServiceStatus::Error
    } else {
        ServiceStatus::Ok
    }
}

pub fn default_route_operation(method: &str, path: &str) -> String {
    let route = if path == "/healthz" || path == "/readyz" {
        path.trim_start_matches('/').to_string()
    } else {
        path.trim_start_matches('/')
            .trim_matches('/')
            .replace('/', "_")
    };
    format!("{}_{}", method.to_ascii_lowercase(), route)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_status_from_http_treats_client_errors_as_handled() {
        assert_eq!(
            service_status_from_http(StatusCode::UNAUTHORIZED),
            ServiceStatus::Ok
        );
        assert_eq!(
            service_status_from_http(StatusCode::NOT_FOUND),
            ServiceStatus::Ok
        );
    }

    #[test]
    fn service_status_from_http_keeps_server_errors_as_errors() {
        assert_eq!(
            service_status_from_http(StatusCode::INTERNAL_SERVER_ERROR),
            ServiceStatus::Error
        );
    }
}
