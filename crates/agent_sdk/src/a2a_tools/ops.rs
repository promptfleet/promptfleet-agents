use crate::{SdkError, SdkResult};
use log::{debug, info};
use serde_json::Value;

#[cfg(feature = "agent-observability")]
use std::sync::OnceLock;

#[cfg(feature = "agent-observability")]
use observability::ObsHandle;

#[cfg(feature = "agent-observability")]
fn obs_from_env_cached() -> Option<observability::Obs> {
    static OBS: OnceLock<Option<observability::Obs>> = OnceLock::new();
    OBS.get_or_init(|| {
        if let Some(obs) = crate::shared_observability() {
            log::info!("a2a_tools.observability:using_shared_obs");
            Some(obs)
        } else {
            match observability::Obs::init_from_env() {
                Ok(o) => {
                    log::info!("a2a_tools.observability:initialized_from_env");
                    Some(o)
                }
                Err(e) => {
                    log::warn!("a2a_tools.observability:init_from_env_failed error={}", e);
                    None
                }
            }
        }
    })
    .clone()
}

/// JSON-RPC call: `GetAgentCard` returning raw AgentCard JSON through the official client.
pub async fn agent_card_get(jsonrpc_url: &str) -> SdkResult<Value> {
    use a2a_http_client::Client;
    let client = {
        let c = Client::external(jsonrpc_url);
        #[cfg(feature = "agent-observability")]
        {
            if let Some(obs) = obs_from_env_cached() {
                c.with_observability(obs)
            } else {
                c
            }
        }
        #[cfg(not(feature = "agent-observability"))]
        {
            c
        }
    };
    info!("a2a_tools.agent.card.get:start url={}", jsonrpc_url);
    let card_json = client.metadata().await.map_err(|e| {
        SdkError::method_execution(
            "agent.card.get",
            format!("Failed to get agent card from {}: {}", jsonrpc_url, e),
        )
    })?;
    let name = card_json.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let version = card_json
        .get("version")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let skills = card_json
        .get("skills")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);
    info!(
        "a2a_tools.agent.card.get:ok name={} version={} skills={}",
        name, version, skills
    );
    debug!("a2a_tools.agent.card.get:response={}", card_json);
    Ok(card_json)
}

/// JSON-RPC call: `SendMessage` using the official client; args should contain keys: message, context_id?, metadata?
pub async fn a2a_message_send(jsonrpc_url: &str, args: Value) -> SdkResult<Value> {
    use a2a_http_client::Client;
    use a2a_protocol_core::data::message::Message;

    let mut msg_val = args
        .get("message")
        .cloned()
        .ok_or_else(|| SdkError::invalid_input("'message' field is required"))?;
    if let Some(obj) = msg_val.as_object_mut() {
        if !obj.contains_key("role") {
            obj.insert("role".to_string(), serde_json::json!("user"));
        }
        if !obj.contains_key("messageId") && !obj.contains_key("message_id") {
            obj.insert(
                "messageId".to_string(),
                serde_json::json!(uuid::Uuid::new_v4().to_string()),
            );
        }
    }

    let mut message: Message = serde_json::from_value(msg_val.clone()).map_err(|e| {
        let err = e.to_string();
        let hint = if err.contains("missing field `role`") {
            "Add 'role': 'user' to message"
        } else if err.contains("missing field") && err.contains("messageId") {
            "Add 'messageId': '<any string>' to message"
        } else {
            "Check message.role/messageId and parts for required fields"
        };
        SdkError::invalid_input(format!("invalid message format: {} | hint: {}", err, hint))
    })?;

    let context_id = args
        .get("context_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let metadata = args
        .get("metadata")
        .and_then(|v| v.as_object())
        .map(|m| m.clone());

    if let Some(refs) = args.get("reference_task_ids").and_then(|v| v.as_array()) {
        let ids: Vec<String> = refs
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect();
        if !ids.is_empty() {
            message.reference_task_ids = Some(ids);
        }
    }

    let mut skill_hint: Option<String> = None;
    for p in &message.parts {
        if let Some(data) = &p.data {
            if let Some(s) = data
                .get("skill")
                .and_then(|v: &serde_json::Value| v.as_str())
            {
                skill_hint = Some(s.to_string());
                break;
            }
        }
    }
    info!(
        "a2a_tools.message_send:start url={} role={:?} parts={} context_id={:?} skill_hint={:?}",
        jsonrpc_url,
        message.role,
        message.parts.len(),
        context_id,
        skill_hint
    );

    let client = {
        let c = Client::external(jsonrpc_url);
        #[cfg(feature = "agent-observability")]
        {
            if let Some(obs) = obs_from_env_cached() {
                c.with_observability(obs)
            } else {
                c
            }
        }
        #[cfg(not(feature = "agent-observability"))]
        {
            c
        }
    };
    if let Some(ctx_id) = context_id {
        message.context_id = Some(ctx_id);
    }
    let result = client
        .message_send(
            message,
            metadata.map(|m| {
                m.into_iter()
                    .collect::<std::collections::HashMap<String, serde_json::Value>>()
            }),
        )
        .await
        .map_err(|e| {
            SdkError::method_execution("a2a.message_send", format!("SendMessage failed: {}", e))
        })?;

    #[cfg(feature = "agent-observability")]
    {
        if let Some(obs) = obs_from_env_cached() {
            let _ = obs.flush();
        }
    }
    let task_id = result
        .get("task")
        .and_then(|t| t.get("id"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    info!("a2a_tools.message_send:ok task_id={:?}", task_id);
    debug!("a2a_tools.message_send:response={}", result);
    Ok(result)
}
