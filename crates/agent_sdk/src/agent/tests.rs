//! Agent Module Tests
//!
//! Comprehensive tests for the agent module architecture covering
//! agent creation, skill registration, and AgentCard generation.

use super::*;
use crate::a2a;
use crate::error::SdkError;
use serde_json::json;

#[test]
fn test_agent_creation() {
    let agent = Agent::new_runtime("test-agent").unwrap();

    assert_eq!(agent.config().name, "test-agent");
    assert!(!a2a::agent_card(&agent).name.is_empty());
    assert_eq!(agent.list_skills().len(), 0);
    assert_eq!(agent.list_notifications().len(), 0);
}

#[test]
fn test_agent_creation_with_config() {
    let config = AgentConfig {
        name: "detailed-test-agent".to_string(),
        description: "Agent for detailed testing".to_string(),
        version: "2.0.0".to_string(),
        storage_prefix: None,
        max_message_size: 2_097_152,
        streaming: true,
        batch_processing: true,
        concurrent_tasks: Some(20),
        stateless_methods: false,
        base_url: None,
        history_policy: None,
    };

    let agent = Agent::new_with_config(config.clone()).unwrap();
    let agent_card = a2a::agent_card(&agent);

    assert_eq!(agent.config().name, "detailed-test-agent");
    assert_eq!(agent_card.name, "detailed-test-agent");
    assert_eq!(
        agent_card.description.as_deref(),
        Some("Agent for detailed testing")
    );
    assert_eq!(agent_card.version.as_deref(), Some("2.0.0"));
    let caps = agent_card
        .capabilities
        .as_ref()
        .expect("capabilities should be present");
    assert!(caps.streaming);
}

#[tokio::test]
async fn test_skill_registration_via_builder() {
    let mut agent = Agent::new_runtime("test-agent").unwrap();

    agent
        .skill("add_numbers", |params| async move {
            let a = params["a"].as_f64().unwrap_or(0.0);
            let b = params["b"].as_f64().unwrap_or(0.0);
            Ok(json!({"result": a + b}))
        })
        .register()
        .unwrap();

    let skills = agent.list_skills();
    assert_eq!(skills.len(), 1);
    assert!(skills.contains(&"add_numbers".to_string()));

    let agent_card = a2a::agent_card(&agent);
    assert!(agent_card.get_skill("add_numbers").is_some());
    assert!(!agent_card.supports_method("add_numbers"));
}

#[tokio::test]
async fn test_notification_registration() {
    let mut agent = Agent::new_runtime("test-agent").unwrap();

    agent
        .register_notification("log_event", |params| async move {
            println!("Event: {}", params);
            Ok(())
        })
        .unwrap();

    let notifications = agent.list_notifications();
    assert_eq!(notifications.len(), 1);
    assert!(notifications.contains(&"log_event".to_string()));
}

#[test]
fn test_invalid_agent_creation() {
    let config = AgentConfig {
        name: "".to_string(),
        ..Default::default()
    };

    let result = Agent::new_with_config(config);
    assert!(result.is_err());
    if let Err(e) = result {
        assert!(matches!(e, SdkError::InvalidInput { .. }));
    }
}

#[test]
fn test_invalid_skill_registration() {
    let mut agent = Agent::new_runtime("test-agent").unwrap();

    let result = agent.skill("", |_| async { Ok(json!({})) }).register();
    assert!(result.is_err());
    if let Err(e) = result {
        assert!(matches!(e, SdkError::InvalidInput { .. }));
    }
}

#[test]
fn test_agent_card_generation() {
    let mut agent = Agent::new_runtime("weather-agent").unwrap();

    agent
        .skill("get_weather", |_| async { Ok(json!({"temp": 22})) })
        .register()
        .unwrap();

    agent
        .skill("get_forecast", |_| async { Ok(json!({"forecast": []})) })
        .register()
        .unwrap();

    let agent_card = a2a::agent_card(&agent);

    assert_eq!(agent_card.name, "weather-agent");
    assert!(agent_card.get_skill("get_weather").is_some());
    assert!(agent_card.get_skill("get_forecast").is_some());
    assert_eq!(agent_card.skills.len(), 2);
    assert!(!agent_card.supports_method("get_weather"));
    assert!(!agent_card.supports_method("get_forecast"));
}

