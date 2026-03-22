//! OTLP collector client for exporting telemetry data

use observability_core::traits::SpanStatus;
use observability_core::{ObservabilityError, ObservabilityResult};
use std::collections::HashMap;
use web_time::{Duration, Instant};

#[cfg(all(target_arch = "wasm32", target_os = "wasi"))]
use spin_sdk::http::{self, IncomingResponse, Request};
#[cfg(all(target_arch = "wasm32", target_os = "wasi"))]
use url::Url;

/// OpenTelemetry span data for export
#[derive(Debug, Clone)]
pub struct OtelSpanData {
    pub span_id: String,
    pub trace_id: String,
    pub parent_span_id: Option<String>,
    pub name: String,
    pub start_time: Instant,
    pub end_time: Option<Instant>,
    pub status: SpanStatus,
    pub attributes: HashMap<String, String>,
    pub events: Vec<SpanEvent>,
}

#[derive(Debug, Clone)]
pub struct SpanEvent {
    pub name: String,
    pub timestamp: Instant,
    pub attributes: HashMap<String, String>,
}

/// OTLP collector client for sending telemetry data
pub struct CollectorClient {
    endpoint: String,
    client: reqwest::Client,
}

/// Minimal HTTP request representation for OTLP export.
///
/// This exists so we can unit-test the exact request we'd send from WASI (URL, headers, body)
/// without needing Alloy/Tempo in the loop.
#[derive(Debug, Clone)]
struct OtlpHttpRequest {
    url: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl CollectorClient {
    /// Create a new collector client (async - preferred for SpinKube environments)
    pub async fn new(endpoint: &str, _timeout: Duration) -> ObservabilityResult<Self> {
        // ✅ **WASM-NATIVE FIX**: keep WASM compatibility while ensuring native exports
        // are bounded-time (timeouts) to avoid hanging flush loops forever.
        let builder = reqwest::Client::builder();
        // Note: reqwest timeouts are not available on all wasm32 targets (including WASI),
        // but they *are* desirable on native to avoid hanging flush loops forever.
        #[cfg(not(target_arch = "wasm32"))]
        let builder = builder.timeout(_timeout).connect_timeout(_timeout);

        let client = builder.build().map_err(|e| {
            ObservabilityError::transport(format!("Failed to create HTTP client: {}", e))
        })?;

        Ok(Self {
            endpoint: endpoint.to_string(),
            client,
        })
    }

    /// Create a new collector client synchronously (for WASM environments)
    pub fn new_sync(endpoint: &str, _timeout: Duration) -> ObservabilityResult<Self> {
        // ✅ **WASM-NATIVE FIX**: keep WASM compatibility while ensuring native exports
        // are bounded-time (timeouts) to avoid hanging flush loops forever.
        let builder = reqwest::Client::builder();
        #[cfg(not(target_arch = "wasm32"))]
        let builder = builder.timeout(_timeout).connect_timeout(_timeout);

        let client = builder.build().map_err(|e| {
            ObservabilityError::transport(format!("Failed to create HTTP client: {}", e))
        })?;

        Ok(Self {
            endpoint: endpoint.to_string(),
            client,
        })
    }

    /// Export spans to the OTLP collector
    pub async fn export_spans(
        &self,
        spans: Vec<OtelSpanData>,
        resource_manager: &crate::resource_attributes::ResourceAttributeManager,
    ) -> ObservabilityResult<()> {
        if spans.is_empty() {
            return Ok(());
        }

        #[cfg(feature = "structured-logging")]
        {
            // OTLP/HTTP in Alloy expects protobuf (`application/x-protobuf`), not OTLP JSON.
            let req = self.build_otlp_traces_export_http_request(spans, resource_manager)?;
            self.send_otlp_http_request(req).await.map_err(|e| {
                ObservabilityError::transport(format!("Failed to export spans: {}", e))
            })?;

            Ok(())
        }

        #[cfg(not(feature = "structured-logging"))]
        {
            Err(ObservabilityError::feature_not_enabled(
                "structured-logging required for OTLP export",
            ))
        }
    }

    /// Export metrics to the OTLP collector
    pub async fn export_metrics(
        &self,
        metrics: Vec<MetricData>,
        resource_manager: &crate::resource_attributes::ResourceAttributeManager,
    ) -> ObservabilityResult<()> {
        if metrics.is_empty() {
            return Ok(());
        }

        #[cfg(feature = "structured-logging")]
        {
            // Convert to OTLP-compatible JSON format
            let otlp_payload = self.create_otlp_metrics_payload(metrics, resource_manager)?;

            let url = format!("{}/v1/metrics", self.endpoint);
            self.send_json(&url, &otlp_payload).await.map_err(|e| {
                ObservabilityError::transport(format!("Failed to export metrics: {}", e))
            })?;

            Ok(())
        }

        #[cfg(not(feature = "structured-logging"))]
        {
            Err(ObservabilityError::feature_not_enabled(
                "structured-logging required for OTLP export",
            ))
        }
    }

    /// Export logs to the OTLP collector
    pub async fn export_logs(
        &self,
        logs: Vec<LogData>,
        resource_manager: &crate::resource_attributes::ResourceAttributeManager,
    ) -> ObservabilityResult<()> {
        if logs.is_empty() {
            return Ok(());
        }

        #[cfg(feature = "structured-logging")]
        {
            // Convert to OTLP-compatible JSON format
            let otlp_payload = self.create_otlp_logs_payload(logs, resource_manager)?;

            let url = format!("{}/v1/logs", self.endpoint);
            self.send_json(&url, &otlp_payload).await.map_err(|e| {
                ObservabilityError::transport(format!("Failed to export logs: {}", e))
            })?;

            Ok(())
        }

        #[cfg(not(feature = "structured-logging"))]
        {
            Err(ObservabilityError::feature_not_enabled(
                "structured-logging required for OTLP export",
            ))
        }
    }

    #[cfg(feature = "structured-logging")]
    async fn send_json(&self, url: &str, payload: &serde_json::Value) -> ObservabilityResult<()> {
        #[cfg(all(target_arch = "wasm32", target_os = "wasi"))]
        {
            let body = serde_json::to_vec(payload).map_err(|e| {
                ObservabilityError::transport(format!("Failed to serialize OTLP payload: {}", e))
            })?;

            log::info!(
                "otel:export_http_wasi_start url={} bytes={}",
                url,
                body.len()
            );
            let parsed = Url::parse(&url).map_err(|e| {
                ObservabilityError::transport(format!("Invalid OTLP URL '{url}': {e}"))
            })?;

            // Spin forbids setting `Host` explicitly; the runtime derives it from the URL authority.
            // If you use an IP literal (e.g. http://10.x.x.x:4318), the derived Host is the IP,
            // which may break virtual-host routing behind meshes/gateways. Prefer Service DNS.
            let authority = parsed.authority();
            let host_only = authority.split(':').next().unwrap_or(authority);
            if host_only.parse::<std::net::IpAddr>().is_ok() {
                log::warn!(
                    "otel:export_http_wasi_ip_authority url={} authority={} hint=use_service_dns",
                    url,
                    authority
                );
            }

            // Prefer the non-streaming `Request` API unless you truly need streaming.
            //
            // In Spin/WASI, outbound streaming semantics can be host-dependent and may lead to
            // hangs if the request body is not finalized the way the host expects. For OTLP/HTTP
            // we always have the full payload in memory, so a non-streaming request is a better,
            // more reliable fit.
            //
            // We still request an `IncomingResponse` so we can read the status without buffering
            // the response body (some peers keep response bodies open indefinitely).
            let mut request = Request::new(spin_sdk::http::Method::Post, parsed.as_str());
            request.set_header("content-type", "application/json");
            request.set_header("accept", "application/json");
            *request.body_mut() = body;

            let incoming_response = http::send::<_, IncomingResponse>(request)
                .await
                .map_err(|e| ObservabilityError::transport(format!("Spin HTTP send error: {e}")))?;

            let status = incoming_response.status();
            log::info!("otel:export_http_wasi_done url={} status={}", url, status);
            if status < 200 || status >= 300 {
                // Avoid reading the response body in WASI: some peers keep it open, which can hang.
                return Err(ObservabilityError::transport(format!(
                    "OTLP collector returned error: {status}"
                )));
            }

            Ok(())
        }

        #[cfg(not(all(target_arch = "wasm32", target_os = "wasi")))]
        {
            let response = self
                .client
                .post(url)
                .header("Content-Type", "application/json")
                .json(payload)
                .send()
                .await
                .map_err(|e| {
                    ObservabilityError::transport(format!("Failed to export telemetry: {}", e))
                })?;

            if !response.status().is_success() {
                let status = response.status();
                let error_text = response.text().await.unwrap_or_default();
                return Err(ObservabilityError::transport(format!(
                    "OTLP collector returned error: {} - {}",
                    status, error_text
                )));
            }

            Ok(())
        }
    }

    #[cfg(feature = "structured-logging")]
    async fn send_otlp_http_request(&self, req: OtlpHttpRequest) -> ObservabilityResult<()> {
        #[cfg(all(target_arch = "wasm32", target_os = "wasi"))]
        {
            let url = req.url;
            let body = req.body;

            // Optional self-check to debug Spin/WASI behavior without involving Alloy/Tempo:
            // decode our own protobuf payload and log a tiny summary.
            //
            // Enable by setting `PF_OBS_OTLP_PROTO_DEBUG=true` on the component.
            if std::env::var("PF_OBS_OTLP_PROTO_DEBUG")
                .ok()
                .as_deref()
                .map(|v| v == "true" || v == "1")
                .unwrap_or(false)
            {
                use opentelemetry_proto::tonic::collector::trace::v1::ExportTraceServiceRequest;
                use prost::Message;
                match ExportTraceServiceRequest::decode(body.as_slice()) {
                    Ok(decoded) => {
                        let span_count = decoded
                            .resource_spans
                            .iter()
                            .flat_map(|rs| rs.scope_spans.iter())
                            .map(|ss| ss.spans.len())
                            .sum::<usize>();
                        let svc_name = decoded
                            .resource_spans
                            .get(0)
                            .and_then(|rs| rs.resource.as_ref())
                            .and_then(|r| {
                                r.attributes.iter().find_map(|kv| {
                                    if kv.key == "service.name" {
                                        kv.value
                                            .as_ref()
                                            .and_then(|v| v.value.as_ref())
                                            .and_then(|v| match v {
                                                opentelemetry_proto::tonic::common::v1::any_value::Value::StringValue(s) => Some(s.as_str()),
                                                _ => None,
                                            })
                                    } else {
                                        None
                                    }
                                })
                            })
                            .unwrap_or("<missing>");
                        log::info!(
                            "otel:otlp_proto_selfcheck ok=true spans={} service.name={}",
                            span_count,
                            svc_name
                        );
                    }
                    Err(e) => {
                        log::warn!("otel:otlp_proto_selfcheck ok=false err={}", e);
                    }
                }
            }

            log::info!(
                "otel:export_http_wasi_start url={} bytes={}",
                url,
                body.len()
            );
            let parsed = Url::parse(&url).map_err(|e| {
                ObservabilityError::transport(format!("Invalid OTLP URL '{url}': {e}"))
            })?;

            let authority = parsed.authority();
            let host_only = authority.split(':').next().unwrap_or(authority);
            if host_only.parse::<std::net::IpAddr>().is_ok() {
                log::warn!(
                    "otel:export_http_wasi_ip_authority url={} authority={} hint=use_service_dns",
                    url,
                    authority
                );
            }

            let mut request = Request::new(spin_sdk::http::Method::Post, parsed.as_str());
            for (k, v) in req.headers {
                request.set_header(&k, &v);
            }
            *request.body_mut() = body;

            let incoming_response = http::send::<_, IncomingResponse>(request)
                .await
                .map_err(|e| ObservabilityError::transport(format!("Spin HTTP send error: {e}")))?;

            let status = incoming_response.status();
            log::info!("otel:export_http_wasi_done url={} status={}", url, status);
            if status < 200 || status >= 300 {
                return Err(ObservabilityError::transport(format!(
                    "OTLP collector returned error: {status}"
                )));
            }

            Ok(())
        }

        #[cfg(not(all(target_arch = "wasm32", target_os = "wasi")))]
        {
            let mut request_builder = self.client.post(&req.url);
            for (k, v) in req.headers {
                request_builder = request_builder.header(k, v);
            }
            let response = self
                .client
                .post(&req.url)
                .headers(
                    request_builder
                        .build()
                        .map_err(|e| {
                            ObservabilityError::transport(format!(
                                "Failed to build OTLP request: {e}"
                            ))
                        })?
                        .headers()
                        .clone(),
                )
                .body(req.body)
                .send()
                .await
                .map_err(|e| {
                    ObservabilityError::transport(format!("Failed to export telemetry: {}", e))
                })?;

            if !response.status().is_success() {
                let status = response.status();
                let error_text = response.text().await.unwrap_or_default();
                return Err(ObservabilityError::transport(format!(
                    "OTLP collector returned error: {} - {}",
                    status, error_text
                )));
            }

            Ok(())
        }
    }

    /// Build the OTLP/HTTP protobuf request for exporting traces.
    ///
    /// This is intentionally "pure" (no network), to allow fast unit tests that validate:
    /// - URL and headers
    /// - protobuf payload decodes and includes expected fields
    #[cfg(feature = "structured-logging")]
    fn build_otlp_traces_export_http_request(
        &self,
        spans: Vec<OtelSpanData>,
        resource_manager: &crate::resource_attributes::ResourceAttributeManager,
    ) -> ObservabilityResult<OtlpHttpRequest> {
        let url = format!("{}/v1/traces", self.endpoint);
        let body = self.create_otlp_spans_request_protobuf(spans, resource_manager)?;
        Ok(OtlpHttpRequest {
            url,
            headers: vec![
                // Use canonical casing to avoid any host quirks.
                (
                    "Content-Type".to_string(),
                    "application/x-protobuf".to_string(),
                ),
                ("Accept".to_string(), "application/json".to_string()),
            ],
            body,
        })
    }

    /// Create OTLP traces payload (protobuf) according to OTLP specification.
    ///
    /// Alloy's OTLP receiver expects `ExportTraceServiceRequest` protobuf bytes over HTTP.
    #[cfg(feature = "structured-logging")]
    fn create_otlp_spans_request_protobuf(
        &self,
        spans: Vec<OtelSpanData>,
        resource_manager: &crate::resource_attributes::ResourceAttributeManager,
    ) -> ObservabilityResult<Vec<u8>> {
        use opentelemetry_proto::tonic::{
            collector::trace::v1::ExportTraceServiceRequest,
            common::v1::{any_value, AnyValue, InstrumentationScope, KeyValue},
            resource::v1::Resource,
            trace::v1::{span::SpanKind, ResourceSpans, ScopeSpans, Span, Status},
        };
        use prost::Message;
        use web_time::SystemTime;

        fn hex_to_bytes(hex: &str) -> ObservabilityResult<Vec<u8>> {
            let hex = hex.trim();
            if hex.len() % 2 != 0 {
                return Err(ObservabilityError::transport(format!(
                    "Invalid hex length: {}",
                    hex.len()
                )));
            }
            let mut out = Vec::with_capacity(hex.len() / 2);
            let mut i = 0;
            while i < hex.len() {
                let b = u8::from_str_radix(&hex[i..i + 2], 16).map_err(|e| {
                    ObservabilityError::transport(format!("Invalid hex at {i}: {e}"))
                })?;
                out.push(b);
                i += 2;
            }
            Ok(out)
        }

        fn system_time_to_unix_nanos(t: SystemTime) -> ObservabilityResult<u64> {
            let d = t.duration_since(SystemTime::UNIX_EPOCH).map_err(|e| {
                ObservabilityError::transport(format!("SystemTime before UNIX_EPOCH: {e}"))
            })?;
            Ok(d.as_nanos() as u64)
        }

        fn kv_string(key: String, value: String) -> KeyValue {
            KeyValue {
                key,
                value: Some(AnyValue {
                    value: Some(any_value::Value::StringValue(value)),
                }),
            }
        }

        let now = SystemTime::now();

        let otel_spans = spans
            .into_iter()
            .map(|span| {
                let trace_id = hex_to_bytes(&span.trace_id)?;
                if trace_id.len() != 16 {
                    return Err(ObservabilityError::transport(format!(
                        "Invalid trace_id length: {} (expected 16 bytes)",
                        trace_id.len()
                    )));
                }
                let span_id = hex_to_bytes(&span.span_id)?;
                if span_id.len() != 8 {
                    return Err(ObservabilityError::transport(format!(
                        "Invalid span_id length: {} (expected 8 bytes)",
                        span_id.len()
                    )));
                }
                let parent_span_id = if let Some(psid) = span.parent_span_id.as_deref() {
                    let b = hex_to_bytes(psid)?;
                    if b.len() != 8 {
                        return Err(ObservabilityError::transport(format!(
                            "Invalid parent_span_id length: {} (expected 8 bytes)",
                            b.len()
                        )));
                    }
                    b
                } else {
                    Vec::new()
                };

                let start_system_time = now.checked_sub(span.start_time.elapsed()).unwrap_or(now);
                let start_time_unix_nano = system_time_to_unix_nanos(start_system_time)?;

                let end_system_time = span
                    .end_time
                    .and_then(|end| now.checked_sub(end.elapsed()))
                    .unwrap_or(start_system_time);
                let end_time_unix_nano = system_time_to_unix_nanos(end_system_time)?;

                let attributes = span
                    .attributes
                    .into_iter()
                    .map(|(k, v)| kv_string(k, v))
                    .collect::<Vec<_>>();

                let events = span
                    .events
                    .into_iter()
                    .map(|event| {
                        let event_system_time =
                            now.checked_sub(event.timestamp.elapsed()).unwrap_or(now);
                        let time_unix_nano = system_time_to_unix_nanos(event_system_time)?;
                        let attributes = event
                            .attributes
                            .into_iter()
                            .map(|(k, v)| kv_string(k, v))
                            .collect::<Vec<_>>();
                        Ok(opentelemetry_proto::tonic::trace::v1::span::Event {
                            time_unix_nano,
                            name: event.name,
                            attributes,
                            dropped_attributes_count: 0,
                        })
                    })
                    .collect::<ObservabilityResult<Vec<_>>>()?;

                let status_code = match span.status {
                    SpanStatus::Ok => opentelemetry_proto::tonic::trace::v1::status::StatusCode::Ok,
                    SpanStatus::Error | SpanStatus::Cancelled => {
                        opentelemetry_proto::tonic::trace::v1::status::StatusCode::Error
                    }
                } as i32;

                Ok(Span {
                    trace_id,
                    span_id,
                    parent_span_id,
                    name: span.name,
                    kind: SpanKind::Client as i32,
                    start_time_unix_nano,
                    end_time_unix_nano,
                    attributes,
                    dropped_attributes_count: 0,
                    events,
                    dropped_events_count: 0,
                    links: Vec::new(),
                    dropped_links_count: 0,
                    status: Some(Status {
                        message: String::new(),
                        code: status_code,
                    }),
                    trace_state: String::new(),
                })
            })
            .collect::<ObservabilityResult<Vec<_>>>()?;

        let resource_attrs = resource_manager
            .get_all_attributes()
            .into_iter()
            .map(|(k, v)| kv_string(k, v))
            .collect::<Vec<_>>();

        let req = ExportTraceServiceRequest {
            resource_spans: vec![ResourceSpans {
                resource: Some(Resource {
                    attributes: resource_attrs,
                    dropped_attributes_count: 0,
                }),
                scope_spans: vec![ScopeSpans {
                    scope: Some(InstrumentationScope {
                        name: "otel".to_string(),
                        version: crate::VERSION.to_string(),
                        attributes: Vec::new(),
                        dropped_attributes_count: 0,
                    }),
                    spans: otel_spans,
                    schema_url: String::new(),
                }],
                schema_url: String::new(),
            }],
        };

        Ok(req.encode_to_vec())
    }

    /// Create OTLP spans payload according to OpenTelemetry specification
    #[cfg(feature = "structured-logging")]
    #[allow(dead_code)]
    fn create_otlp_spans_payload(
        &self,
        spans: Vec<OtelSpanData>,
        resource_manager: &crate::resource_attributes::ResourceAttributeManager,
    ) -> ObservabilityResult<serde_json::Value> {
        use base64::{engine::general_purpose::STANDARD, Engine as _};
        use serde_json::{json, Value};
        use web_time::SystemTime;

        fn hex_to_bytes(hex: &str) -> Option<Vec<u8>> {
            let hex = hex.trim();
            if hex.len() % 2 != 0 {
                return None;
            }
            let mut out = Vec::with_capacity(hex.len() / 2);
            let mut i = 0;
            while i < hex.len() {
                let b = u8::from_str_radix(&hex[i..i + 2], 16).ok()?;
                out.push(b);
                i += 2;
            }
            Some(out)
        }

        fn hex_to_b64(hex: &str, expected_len: usize) -> Option<String> {
            let bytes = hex_to_bytes(hex)?;
            if bytes.len() != expected_len {
                return None;
            }
            Some(STANDARD.encode(bytes))
        }

        fn system_time_to_unix_nanos(t: SystemTime) -> ObservabilityResult<u64> {
            let d = t.duration_since(SystemTime::UNIX_EPOCH).map_err(|e| {
                ObservabilityError::transport(format!("SystemTime before UNIX_EPOCH: {e}"))
            })?;
            Ok(d.as_nanos() as u64)
        }

        // `Instant` can't be converted to Unix time directly. We approximate by anchoring
        // it to "now" at export time: \(event_time ≈ now - event_instant.elapsed()\).
        let now = SystemTime::now();

        let resource_spans = spans
            .into_iter()
            .map(|span| {
                // OTLP/HTTP JSON uses Protobuf JSON mapping:
                // - bytes fields are base64 (NOT hex)
                // - prefer `scopeSpans` over legacy `instrumentationLibrarySpans`
                let trace_id = hex_to_b64(&span.trace_id, 16).unwrap_or_default();
                let span_id = hex_to_b64(&span.span_id, 8).unwrap_or_default();
                let parent_span_id = span
                    .parent_span_id
                    .as_deref()
                    .and_then(|s| hex_to_b64(s, 8));

                let start_system_time = now.checked_sub(span.start_time.elapsed()).unwrap_or(now);
                let start_time_unix_nano =
                    system_time_to_unix_nanos(start_system_time).unwrap_or(0);

                let end_system_time = span
                    .end_time
                    .and_then(|end| now.checked_sub(end.elapsed()))
                    .unwrap_or(start_system_time);
                let end_time_unix_nano =
                    system_time_to_unix_nanos(end_system_time).unwrap_or(start_time_unix_nano);

                let attributes: Vec<Value> = span
                    .attributes
                    .into_iter()
                    .map(|(key, value)| {
                        json!({
                            "key": key,
                            "value": {
                                "stringValue": value
                            }
                        })
                    })
                    .collect();

                let events: Vec<Value> = span
                    .events
                    .into_iter()
                    .map(|event| {
                        let event_attributes: Vec<Value> = event
                            .attributes
                            .into_iter()
                            .map(|(key, value)| {
                                json!({
                                    "key": key,
                                    "value": {
                                        "stringValue": value
                                    }
                                })
                            })
                            .collect();

                        let event_system_time =
                            now.checked_sub(event.timestamp.elapsed()).unwrap_or(now);
                        let event_time_unix_nano =
                            system_time_to_unix_nanos(event_system_time).unwrap_or(0);

                        json!({
                            "timeUnixNano": event_time_unix_nano.to_string(),
                            "name": event.name,
                            "attributes": event_attributes
                        })
                    })
                    .collect();

                let mut span_obj = serde_json::Map::<String, Value>::new();
                span_obj.insert("traceId".to_string(), Value::String(trace_id));
                span_obj.insert("spanId".to_string(), Value::String(span_id));
                if let Some(psid) = parent_span_id {
                    span_obj.insert("parentSpanId".to_string(), Value::String(psid));
                }
                span_obj.insert("name".to_string(), Value::String(span.name));
                // Protobuf JSON mapping encodes enums as strings by default.
                span_obj.insert(
                    "kind".to_string(),
                    Value::String("SPAN_KIND_CLIENT".to_string()),
                );
                span_obj.insert(
                    "startTimeUnixNano".to_string(),
                    Value::String(start_time_unix_nano.to_string()),
                );
                span_obj.insert(
                    "endTimeUnixNano".to_string(),
                    Value::String(end_time_unix_nano.to_string()),
                );
                span_obj.insert("attributes".to_string(), Value::Array(attributes));
                span_obj.insert("events".to_string(), Value::Array(events));
                span_obj.insert(
                    "status".to_string(),
                    json!({
                        "code": match span.status {
                            SpanStatus::Ok => "STATUS_CODE_OK",
                            SpanStatus::Error => "STATUS_CODE_ERROR",
                            // OTLP has no "cancelled" status code; treat as error.
                            SpanStatus::Cancelled => "STATUS_CODE_ERROR",
                        }
                    }),
                );
                Value::Object(span_obj)
            })
            .collect::<Vec<_>>();

        let resource_attributes = resource_manager.get_all_attributes();
        let resource_attrs: Vec<Value> = resource_attributes
            .into_iter()
            .map(|(key, value)| {
                json!({
                    "key": key,
                    "value": {
                        "stringValue": value
                    }
                })
            })
            .collect();

        Ok(json!({
            "resourceSpans": [{
                "resource": {
                    "attributes": resource_attrs
                },
                "scopeSpans": [{
                    "scope": {
                        "name": "otel",
                        "version": crate::VERSION
                    },
                    "spans": resource_spans
                }],
            }]
        }))
    }

    /// Create OTLP metrics payload according to OpenTelemetry specification
    #[cfg(feature = "structured-logging")]
    fn create_otlp_metrics_payload(
        &self,
        metrics: Vec<MetricData>,
        resource_manager: &crate::resource_attributes::ResourceAttributeManager,
    ) -> ObservabilityResult<serde_json::Value> {
        use serde_json::{json, Value};
        use web_time::SystemTime;

        fn system_time_to_unix_nanos(t: SystemTime) -> ObservabilityResult<u64> {
            let d = t.duration_since(SystemTime::UNIX_EPOCH).map_err(|e| {
                ObservabilityError::transport(format!("SystemTime before UNIX_EPOCH: {e}"))
            })?;
            Ok(d.as_nanos() as u64)
        }

        fn resource_attributes_json(
            resource_manager: &crate::resource_attributes::ResourceAttributeManager,
        ) -> Vec<Value> {
            resource_manager
                .get_all_attributes()
                .into_iter()
                .map(|(key, value)| {
                    json!({
                        "key": key,
                        "value": {
                            "stringValue": value
                        }
                    })
                })
                .collect()
        }

        let now = SystemTime::now();
        let resource_attrs = resource_attributes_json(resource_manager);

        let resource_metrics = metrics
            .into_iter()
            .map(|metric| {
                let labels: Vec<Value> = metric
                    .labels
                    .into_iter()
                    .map(|(key, value)| {
                        json!({
                            "key": key,
                            "value": {
                                "stringValue": value
                            }
                        })
                    })
                    .collect();

                let point_time = now.checked_sub(metric.timestamp.elapsed()).unwrap_or(now);
                let time_unix_nano = system_time_to_unix_nanos(point_time).unwrap_or(0);

                let mut metric_json = json!({
                    "name": metric.name,
                    "description": metric.description.unwrap_or_default(),
                    "unit": metric.unit.unwrap_or_default(),
                });

                match metric.kind {
                    MetricKind::Counter => {
                        metric_json["sum"] = json!({
                            "aggregationTemporality": "AGGREGATION_TEMPORALITY_CUMULATIVE",
                            "isMonotonic": true,
                            "dataPoints": [{
                                "asDouble": metric.value,
                                "timeUnixNano": time_unix_nano.to_string(),
                                "attributes": labels
                            }]
                        });
                    }
                    MetricKind::Histogram => {
                        metric_json["histogram"] = json!({
                            "aggregationTemporality": "AGGREGATION_TEMPORALITY_CUMULATIVE",
                            "dataPoints": [{
                                "count": "1",
                                "sum": metric.value,
                                "bucketCounts": ["1"],
                                "explicitBounds": [],
                                "timeUnixNano": time_unix_nano.to_string(),
                                "attributes": labels
                            }]
                        });
                    }
                    MetricKind::Gauge => {
                        metric_json["gauge"] = json!({
                            "dataPoints": [{
                                "asDouble": metric.value,
                                "timeUnixNano": time_unix_nano.to_string(),
                                "attributes": labels
                            }]
                        });
                    }
                }

                metric_json
            })
            .collect::<Vec<_>>();

        Ok(json!({
            "resourceMetrics": [{
                "resource": {
                    "attributes": resource_attrs
                },
                "instrumentationLibraryMetrics": [{
                    "instrumentationLibrary": {
                        "name": "otel",
                        "version": crate::VERSION
                    },
                    "metrics": resource_metrics
                }]
            }]
        }))
    }

    /// Create OTLP logs payload according to OpenTelemetry specification
    #[cfg(feature = "structured-logging")]
    fn create_otlp_logs_payload(
        &self,
        logs: Vec<LogData>,
        resource_manager: &crate::resource_attributes::ResourceAttributeManager,
    ) -> ObservabilityResult<serde_json::Value> {
        use base64::{engine::general_purpose::STANDARD, Engine as _};
        use serde_json::{json, Value};
        use web_time::SystemTime;

        fn hex_to_bytes(hex: &str) -> Option<Vec<u8>> {
            let hex = hex.trim();
            if hex.len() % 2 != 0 {
                return None;
            }
            let mut out = Vec::with_capacity(hex.len() / 2);
            let mut i = 0;
            while i < hex.len() {
                let b = u8::from_str_radix(&hex[i..i + 2], 16).ok()?;
                out.push(b);
                i += 2;
            }
            Some(out)
        }

        fn hex_to_b64(hex: &str, expected_len: usize) -> Option<String> {
            let bytes = hex_to_bytes(hex)?;
            if bytes.len() != expected_len {
                return None;
            }
            Some(STANDARD.encode(bytes))
        }

        fn system_time_to_unix_nanos(t: SystemTime) -> ObservabilityResult<u64> {
            let d = t.duration_since(SystemTime::UNIX_EPOCH).map_err(|e| {
                ObservabilityError::transport(format!("SystemTime before UNIX_EPOCH: {e}"))
            })?;
            Ok(d.as_nanos() as u64)
        }

        fn resource_attributes_json(
            resource_manager: &crate::resource_attributes::ResourceAttributeManager,
        ) -> Vec<Value> {
            resource_manager
                .get_all_attributes()
                .into_iter()
                .map(|(key, value)| {
                    json!({
                        "key": key,
                        "value": {
                            "stringValue": value
                        }
                    })
                })
                .collect()
        }

        let now = SystemTime::now();
        let resource_attrs = resource_attributes_json(resource_manager);

        let log_records = logs
            .into_iter()
            .map(|log| {
                let log_time = now.checked_sub(log.timestamp.elapsed()).unwrap_or(now);
                let time_unix_nano = system_time_to_unix_nanos(log_time).unwrap_or(0);
                let attributes: Vec<Value> = log
                    .attributes
                    .into_iter()
                    .map(|(key, value)| {
                        json!({
                            "key": key,
                            "value": {
                                "stringValue": value
                            }
                        })
                    })
                    .collect();

                let mut log_record = json!({
                    "timeUnixNano": time_unix_nano.to_string(),
                    "severityText": log.level,
                    "body": {
                        "stringValue": log.message
                    },
                    "attributes": attributes
                });

                if let Some(trace_id) = log
                    .trace_id
                    .as_deref()
                    .and_then(|trace_id| hex_to_b64(trace_id, 16))
                {
                    log_record["traceId"] = Value::String(trace_id);
                }

                if let Some(span_id) = log
                    .span_id
                    .as_deref()
                    .and_then(|span_id| hex_to_b64(span_id, 8))
                {
                    log_record["spanId"] = Value::String(span_id);
                }

                log_record
            })
            .collect::<Vec<_>>();

        Ok(json!({
            "resourceLogs": [{
                "resource": {
                    "attributes": resource_attrs
                },
                "instrumentationLibraryLogs": [{
                    "instrumentationLibrary": {
                        "name": "otel",
                        "version": crate::VERSION
                    },
                    "logs": log_records
                }]
            }]
        }))
    }

    /// Health check endpoint for the OTLP collector
    pub async fn health_check(&self) -> ObservabilityResult<bool> {
        let url = format!("{}/health", self.endpoint);
        match self.client.get(&url).send().await {
            Ok(response) => Ok(response.status().is_success()),
            Err(_) => Ok(false), // Collector might not have health endpoint
        }
    }

    /// Get collector information and capabilities
    #[cfg(feature = "structured-logging")]
    pub async fn get_collector_info(&self) -> ObservabilityResult<serde_json::Value> {
        let url = format!("{}/info", self.endpoint);
        let response = self.client.get(&url).send().await.map_err(|e| {
            ObservabilityError::transport(format!("Failed to get collector info: {}", e))
        })?;

        if !response.status().is_success() {
            return Err(ObservabilityError::transport(
                "Collector info endpoint not available".to_string(),
            ));
        }

        let info: serde_json::Value = response.json().await.map_err(|e| {
            ObservabilityError::transport(format!("Failed to parse collector info: {}", e))
        })?;

        Ok(info)
    }
}

/// Metric data for export with proper OpenTelemetry structure
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricKind {
    Counter,
    Histogram,
    Gauge,
}

impl MetricKind {
    pub fn from_metric_name(name: &str) -> Self {
        let lower = name.to_ascii_lowercase();
        if lower.ends_with("_total") || lower.ends_with("_count") || lower.ends_with("_counter") {
            Self::Counter
        } else if lower.ends_with("_ms")
            || lower.ends_with("_seconds")
            || lower.contains("latency")
            || lower.contains("duration")
        {
            Self::Histogram
        } else {
            Self::Gauge
        }
    }
}

/// Metric data for export with proper OpenTelemetry structure
#[derive(Debug, Clone)]
pub struct MetricData {
    pub name: String,
    pub value: f64,
    pub kind: MetricKind,
    pub timestamp: Instant,
    pub labels: HashMap<String, String>,
    pub unit: Option<String>,
    pub description: Option<String>,
}

impl MetricData {
    /// Create a new metric data entry
    pub fn new(name: impl Into<String>, value: f64) -> Self {
        let name = name.into();
        Self {
            kind: MetricKind::from_metric_name(&name),
            name,
            value,
            timestamp: Instant::now(),
            labels: HashMap::new(),
            unit: None,
            description: None,
        }
    }

