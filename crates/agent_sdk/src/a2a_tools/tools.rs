use crate::SdkResult;
use crate::agent::tools::{ToolExecutor, ToolRegistry, ToolSpec};
use log::{debug, info};
use serde_json::Value;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct A2AToolConfig {
    /// Tool allowlist — only tools whose names appear here are registered.
    /// Known names: `"agent.card.get"`, `"a2a.message_send"`.
    pub llm_tools: Vec<String>,
}

fn schema_agent_card_get() -> Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "jsonrpc_url": {
                "type": "string",
                "pattern": "^https?://.+",
                "description": "Agent JSON-RPC endpoint URL."
            }
        },
        "required": ["jsonrpc_url"],
        "additionalProperties": false
    })
}

fn schema_a2a_message_send() -> Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "jsonrpc_url": {
                "type": "string",
                "pattern": "^https?://.+",
                "description": "Target agent JSON-RPC endpoint URL."
            },
            "message": {
                "type": "object",
                "description": "A2A message payload. For skill-like calls, add a Data part with {\"skill\":\"<id>\", ...params}.",
                "properties": {
                    "role": { "type": "string", "enum": ["user", "assistant"], "default": "user" },
                    "parts": {
                        "type": "array",
                        "minItems": 1,
                        "items": {
                            "type": "object",
                            "properties": {
                                "kind": { "type": "string", "enum": ["text", "data"] },
                                "text": { "type": "string", "minLength": 1 },
                                "data": { "type": "object", "additionalProperties": true }
                            },
                            "required": ["kind"],
                            "additionalProperties": false
                        }
                    }
                },
                "required": ["parts"],
                "additionalProperties": false
            },
            "context_id": { "type": "string" },
            "reference_task_ids": { "type": "array", "items": { "type": "string" } }
        },
        "required": ["jsonrpc_url", "message"],
        "additionalProperties": false
    })
}

pub fn make_tools_from_names<I, S>(names: I) -> SdkResult<ToolRegistry>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let cfg = A2AToolConfig {
        llm_tools: names.into_iter().map(Into::into).collect(),
    };
    make_tools_from_config(cfg)
}

pub fn make_tools_from_config(cfg: A2AToolConfig) -> SdkResult<ToolRegistry> {
    let allow = cfg.llm_tools;
    let mut reg = ToolRegistry::new();

    if allow.iter().any(|n| n == "agent.card.get") {
        reg.register(ToolSpec {
            name: "agent_card_get".to_string(),
            description: Some("Fetch an AgentCard via JSON-RPC agent/card/get.".to_string()),
            parameters: schema_agent_card_get(),
            strict: true,
            parallel_ok: false,
            executor: ToolExecutor::Simple(Arc::new(|args| {
                Box::pin(async move {
                    let url = args
                        .get("jsonrpc_url")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| "Missing required: jsonrpc_url".to_string())?
                        .to_string();
                    info!("tool:agent.card.get:start url={}", url);
                    let out = super::ops::agent_card_get(&url)
                        .await
                        .map_err(|e| e.to_string())?;
                    let name = out
                        .get("agent")
                        .and_then(|a| a.get("name"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let version = out
                        .get("agent")
                        .and_then(|a| a.get("version"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let skills = out
                        .get("skills")
                        .and_then(|v| v.as_array())
                        .map(|a| a.len())
                        .unwrap_or(0);
                    info!(
                        "tool:agent.card.get:ok name={} version={} skills={}",
                        name, version, skills
                    );
                    debug!("tool:agent.card.get:response={}", out);
                    Ok(out)
                })
            })),
        });
    }

    if allow.iter().any(|n| n == "a2a.message_send") {
        reg.register(ToolSpec {
            name: "a2a_message_send".to_string(),
            description: Some("Send an A2A message to an agent via its jsonrpc_url.".to_string()),
            parameters: schema_a2a_message_send(),
            strict: true,
            parallel_ok: false,
            executor: ToolExecutor::Simple(Arc::new(|args| {
                Box::pin(async move {
                    let url = args
                        .get("jsonrpc_url")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| "Missing required: jsonrpc_url".to_string())?
                        .to_string();
                    let mut params = args.clone();
                    if let Some(obj) = params.as_object_mut() {
                        obj.remove("jsonrpc_url");
                    }
                    let skill_hint = params
                        .get("message")
                        .and_then(|m| m.get("parts"))
                        .and_then(|p| p.as_array())
                        .and_then(|arr| {
                            arr.iter().find_map(|part| {
                                part.get("data")
                                    .and_then(|d| d.get("skill"))
                                    .and_then(|v| v.as_str())
                            })
                        })
                        .map(|s| s.to_string());
                    info!(
                        "tool:a2a.message_send:start url={} skill_hint={:?}",
                        url, skill_hint
                    );
                    let out = super::ops::a2a_message_send(&url, params)
                        .await
                        .map_err(|e| e.to_string())?;
                    let task_id = out
                        .get("task")
                        .and_then(|t| t.get("id"))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    info!("tool:a2a.message_send:ok task_id={:?}", task_id);
                    debug!("tool:a2a.message_send:response={}", out);
                    Ok(out)
                })
            })),
        });
    }

    Ok(reg)
}
