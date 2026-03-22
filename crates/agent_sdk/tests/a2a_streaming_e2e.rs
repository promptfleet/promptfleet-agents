#![cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]

use a2a_http_client::Client;
use a2a_protocol_core::data::{Message, MessageRole, Part, TaskState};
use a2a_protocol_core::streaming::StreamResponse;
use agent_sdk::{a2a::A2aServer, Agent};
use futures_util::StreamExt;
use std::time::Duration;

#[tokio::test]
async fn sdk_server_and_client_send_subscribe_roundtrip() {
    let agent = Agent::new_runtime("streaming-e2e-agent").expect("agent");
    let router = A2aServer::with_a2a_methods(agent)
        .expect("agent server")
        .build_router();

    let std_listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind listener");
    let addr = std_listener.local_addr().expect("listener addr");
    std_listener
        .set_nonblocking(true)
        .expect("set_nonblocking");
    let listener = tokio::net::TcpListener::from_std(std_listener).expect("tokio listener");

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let server_task = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx.await;
            })
            .await
            .expect("axum serve");
    });

    let client = Client::external(format!("http://{}/jsonrpc", addr));
    let message = Message::new(
        MessageRole::User,
        vec![Part::text("hello from e2e")],
        "ctx-e2e".to_string(),
    );

    let mut stream = client
        .send_subscribe("task-e2e", message)
        .await
        .expect("send_subscribe");

    let mut events = Vec::new();
    for _ in 0..24 {
        let next = tokio::time::timeout(Duration::from_secs(2), stream.next())
            .await
            .expect("timed out while waiting for SSE event");
        match next {
            Some(Ok(event)) => {
                let final_event = is_final(&event);
                events.push(event);
                if final_event {
                    break;
                }
            }
            Some(Err(err)) => panic!("SSE stream item error: {}", err),
            None => break,
        }
    }

    let _ = shutdown_tx.send(());
    let _ = server_task.await;

    assert!(!events.is_empty(), "expected at least one SSE event");
    assert!(
        matches!(
            events.first(),
            Some(StreamResponse::StatusUpdate(status)) if status.status.state == TaskState::Working
        ),
        "first event should be StatusUpdate(Working), got: {:?}",
        events.first()
    );
    assert!(
        events.last().map_or(false, |e| e.is_terminal()),
        "last event should be terminal, got: {:?}",
        events.last()
    );
}

fn is_final(event: &StreamResponse) -> bool {
    event.is_terminal()
}