#[test]
fn test_agent_card_only_exposed_skills() {
    let mut agent = Agent::new_runtime("visibility-test").unwrap();

    agent
        .skill("public_skill", |_| async { Ok(json!({})) })
        .expose(true)
        .register()
        .unwrap();

    agent
        .skill("hidden_skill", |_| async { Ok(json!({})) })
        .expose(false)
        .register()
        .unwrap();

    let card = a2a::agent_card(&agent);
    assert_eq!(card.skills.len(), 1);
    assert_eq!(card.skills[0].id, "public_skill");
}

#[test]
fn test_request_handling() {
    let mut agent = Agent::new_runtime("test-agent").unwrap();

    agent
        .skill("echo", |params| async move { Ok(params) })
        .register()
        .unwrap();

    let agent_card = a2a::agent_card(&agent);
    assert!(agent_card.get_skill("echo").is_some());
    assert_eq!(agent_card.skills.len(), 1);
    assert_eq!(agent_card.skills[0].id, "echo");
    assert!(!agent_card.supports_method("echo"));
}

#[test]
fn test_response_mode_configurations() {
    let config = AgentConfig::default();
    let _agent = Agent::new_with_config(config).unwrap();

    let config = AgentConfig::new("api-agent", "API Agent").stateless();
    assert!(config.stateless_methods);
    let _agent = Agent::new_with_config(config).unwrap();

    let config = AgentConfig::new("chat-agent", "Chat Agent").stateful();
    assert!(!config.stateless_methods);
    let _agent = Agent::new_with_config(config).unwrap();
}

#[test]
fn test_skill_builder_pattern() {
    let mut agent = Agent::new_runtime("test-agent").unwrap();

    let result = agent
        .skill("test_skill", |_| async { Ok(json!({})) })
        .display_name("Test Skill")
        .tags(&["test", "demo"])
        .instructions("Be careful with edge cases.")
        .llm_callable(true)
        .register();

    assert!(result.is_ok());

    let skills = agent.list_skills();
    assert!(skills.contains(&"test_skill".to_string()));

    let defs = agent.skill_registry().get_skill_definitions();
    let def = defs.get("test_skill").unwrap();
    assert_eq!(
        def.instructions.as_deref(),
        Some("Be careful with edge cases.")
    );
    assert!(def.llm_callable);
    assert!(def.expose);
}

#[test]
fn test_service_injection() {
    let agent = Agent::new_runtime("test-agent").unwrap();

    let agent_with_service = agent.with_service("test_service".to_string());

    assert!(agent_with_service.has_service::<String>());
    assert!(agent_with_service.get_service::<String>().is_some());
    assert!(!agent_with_service.has_service::<i32>());
    assert!(agent_with_service.get_service::<i32>().is_none());
}

#[test]
fn test_duplicate_skill_registration_fails_fast() {
    let mut agent = Agent::new_runtime("test-agent").unwrap();

    agent
        .skill("get_weather", |_| async { Ok(json!({"temp": 22})) })
        .register()
        .unwrap();

    let result = agent
        .skill("get_weather", |_| async { Ok(json!({"temp": 25})) })
        .register();

    assert!(result.is_err());
    let error_msg = result.unwrap_err().to_string();
    assert!(error_msg.contains("already exists"));

    let agent_card = a2a::agent_card(&agent);
    assert_eq!(agent_card.skills.len(), 1);
    assert!(agent_card.get_skill("get_weather").is_some());
}

