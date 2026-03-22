#![cfg(not(target_arch = "wasm32"))]

use axum::{
    http::{header::CONTENT_TYPE, HeaderValue, StatusCode},
    response::Response,
    routing::post,
    Router,
};
use llm_client::{
    model_client::{ClientError, HttpModelClient},
    ClientConfig,
};
use protocol_transport_core::TransportError;
use serde_json::json;
use tokio::{net::TcpListener, task::JoinHandle};

async fn spawn_server(router: Router) -> (String, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind test listener");
    let addr = listener.local_addr().expect("listener local_addr");
    let handle = tokio::spawn(async move {
        axum::serve(listener, router)
            .await
            .expect("serve test router");
    });
    (format!("http://{addr}"), handle)
}

fn build_client(base_url: String) -> HttpModelClient {
    HttpModelClient::new(ClientConfig {
        base_url,
        ..ClientConfig::default()
    })
}

async fn json_http_error_handler() -> Response<String> {
    let mut response = Response::new(r#"{"error":"rate_limited","retry_after":30}"#.to_string());
    *response.status_mut() = StatusCode::TOO_MANY_REQUESTS;
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    response
        .headers_mut()
        .insert("x-ratelimit-reset", HeaderValue::from_static("1735689600"));
    response
}

async fn invalid_json_handler() -> Response<String> {
    let mut response = Response::new("not-json".to_string());
    *response.status_mut() = StatusCode::OK;
    response
}

async fn sse_http_error_handler() -> Response<String> {
    let mut response = Response::new("upstream unavailable".to_string());
    *response.status_mut() = StatusCode::SERVICE_UNAVAILABLE;
    response
}

#[tokio::test]
async fn test_post_json_http_error_maps_transport_and_preserves_payload() {
    let app = Router::new().route("/http-error", post(json_http_error_handler));
    let (base_url, server) = spawn_server(app).await;
    let client = build_client(base_url);

    let err = client
        .post_json("/http-error", json!({"hello":"world"}))
        .await
        .expect_err("HTTP 429 must map to ClientError::Transport");
    server.abort();

    match err {
        ClientError::Transport(TransportError::Http {
            status,
            message,
            body,
            headers,
        }) => {
            assert_eq!(status, 429);
            assert!(
                message.contains("429"),
                "expected normalized HTTP status in message, got {message}"
            );
            let body = body.expect("HTTP body should be preserved");
            let body_text = String::from_utf8_lossy(&body);
            assert!(
                body_text.contains("rate_limited"),
                "expected error payload to be preserved, got {body_text}"
            );

            let headers = headers.expect("HTTP headers should be preserved");
            assert!(
                headers
                    .iter()
                    .any(|(k, v)| k.eq_ignore_ascii_case("x-ratelimit-reset") && v == "1735689600"),
                "expected x-ratelimit-reset header to be preserved, got {headers:?}"
            );
        }
        other => panic!("expected HTTP transport error, got {other:?}"),
    }
}

#[tokio::test]
async fn test_post_json_invalid_json_maps_to_serialization_error() {
    let app = Router::new().route("/invalid-json", post(invalid_json_handler));
    let (base_url, server) = spawn_server(app).await;
    let client = build_client(base_url);

    let err = client
        .post_json("/invalid-json", json!({"probe":"value"}))
        .await
        .expect_err("invalid JSON body must fail");
    server.abort();

    assert!(
        matches!(err, ClientError::Serialization(_)),
        "expected serialization error, got {err:?}"
    );
}

#[tokio::test]
async fn test_post_sse_http_error_maps_transport_with_body() {
    let app = Router::new().route("/sse-error", post(sse_http_error_handler));
    let (base_url, server) = spawn_server(app).await;
    let client = build_client(base_url);

    let err = client
        .post_sse("/sse-error", json!({"stream": true}))
        .await
        .expect_err("HTTP 503 must map to ClientError::Transport");
    server.abort();

    match err {
        ClientError::Transport(TransportError::Http {
            status,
            message,
            body,
            headers,
        }) => {
            assert_eq!(status, 503);
            assert_eq!(message, "HTTP 503 error");
            assert!(
                String::from_utf8_lossy(&body.expect("body should be preserved"))
                    .contains("upstream unavailable")
            );
            assert!(
                headers.is_none(),
                "post_sse currently normalizes HTTP errors with no headers"
            );
        }
        other => panic!("expected HTTP transport error, got {other:?}"),
    }
}
