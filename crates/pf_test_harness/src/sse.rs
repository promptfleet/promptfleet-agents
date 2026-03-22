use std::fmt::Display;
use std::time::Duration;

use bytes::Bytes;
use http::{HeaderMap, Response, StatusCode};
use http_body::Body;
use http_body_util::BodyExt;
use serde_json::Value;

#[derive(Debug, Clone)]
pub struct SseFrame {
    pub event: String,
    pub id: Option<String>,
    pub data_raw: String,
    pub data_json: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct SseCapture {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub raw_body: String,
    pub frames: Vec<SseFrame>,
}

impl SseCapture {
    pub fn first(&self) -> Option<&SseFrame> {
        self.frames.first()
    }

    pub fn last(&self) -> Option<&SseFrame> {
        self.frames.last()
    }

    pub fn event_names(&self) -> Vec<&str> {
        self.frames.iter().map(|f| f.event.as_str()).collect()
    }

    pub fn assert_sequence(&self, expected: &[&str]) {
        let actual: Vec<&str> = self.event_names();
        assert_eq!(actual, expected, "unexpected SSE event sequence");
    }

    pub fn assert_event_names(&self, expected: &[&str]) {
        self.assert_sequence(expected);
    }

    pub fn assert_json_path_eq(&self, frame_index: usize, path: &str, expected: Value) {
        let frame = self
            .frames
            .get(frame_index)
            .unwrap_or_else(|| panic!("missing frame at index {frame_index}"));
        let json = frame
            .data_json
            .as_ref()
            .unwrap_or_else(|| panic!("frame {frame_index} has no JSON payload"));

        let actual = json_path_get(json, path)
            .unwrap_or_else(|| panic!("JSON path '{path}' not found in frame {frame_index}"));
        assert_eq!(actual, &expected, "JSON path assertion failed for '{path}'");
    }

    pub fn assert_terminal(&self, expected_event: &str) {
        let last = self
            .last()
            .unwrap_or_else(|| panic!("no SSE frames captured"));
        assert_eq!(last.event, expected_event, "unexpected terminal SSE event");
    }
}

pub struct SseCollector<B> {
    response: Response<B>,
    timeout: Duration,
}

impl<B> SseCollector<B>
where
    B: Body<Data = Bytes> + Send + 'static,
    B::Error: Display,
{
    pub fn from_response(response: Response<B>) -> Self {
        Self {
            response,
            timeout: Duration::from_secs(5),
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub async fn collect_all(self) -> Result<SseCapture, String> {
        let (parts, body) = self.response.into_parts();
        let collected = tokio::time::timeout(self.timeout, body.collect())
            .await
            .map_err(|_| format!("timed out collecting SSE response after {:?}", self.timeout))?
            .map_err(|e| format!("failed to collect SSE response body: {e}"))?;

        let raw_body = String::from_utf8_lossy(&collected.to_bytes()).to_string();
        let frames = parse_sse_frames(&raw_body);

        Ok(SseCapture {
            status: parts.status,
            headers: parts.headers,
            raw_body,
            frames,
        })
    }
}

pub fn parse_sse_frames(body: &str) -> Vec<SseFrame> {
    let mut frames = Vec::new();

    let mut current_event = String::new();
    let mut current_id: Option<String> = None;
    let mut current_data_lines: Vec<String> = Vec::new();

    let flush = |frames: &mut Vec<SseFrame>,
                 current_event: &mut String,
                 current_id: &mut Option<String>,
                 current_data_lines: &mut Vec<String>| {
        if current_event.is_empty() && current_data_lines.is_empty() && current_id.is_none() {
            return;
        }

        let event = if current_event.is_empty() {
            "message".to_string()
        } else {
            current_event.clone()
        };
        let data_raw = current_data_lines.join("\n");
        let data_json = serde_json::from_str::<Value>(&data_raw).ok();

        frames.push(SseFrame {
            event,
            id: current_id.clone(),
            data_raw,
            data_json,
        });

        current_event.clear();
        *current_id = None;
        current_data_lines.clear();
    };

    for line in body.lines() {
        if line.is_empty() {
            flush(
                &mut frames,
                &mut current_event,
                &mut current_id,
                &mut current_data_lines,
            );
            continue;
        }

        if line.starts_with(':') {
            continue;
        }

        if let Some(event) = line.strip_prefix("event:") {
            current_event = event.trim().to_string();
            continue;
        }

        if let Some(id) = line.strip_prefix("id:") {
            current_id = Some(id.trim().to_string());
            continue;
        }

        if let Some(data) = line.strip_prefix("data:") {
            current_data_lines.push(data.trim_start().to_string());
        }
    }

    flush(
        &mut frames,
        &mut current_event,
        &mut current_id,
        &mut current_data_lines,
    );

    frames
}

fn json_path_get<'a>(root: &'a Value, path: &str) -> Option<&'a Value> {
    let mut current = root;
    for segment in path.split('.') {
        if segment.is_empty() {
            continue;
        }

        if let Ok(index) = segment.parse::<usize>() {
            current = current.as_array()?.get(index)?;
        } else {
            current = current.get(segment)?;
        }
    }
    Some(current)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_sse_frames_multiline_keepalive_and_trailing() {
        let body = ":keepalive\n\
                    event: task_status_update\n\
                    data: {\"jsonrpc\":\"2.0\",\n\
                    data: \"id\":\"1\"}\n\n\
                    event: run_event\n\
                    id: 42\n\
                    data: {\"type\":\"run_finished\"}";

        let frames = parse_sse_frames(body);
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].event, "task_status_update");
        assert_eq!(frames[1].event, "run_event");
        assert_eq!(frames[1].id.as_deref(), Some("42"));
    }

    #[test]
    fn test_assert_json_path_eq() {
        let capture = SseCapture {
            status: StatusCode::OK,
            headers: HeaderMap::new(),
            raw_body: String::new(),
            frames: vec![SseFrame {
                event: "task_status_update".into(),
                id: None,
                data_raw: "{\"result\":{\"final\":true}}".into(),
                data_json: Some(serde_json::json!({"result": {"final": true}})),
            }],
        };

        capture.assert_json_path_eq(0, "result.final", Value::Bool(true));
    }
}
