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

fn extract_topic(raw: &str) -> String {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(raw) {
        if let Some(t) = v.pointer("/__node_traits/task_prompt").and_then(|t| t.as_str()) {
            return t.to_string();
        }
        for key in ["task_spec", "task_prompt", "delegation_context"] {
            if let Some(t) = v.get(key).and_then(|t| t.as_str()) {
                return t.to_string();
            }
        }
    }
    let trimmed = raw.trim();
    if trimmed.len() > 120 {
        format!("{}...", &trimmed[..trimmed.floor_char_boundary(120)])
    } else {
        trimmed.to_string()
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    let args = Args::parse();

    info!(port = args.port, "Starting researcher agent");

    let handler = Arc::new(|message: Message| {
        let raw = message.get_text_content();
        let topic = extract_topic(&raw);
        let response_text = format!(
            "## Research Report: {topic}\n\n\
             **Key findings**:\n\
             1. {topic} has seen a 40% increase in academic publications since 2023.\n\
             2. Three dominant approaches exist: rule-based, learning-based, and hybrid.\n\
             3. The hybrid approach shows the best cost/performance trade-off in benchmarks.\n\n\
             **Open questions**: Scalability beyond 10k concurrent users remains unproven.\n\n\
             **Sources**: 12 papers, 3 industry reports | **Confidence**: 0.85",
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

            let result = if method == "message/send" || method == "SendMessage" {
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