    /// Override the inferred metric kind.
    pub fn with_kind(mut self, kind: MetricKind) -> Self {
        self.kind = kind;
        self
    }

    /// Add a label to the metric
    pub fn with_label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.labels.insert(key.into(), value.into());
        self
    }

    /// Set the unit of measurement
    pub fn with_unit(mut self, unit: impl Into<String>) -> Self {
        self.unit = Some(unit.into());
        self
    }

    /// Set the description
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
}

/// Log data for export with proper OpenTelemetry structure
#[derive(Debug, Clone)]
pub struct LogData {
    pub message: String,
    pub level: String,
    pub timestamp: web_time::Instant,
    pub trace_id: Option<String>,
    pub span_id: Option<String>,
    pub attributes: HashMap<String, String>,
}

impl LogData {
    /// Create a new log data entry
    pub fn new(level: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            level: level.into(),
            timestamp: web_time::Instant::now(),
            trace_id: None,
            span_id: None,
            attributes: HashMap::new(),
        }
    }

    /// Add an attribute to the log entry
    pub fn with_attribute(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.attributes.insert(key.into(), value.into());
        self
    }

    /// Attach canonical trace correlation to the log entry.
    pub fn with_trace_context(
        mut self,
        trace_id: impl Into<String>,
        span_id: impl Into<String>,
    ) -> Self {
        self.trace_id = Some(trace_id.into());
        self.span_id = Some(span_id.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use observability_core::traits::SpanStatus;

    #[test]
    fn test_metric_data_builder() {
        let metric = MetricData::new("test_metric_total", 42.0)
            .with_label("env", "test")
            .with_unit("count")
            .with_description("A test metric");

        assert_eq!(metric.name, "test_metric_total");
        assert_eq!(metric.value, 42.0);
        assert_eq!(metric.kind, MetricKind::Counter);
        assert_eq!(metric.labels.get("env"), Some(&"test".to_string()));
        assert_eq!(metric.unit, Some("count".to_string()));
        assert_eq!(metric.description, Some("A test metric".to_string()));
    }

    #[test]
    fn test_log_data_builder() {
        let log = LogData::new("INFO", "Test message")
            .with_trace_context(
                "0af7651916cd43dd8448eb211c80319c",
                "b7ad6b7169203331",
            )
            .with_attribute("component", "test");

        assert_eq!(log.level, "INFO");
        assert_eq!(log.message, "Test message");
        assert_eq!(
            log.trace_id.as_deref(),
            Some("0af7651916cd43dd8448eb211c80319c")
        );
        assert_eq!(log.span_id.as_deref(), Some("b7ad6b7169203331"));
        assert_eq!(log.attributes.get("component"), Some(&"test".to_string()));
    }

    #[test]
    fn test_create_otlp_metrics_payload_preserves_metric_kinds() {
        let client = CollectorClient::new_sync("http://localhost:4318", Duration::from_secs(1))
            .expect("client");
        let resource_manager = crate::resource_attributes::ResourceAttributeManager::new(
            "metric-service",
            "1.2.3",
            "test-ns",
            HashMap::new(),
        );

        let payload = client
            .create_otlp_metrics_payload(
                vec![
                    MetricData::new("requests_total", 3.0),
                    MetricData::new("request_latency_ms", 125.0),
                    MetricData::new("queue_depth", 7.0),
                ],
                &resource_manager,
            )
            .expect("payload");

        let metrics = payload["resourceMetrics"][0]["instrumentationLibraryMetrics"][0]["metrics"]
            .as_array()
            .expect("metrics array");
        assert!(metrics[0].get("sum").is_some(), "counter should export as sum");
        assert_eq!(
            metrics[0]["sum"]["isMonotonic"].as_bool(),
            Some(true),
            "counter should be monotonic"
        );
        assert!(
            metrics[1].get("histogram").is_some(),
            "latency metric should export as histogram"
        );
        assert!(
            metrics[2].get("gauge").is_some(),
            "generic metric should export as gauge"
        );

        let resource_attrs = payload["resourceMetrics"][0]["resource"]["attributes"]
            .as_array()
            .expect("resource attrs");
        assert!(resource_attrs.iter().any(|attr| {
            attr["key"].as_str() == Some("service.name")
                && attr["value"]["stringValue"].as_str() == Some("metric-service")
        }));
    }

    #[test]
    fn test_create_otlp_logs_payload_includes_resource_attrs_and_trace_context() {
        let client = CollectorClient::new_sync("http://localhost:4318", Duration::from_secs(1))
            .expect("client");
        let resource_manager = crate::resource_attributes::ResourceAttributeManager::new(
            "log-service",
            "9.9.9",
            "test-ns",
            HashMap::new(),
        );

        let payload = client
            .create_otlp_logs_payload(
                vec![LogData::new("INFO", "hello")
                    .with_trace_context(
                        "0af7651916cd43dd8448eb211c80319c",
                        "b7ad6b7169203331",
                    )
                    .with_attribute("trace_id", "0af7651916cd43dd8448eb211c80319c")
                    .with_attribute("span_id", "b7ad6b7169203331")],
                &resource_manager,
            )
            .expect("payload");

        let log = &payload["resourceLogs"][0]["instrumentationLibraryLogs"][0]["logs"][0];
        assert!(log.get("traceId").is_some(), "trace context should be attached");
        assert!(log.get("spanId").is_some(), "span context should be attached");
        assert!(log["attributes"]
            .as_array()
            .expect("attributes")
            .iter()
            .any(|attr| attr["key"].as_str() == Some("trace_id")));

        let resource_attrs = payload["resourceLogs"][0]["resource"]["attributes"]
            .as_array()
            .expect("resource attrs");
        assert!(resource_attrs.iter().any(|attr| {
            attr["key"].as_str() == Some("service.name")
                && attr["value"]["stringValue"].as_str() == Some("log-service")
        }));
    }

    #[test]
    fn test_otel_span_data_creation() {
        let span = OtelSpanData {
            span_id: "test-span".to_string(),
            trace_id: "test-trace".to_string(),
            parent_span_id: None,
            name: "test-operation".to_string(),
            start_time: web_time::Instant::now(),
            end_time: None,
            status: SpanStatus::Ok,
            attributes: std::collections::HashMap::new(),
            events: Vec::new(),
        };

        assert_eq!(span.name, "test-operation");
        assert_eq!(span.status as u8, SpanStatus::Ok as u8);
    }

    #[test]
    fn test_otlp_protobuf_trace_export_request_encodes_and_decodes() {
        use opentelemetry_proto::tonic::collector::trace::v1::ExportTraceServiceRequest;
        use prost::Message;

        let client = CollectorClient::new_sync("http://localhost:4318", Duration::from_secs(1))
            .expect("client");

        // 16-byte trace id and 8-byte span id, as hex strings.
        let trace_id = "000102030405060708090a0b0c0d0e0f".to_string();
        let span_id = "0001020304050607".to_string();

        let span = OtelSpanData {
            span_id,
            trace_id,
            parent_span_id: None,
            name: "test-operation".to_string(),
            start_time: web_time::Instant::now(),
            end_time: Some(web_time::Instant::now()),
            status: SpanStatus::Ok,
            attributes: HashMap::from([("k".to_string(), "v".to_string())]),
            events: vec![SpanEvent {
                name: "evt".to_string(),
                timestamp: web_time::Instant::now(),
                attributes: HashMap::from([("ek".to_string(), "ev".to_string())]),
            }],
        };

        let resource_manager = crate::resource_attributes::ResourceAttributeManager::new(
            "test-service",
            "0.0.0-test",
            "test-ns",
            HashMap::new(),
        );
        let body = client
            .create_otlp_spans_request_protobuf(vec![span], &resource_manager)
            .expect("protobuf body");

        let decoded = ExportTraceServiceRequest::decode(body.as_slice()).expect("decode");
        assert_eq!(decoded.resource_spans.len(), 1);
        let rs = &decoded.resource_spans[0];
        assert_eq!(rs.scope_spans.len(), 1);
        let ss = &rs.scope_spans[0];
        assert_eq!(ss.spans.len(), 1);
        let s = &ss.spans[0];
        assert_eq!(s.trace_id.len(), 16);
        assert_eq!(s.span_id.len(), 8);
        assert_eq!(s.name, "test-operation");
        assert_eq!(s.attributes.len(), 1);
        assert_eq!(s.events.len(), 1);
        assert!(s.start_time_unix_nano > 0);
        assert!(s.end_time_unix_nano > 0);
    }

    #[test]
    fn test_build_otlp_traces_http_request_has_expected_headers_and_resource_attrs() {
        use opentelemetry_proto::tonic::collector::trace::v1::ExportTraceServiceRequest;
        use prost::Message;

        let client =
            CollectorClient::new_sync("http://example-collector:4318", Duration::from_secs(1))
                .expect("client");

        let trace_id = "000102030405060708090a0b0c0d0e0f".to_string();
        let span_id = "0001020304050607".to_string();

        let span = OtelSpanData {
            span_id,
            trace_id,
            parent_span_id: None,
            name: "test-operation".to_string(),
            start_time: web_time::Instant::now(),
            end_time: Some(web_time::Instant::now()),
            status: SpanStatus::Ok,
            attributes: HashMap::new(),
            events: Vec::new(),
        };

        let resource_manager = crate::resource_attributes::ResourceAttributeManager::new(
            "llm-coordinator",
            "0.1.0",
            "default",
            HashMap::new(),
        );

        let req = client
            .build_otlp_traces_export_http_request(vec![span], &resource_manager)
            .expect("otlp http request");

        assert!(req.url.ends_with("/v1/traces"));
        assert!(req
            .headers
            .iter()
            .any(|(k, v)| k.eq_ignore_ascii_case("content-type") && v == "application/x-protobuf"));

        let decoded = ExportTraceServiceRequest::decode(req.body.as_slice()).expect("decode");
        let rs = &decoded.resource_spans[0];
        let resource = rs.resource.as_ref().expect("resource");

        // Ensure OTel resource attrs are present.
        let mut found = HashMap::<String, String>::new();
        for kv in &resource.attributes {
            let key = kv.key.clone();
            let val = kv
                .value
                .as_ref()
                .and_then(|v| v.value.as_ref())
                .and_then(|v| match v {
                    opentelemetry_proto::tonic::common::v1::any_value::Value::StringValue(s) => {
                        Some(s.clone())
                    }
                    _ => None,
                })
                .unwrap_or_default();
            found.insert(key, val);
        }

        assert_eq!(
            found.get("service.name").map(String::as_str),
            Some("llm-coordinator")
        );
        assert_eq!(
            found.get("service.version").map(String::as_str),
            Some("0.1.0")
        );
        assert_eq!(
            found.get("service.namespace").map(String::as_str),
            Some("default")
        );
    }
}
