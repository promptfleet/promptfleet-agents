//! A2A v1.0 Messaging Methods

use crate::data::message::{Message, MessageRole, Part};
use crate::data::task::{Task, TaskState};
use crate::error::A2AResult;
use crate::methods::params::{SendMessageRequest, SendMessageResponse};
use crate::services::TaskStorage;
use protocol_transport_core::{JsonRpcRequest, JsonRpcResponse};
use std::sync::Arc;
use uuid;

/// `SendMessage` handler (was `message/send`).
pub fn handle_message_send(
    request: JsonRpcRequest,
    storage: Arc<dyn TaskStorage>,
) -> A2AResult<JsonRpcResponse> {
    let params = SendMessageRequest::from_json(request.params)?;
    params.validate()?;

    let context_id = params
        .message
        .context_id
        .clone()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    log::debug!(
        "SendMessage: context_id={} task_id={} parts={}",
        context_id,
        params.message.task_id.as_deref().unwrap_or("<none>"),
        params.message.parts.len()
    );

    let response = if should_create_task(&params.message) {
        create_task_response(params, context_id, storage)?
    } else {
        create_message_response(params, context_id)?
    };

    Ok(JsonRpcResponse::success(
        request.id,
        serde_json::to_value(response)?,
    ))
}

fn should_create_task(message: &Message) -> bool {
    if message.parts.iter().any(|p| p.is_data()) {
        return true;
    }
    #[cfg(feature = "file-handling")]
    if message.parts.iter().any(|p| p.is_url() || p.is_raw()) {
        return true;
    }
    if message.parts.len() > 1 {
        return true;
    }
    if let Some(text) = message.parts.first().and_then(|p| p.get_text()) {
        let lower = text.to_lowercase();
        if matches!(
            lower.as_str(),
            "ping" | "health" | "status" | "ok" | "ack" | "acknowledgment"
        ) {
            return false;
        }
    }
    true
}

fn create_task_response(
    params: SendMessageRequest,
    context_id: String,
    storage: Arc<dyn TaskStorage>,
) -> A2AResult<SendMessageResponse> {
    let history_length = params
        .configuration
        .as_ref()
        .and_then(|cfg| cfg.history_length)
        .or(Some(0));
    let mut task = if let Some(task_id) = &params.message.task_id {
        match storage.get_task(task_id)? {
            Some(mut existing) => {
                existing.add_to_history(params.message.clone());
                existing.update_status(TaskState::Working);
                existing
            }
            None => {
                let mut new_task = Task::with_id(task_id.clone(), context_id);
                new_task.add_to_history(params.message.clone());
                new_task.update_status(TaskState::Working);
                new_task
            }
        }
    } else {
        let mut new_task = Task::new(context_id);
        new_task.add_to_history(params.message.clone());
        new_task.update_status(TaskState::Working);
        new_task
    };

    if let Some(metadata) = params.metadata {
        for (key, value) in metadata {
            task.set_metadata(key, value);
        }
    }

    storage.store_task(task.clone())?;
    apply_history_length(&mut task, history_length);
    Ok(SendMessageResponse::Task(task))
}

fn create_message_response(
    params: SendMessageRequest,
    context_id: String,
) -> A2AResult<SendMessageResponse> {
    let input_text = params.message.get_text_content().to_lowercase();
    let response_text = match input_text.as_str() {
        "ping" => "pong",
        "health" => "healthy",
        "status" => "active",
        "ok" => "acknowledged",
        "ack" | "acknowledgment" => "confirmed",
        _ => "utility response received",
    };

    let response_message = Message::new(
        MessageRole::Agent,
        vec![Part::text(response_text)],
        context_id.clone(),
    )
    .with_context(context_id);

    Ok(SendMessageResponse::Message(response_message))
}

fn apply_history_length(task: &mut Task, history_length: Option<u32>) {
    let keep = history_length.unwrap_or(0) as usize;
    match task.history.take() {
        Some(history) if keep == 0 => {
            task.history = None;
        }
        Some(history) => {
            let start = history.len().saturating_sub(keep);
            let trimmed: Vec<_> = history.into_iter().skip(start).collect();
            task.history = (!trimmed.is_empty()).then_some(trimmed);
        }
        None => {
            task.history = None;
        }
    }
}

/// `SendStreamingMessage` handler (was `tasks/sendSubscribe`).
#[cfg(feature = "event-stream")]
pub fn handle_tasks_send_subscribe(
    request: JsonRpcRequest,
    storage: Arc<dyn TaskStorage>,
) -> A2AResult<JsonRpcResponse> {
    let params = SendMessageRequest::from_json(request.params)?;
    params.validate()?;

    let context_id = params
        .message
        .context_id
        .clone()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    let mut task = Task::new(context_id);
    task.add_to_history(params.message.clone());
    task.update_status(TaskState::Working);
    task.set_metadata("streaming".to_string(), serde_json::Value::Bool(true));

    storage.store_task(task.clone())?;

    let result = SendMessageResponse::Task(task);
    Ok(JsonRpcResponse::success(
        request.id,
        serde_json::to_value(result)?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::message::MessageRole;
    use crate::services::InMemoryTaskStorage;
    use serde_json::json;

    fn create_test_message() -> Message {
        Message::new(
            MessageRole::User,
            vec![Part::text("Hello, agent!")],
            "task-123".to_string(),
        )
    }

    #[test]
    fn test_send_message_creates_new_task() {
        let storage = Arc::new(InMemoryTaskStorage::new());
        let message = create_test_message();

        let request = JsonRpcRequest::new(
            json!("req-123"),
            "SendMessage".to_string(),
            json!({"message": message}),
        );

        let response = handle_message_send(request, storage.clone()).unwrap();
        assert!(response.is_success());
        assert_eq!(storage.task_count(), 1);
    }

    #[test]
    fn test_send_message_updates_existing_task() {
        let storage = Arc::new(InMemoryTaskStorage::new());
        let task = Task::new("test-context".to_string());
        let task_id = task.id.clone();
        storage.store_task(task).unwrap();

        let mut message = create_test_message();
        message.task_id = Some(task_id.clone());

        let request = JsonRpcRequest::new(
            json!("req-456"),
            "SendMessage".to_string(),
            json!({"message": message}),
        );

        let response = handle_message_send(request, storage.clone()).unwrap();
        assert!(response.is_success());
        assert_eq!(storage.task_count(), 1);

        let updated_task = storage.get_task(&task_id).unwrap().unwrap();
        assert!(updated_task.history.is_some());
        assert_eq!(updated_task.status.state, TaskState::Working);
    }

    #[cfg(feature = "event-stream")]
    #[test]
    fn test_send_subscribe_creates_streaming_task() {
        let storage = Arc::new(InMemoryTaskStorage::new());
        let message = create_test_message();

        let request = JsonRpcRequest::new(
            json!("req-stream"),
            "SendStreamingMessage".to_string(),
            json!({"message": message}),
        );

        let response = handle_tasks_send_subscribe(request, storage.clone()).unwrap();
        assert!(response.is_success());

        let tasks = storage.list_tasks().unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].metadata.as_ref().unwrap()["streaming"], true);
    }
}