#[tokio::test]
async fn test_three_tier_message_handler_cascade() {
    use a2a_protocol_core::data::message::{Message, MessageRole, Part};
    use a2a_protocol_core::methods::params::MessageSendParams;

    let mut agent = Agent::new_runtime("test-agent").unwrap();

    agent
        .add_skill("travel_planning")
        .description("Help plan trips and itineraries")
        .register()
        .unwrap();

    agent
        .skill("get_weather", |params| async move {
            let location = params["location"].as_str().unwrap_or("unknown");
            Ok(json!({"location": location, "temp": 22, "condition": "sunny"}))
        })
        .register()
        .unwrap();

    let skill_data = serde_json::json!({
        "skill": "get_weather",
        "location": "Tokyo"
    });

    let message_with_handler = Message {
        role: MessageRole::User,
        parts: vec![Part::data(skill_data)],
        message_id: "test-msg-1".to_string(),
        task_id: None,
        context_id: None,
        metadata: None,
        extensions: None,
        reference_task_ids: None,
    };

    let params_with_handler = MessageSendParams {
        message: message_with_handler,
        configuration: None,
        metadata: None,
        tenant: None,
    };

    let result_with_handler = a2a::handle_message_send(&agent, params_with_handler).await;
    assert!(result_with_handler.is_ok());

    let skill_data_no_handler = serde_json::json!({
        "skill": "travel_planning",
        "destination": "Japan"
    });

    let message_no_handler = Message {
        role: MessageRole::User,
        parts: vec![Part::data(skill_data_no_handler)],
        message_id: "test-msg-2".to_string(),
        task_id: None,
        context_id: None,
        metadata: None,
        extensions: None,
        reference_task_ids: None,
    };

    let params_no_handler = MessageSendParams {
        message: message_no_handler,
        configuration: None,
        metadata: None,
        tenant: None,
    };

    let result_no_handler = a2a::handle_message_send(&agent, params_no_handler).await;
    assert!(result_no_handler.is_ok());

    match result_no_handler.unwrap() {
        a2a_protocol_core::methods::params::MessageSendResponse::Task(task) => {
            if let Some(history) = &task.history {
                let agent_response = history
                    .iter()
                    .find(|msg| msg.role == MessageRole::Agent)
                    .expect("Should have agent response in history");
                let response_text = agent_response.get_text_content();
                assert!(response_text.contains("travel_planning"));
                assert!(response_text.contains("don't have an automated handler"));
            }
        }
        a2a_protocol_core::methods::params::MessageSendResponse::Message(message) => {
            let response_text = message.get_text_content();
            assert!(response_text.contains("travel_planning"));
            assert!(response_text.contains("don't have an automated handler"));
        }
    }
}

#[test]
fn test_skill_def_to_agent_skill_excludes_internal_fields() {
    use super::skill::SkillDefinition;
    use crate::a2a::skill_def_to_agent_skill;

    let def = SkillDefinition {
        id: "test".to_string(),
        name: "Test".to_string(),
        description: "A test skill".to_string(),
        input_modes: vec!["text/plain".to_string()],
        output_modes: vec!["text/plain".to_string()],
        schema: None,
        examples: None,
        tags: Some(vec!["test".to_string()]),
        instructions: Some("Secret instructions".to_string()),
        expose: false,
        llm_callable: true,
    };

    let agent_skill = skill_def_to_agent_skill(&def);
    assert_eq!(agent_skill.id, "test");
    assert_eq!(agent_skill.name, "Test");
    assert_eq!(agent_skill.tags, Some(vec!["test".to_string()]));
    let json = serde_json::to_value(&agent_skill).unwrap();
    assert!(json.get("instructions").is_none());
    assert!(json.get("expose").is_none());
    assert!(json.get("llm_callable").is_none());
}

#[tokio::test]
async fn test_resolve_skill_context_handler_only() {
    use super::skill::SkillRegistry;

    let mut reg = SkillRegistry::new();
    reg.skill("compute", |_p| async move { Ok(json!({"answer": 42})) })
        .register()
        .unwrap();

    let ctx = reg
        .resolve_skill_context("compute", &serde_json::Value::Null)
        .await
        .expect("should resolve when handler exists");
    assert_eq!(ctx.skill_id, "compute");
    assert!(
        ctx.handler_output.is_some(),
        "handler output should be present"
    );
    assert!(ctx.instructions.is_none(), "instructions should be None");
}
