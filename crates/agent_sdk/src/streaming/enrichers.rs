use serde_json::{json, Value};

use crate::agent::trace::AgentTraceEvent;

use super::{AgentIoEvent, IoEventContext, StreamEnricher};

pub struct CitationEnricher {
    pub tool_name_pattern: fn(&str) -> bool,
}

impl Default for CitationEnricher {
    fn default() -> Self {
        Self {
            tool_name_pattern: default_tool_name_pattern,
        }
    }
}

impl StreamEnricher for CitationEnricher {
    fn enrich(&self, event: &AgentTraceEvent, ctx: &IoEventContext) -> Vec<AgentIoEvent> {
        let AgentTraceEvent::ToolCallCompleted {
            id,
            name,
            result,
            success,
            ..
        } = event
        else {
            return Vec::new();
        };

        if !success || !(self.tool_name_pattern)(name) {
            return Vec::new();
        }

        let Some(results_arr) = result.get("results").and_then(|v| v.as_array()) else {
            return Vec::new();
        };

        let citations: Vec<Value> = results_arr
            .iter()
            .enumerate()
            .filter_map(|(i, r)| {
                let url = r.get("url").and_then(|v| v.as_str())?;
                Some(json!({
                    "id": format!("cite-{}-{}", id, i),
                    "title": r.get("title").and_then(|v| v.as_str()),
                    "url": url,
                    "snippet": r
                        .get("content")
                        .and_then(|v| v.as_str())
                        .map(|s| {
                            if s.len() > 300 {
                                format!("{}...", &s[..300])
                            } else {
                                s.to_string()
                            }
                        }),
                    "sourceType": "web",
                }))
            })
            .collect();

        if citations.is_empty() {
            return Vec::new();
        }

        vec![AgentIoEvent::Custom {
            name: "citations_updated".to_string(),
            value: json!({
                "messageId": ctx.message_id,
                "items": citations,
            }),
        }]
    }
}

fn default_tool_name_pattern(tool_name: &str) -> bool {
    tool_name.to_lowercase().contains("search")
}
