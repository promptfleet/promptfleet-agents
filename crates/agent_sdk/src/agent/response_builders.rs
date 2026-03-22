//! Response Builder Module
//!
//! Creates different types of runtime responses following clean architecture principles.

use agent_core::{ContentPart, TaskPhase};
use log::debug;
use serde_json::Value;

use super::message::{MessageContext, SkillCall, TaskContext};
use crate::agent::response::{Response, RuntimeArtifact, RuntimeResponse, TaskOpts};
use crate::error::SdkResult;

/// **Response Builder**
///
/// Provides static methods for creating different types of runtime responses.
/// Follows the builder pattern and maintains consistency across response types.
pub struct ResponseBuilder;

impl ResponseBuilder {
    /// Create skill success response
    ///
    /// Generates a runtime response when a skill executes successfully.
    /// Handles both stateful (Task) and stateless (Message) response modes.
    pub fn create_skill_success_response(
        skill_call: &SkillCall,
        result: Value,
        msg_ctx: &MessageContext,
        task_ctx: Option<TaskContext>,
    ) -> SdkResult<RuntimeResponse> {
        debug!(
            "Creating skill success response for skill: {}",
            skill_call.skill_id
        );

        let pretty_result =
            serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string());
        let response_text = format!(
            "Skill '{}' executed successfully. Result: {}",
            skill_call.skill_id, pretty_result
        );

        // Delegate to explicit API: return a Task with structured result as artifact
        let artifact = RuntimeArtifact::data(format!("{}_result", skill_call.skill_id), result);
        Response::task(
            TaskOpts {
                artifacts: vec![artifact],
                state: Some(TaskPhase::Completed),
                status_text: None,
                task_meta: None,
                history_parts: Some(vec![ContentPart::Text(response_text)]),
            },
            msg_ctx,
            task_ctx,
        )
    }

    /// Create response for skill that's advertised but not implemented
    ///
    /// Provides a helpful conversational response when a skill is in the agent card
    /// but doesn't have an automated handler implementation.
    pub fn create_skill_not_implemented_response(
        skill_call: &SkillCall,
        msg_ctx: &MessageContext,
        task_ctx: Option<TaskContext>,
    ) -> SdkResult<RuntimeResponse> {
        let response_text = format!(
            "I understand you want to use the '{}' skill, but I don't have an automated handler for it yet. \
             Let me help you with what I can do conversationally. Could you provide more details about what you need?",
            skill_call.skill_id
        );

        // Delegate to explicit API: prefer a polite stateless message by default
        Response::message_text(
            response_text,
            None,
            None,
            task_ctx.and_then(|t| t.context_id),
        )
    }

    /// Create conversational response for non-skill messages
    ///
    /// Handles general conversational messages that don't involve skill calls.
    /// Provides appropriate responses based on stateful vs stateless mode.
    pub fn create_conversational_response(
        msg_ctx: &MessageContext,
        task_ctx: Option<TaskContext>,
    ) -> SdkResult<RuntimeResponse> {
        let text_content = msg_ctx.text_content.as_deref().unwrap_or("No text content");
        let response_text = format!(
            "I received your message: {}. I'm a conversational agent ready to help!",
            text_content
        );

        // Delegate to explicit API: prefer stateless message by default
        Response::message_text(
            response_text,
            None,
            None,
            task_ctx.and_then(|t| t.context_id),
        )
    }
}
