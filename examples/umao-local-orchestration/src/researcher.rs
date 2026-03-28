//! Researcher agent — receives a topic, returns a research summary.
//!
//! Runs a minimal A2A HTTP server on a configurable port.

use std::sync::Arc;
use a2a_protocol_core::data::{Message, MessageRole, Part};
use clap::Parser;
use tracing::info;

#[derive(Parser)]
struct Args {
    #[arg(long, default_value = "3001")]
    port: u16,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    let args = Args::parse();

    info!(port = args.port, "Starting researcher agent");

    let handler = Arc::new(|message: Message| {
        let text = message.get_text_content();
        let response_text = format!(
            "## Research Report\n\n\
             **Topic**: {}\n\n\
             **Findings**: After thorough analysis, the key points are:\n\
             1. The topic has significant implications for the field.\n\
             2. Multiple perspectives exist in the current literature.\n\
             3. Further investigation is recommended in specific sub-areas.\n\n\
             **Confidence**: 0.85",
            if text.len() > 200 { &text[..200] } else { &text }
        );
        Message::new(
            MessageRole::Agent,
            vec![Part::text(response_text)],
            "research-ctx".to_string(),
        )
    });

    let addr = format!("0.0.0.0:{}", args.port);
    info!(addr, "Researcher listening");

    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();

    // Minimal A2A-compatible HTTP server
    let handler = handler.clone();
    loop {
        let (stream, _) = listener.accept().await.unwrap();
        let handler = handler.clone();
        tokio::spawn(async move {
            handle_connection(stream, handler).await;
        });
    }
}

async fn handle_connection(
    stream: tokio::net::TcpStream,
    handler: Arc<dyn Fn(Message) -> Message + Send + Sync>,
) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let mut stream = stream;
    let mut buf = vec![0u8; 8192];
    let n = stream.read(&mut buf).await.unwrap_or(0);
    let request = String::from_utf8_lossy(&buf[..n]);

    if let Some(body_start) = request.find("\r\n\r\n") {
        let body = &request[body_start + 4..];
        if let Ok(rpc) = serde_json::from_str::<serde_json::Value>(body) {
            let method = rpc.get("method").and_then(|v| v.as_str()).unwrap_or("");
            let id = rpc.get("id").cloned().unwrap_or(serde_json::json!(null));

            let result = if method == "message/send" {
                let params = rpc.get("params").cloned().unwrap_or_default();
                let text = params
                    .get("message")
                    .and_then(|m| m.get("parts"))
                    .and_then(|p| p.as_array())
                    .and_then(|arr| arr.first())
                    .and_then(|p| p.get("text"))
                    .and_then(|t| t.as_str())
                    .unwrap_or("No input");

                let msg = Message::new(
                    MessageRole::User,
                    vec![Part::text(text.to_string())],
                    "ctx".to_string(),
                );
                let response = handler(msg);
                let response_text = response.get_text_content();
                serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": response_text
                })
            } else {
                serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": {"code": -32601, "message": "Method not found"}
                })
            };

            let body = serde_json::to_string(&result).unwrap();
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(response.as_bytes()).await;
        }
    }
}
