//! Synthesizer agent — receives multiple inputs, returns a synthesis.
//!
//! Runs a minimal A2A HTTP server on a configurable port.

use a2a_protocol_core::data::{Message, MessageRole, Part};
use clap::Parser;
use std::sync::Arc;
use tracing::info;

#[derive(Parser)]
struct Args {
    #[arg(long, default_value = "3002")]
    port: u16,
}

fn extract_upstream_summary(raw: &str) -> String {
    fn truncate(s: &str, max: usize) -> String {
        let t = s.trim();
        if t.len() > max {
            format!("{}...", &t[..t.floor_char_boundary(max)])
        } else {
            t.to_string()
        }
    }

    if let Ok(v) = serde_json::from_str::<serde_json::Value>(raw) {
        for key in ["task_results", "task_spec", "delegation_context"] {
            match v.get(key) {
                Some(serde_json::Value::String(s)) => return truncate(s, 200),
                Some(val) if !val.is_null() => {
                    return truncate(&serde_json::to_string(val).unwrap_or_default(), 200);
                }
                _ => {}
            }
        }
        if let Some(t) = v
            .pointer("/__node_traits/task_prompt")
            .and_then(|t| t.as_str())
        {
            return truncate(t, 200);
        }
    }
    truncate(raw, 200)
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    let args = Args::parse();

    info!(port = args.port, "Starting synthesizer agent");

    let handler = Arc::new(|message: Message| {
        let raw = message.get_text_content();
        let upstream = extract_upstream_summary(&raw);
        let response_text = format!(
            "## Synthesis\n\n\
             **Upstream input**: {upstream}\n\n\
             **Conclusions**:\n\
             - The hybrid approach is the strongest candidate for production deployment.\n\
             - Scalability gaps should be addressed with load-testing before launch.\n\
             - Budget allocation: 60% implementation, 25% testing, 15% documentation.\n\n\
             **Recommended next step**: Build a proof-of-concept targeting the hybrid approach.\n\n\
             **Confidence**: 0.92",
        );
        Message::new(
            MessageRole::Agent,
            vec![Part::text(response_text)],
            "synth-ctx".to_string(),
        )
    });

    let addr = format!("0.0.0.0:{}", args.port);
    info!(addr, "Synthesizer listening");

    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();

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
