//! A2A v1.0 Task Management Methods

use crate::{
    A2AResult, JsonRpcRequest, JsonRpcResponse,
    data::task::{Task, TaskState},
    methods::params::{CancelTaskRequest, GetTaskRequest, ListTasksRequest, ListTasksResponse},
    services::TaskStorage,
};
use std::sync::Arc;

/// `GetTask` handler (was `tasks/get`).
pub fn handle_tasks_get(
    request: JsonRpcRequest,
    storage: Arc<dyn TaskStorage>,
) -> A2AResult<JsonRpcResponse> {
    let params = GetTaskRequest::from_json(request.params)?;
    params.validate()?;
    log::debug!("GetTask: requested id={}", params.id);

    match storage.get_task(&params.id)? {
        Some(mut task) => {
            apply_history_length(&mut task, params.history_length);
            log::debug!("GetTask: found id={} state={}", task.id, task.status.state);
            Ok(JsonRpcResponse::success(
                request.id,
                serde_json::to_value(task)?,
            ))
        }
        None => {
            log::warn!("GetTask: not found id={}", params.id);
            Err(crate::A2AError::task_not_found(&params.id))
        }
    }
}

/// `CancelTask` handler (was `tasks/cancel`).
pub fn handle_tasks_cancel(
    request: JsonRpcRequest,
    storage: Arc<dyn TaskStorage>,
) -> A2AResult<JsonRpcResponse> {
    let params = CancelTaskRequest::from_json(request.params)?;
    params.validate()?;

    match storage.get_task(&params.id)? {
        Some(mut task) => {
            if task.is_terminal() {
                return Err(crate::A2AError::task_not_cancelable(&params.id));
            }

            task.update_status(TaskState::Canceled);

            if let Some(metadata) = params.metadata {
                for (key, value) in metadata {
                    task.set_metadata(key, value);
                }
            }

            storage.update_task(task.clone())?;
            Ok(JsonRpcResponse::success(
                request.id,
                serde_json::to_value(task)?,
            ))
        }
        None => Err(crate::A2AError::task_not_found(&params.id)),
    }
}

/// `ListTasks` handler (was `tasks/list`).
pub fn handle_tasks_list(
    request: JsonRpcRequest,
    storage: Arc<dyn TaskStorage>,
) -> A2AResult<JsonRpcResponse> {
    let params = ListTasksRequest::from_json(request.params)?;
    params.validate()?;

    let mut all_tasks = storage.list_tasks()?;

    if let Some(ref status) = params.status {
        all_tasks.retain(|t| t.status.state.as_str() == status.as_str());
    }

    if let Some(ref ctx) = params.context_id {
        all_tasks.retain(|t| t.context_id == *ctx);
    }

    let total = all_tasks.len();
    let page_size = params.page_size.unwrap_or(50) as usize;
    let mut tasks: Vec<Task> = all_tasks.into_iter().take(page_size).collect();
    for task in &mut tasks {
        apply_history_length(task, params.history_length);
    }
    let has_more = tasks.len() < total;

    let result = ListTasksResponse {
        tasks,
        next_page_token: if has_more {
            Some("next".to_string())
        } else {
            None
        },
        page_size: Some(page_size as u32),
        total_size: Some(total as u32),
    };

    Ok(JsonRpcResponse::success(
        request.id,
        serde_json::to_value(result)?,
    ))
}

fn apply_history_length(task: &mut Task, history_length: Option<u32>) {
    let Some(history_length) = history_length else {
        return;
    };
    let keep = history_length as usize;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::InMemoryTaskStorage;
    use serde_json::json;

    fn create_test_task() -> Task {
        let mut task = Task::new("test-context".to_string());
        task.update_status(TaskState::Working);
        task
    }

    #[test]
    fn test_get_task_existing() {
        let storage = Arc::new(InMemoryTaskStorage::new());
        let task = create_test_task();
        let task_id = task.id.clone();
        storage.store_task(task).unwrap();

        let request = JsonRpcRequest::new(
            json!("req-123"),
            "GetTask".to_string(),
            json!({"id": task_id}),
        );

        let response = handle_tasks_get(request, storage).unwrap();
        assert!(response.is_success());
    }

    #[test]
    fn test_get_task_not_found() {
        let storage = Arc::new(InMemoryTaskStorage::new());
        let request = JsonRpcRequest::new(
            json!("req"),
            "GetTask".to_string(),
            json!({"id": "nonexistent"}),
        );
        assert!(handle_tasks_get(request, storage).is_err());
    }

    #[test]
    fn test_cancel_task() {
        let storage = Arc::new(InMemoryTaskStorage::new());
        let task = create_test_task();
        let task_id = task.id.clone();
        storage.store_task(task).unwrap();

        let request = JsonRpcRequest::new(
            json!("req"),
            "CancelTask".to_string(),
            json!({"id": task_id}),
        );

        let response = handle_tasks_cancel(request, storage).unwrap();
        assert!(response.is_success());
    }

    #[test]
    fn test_cancel_terminal_task() {
        let storage = Arc::new(InMemoryTaskStorage::new());
        let mut task = create_test_task();
        task.update_status(TaskState::Completed);
        let task_id = task.id.clone();
        storage.store_task(task).unwrap();

        let request = JsonRpcRequest::new(
            json!("req"),
            "CancelTask".to_string(),
            json!({"id": task_id}),
        );
        assert!(handle_tasks_cancel(request, storage).is_err());
    }

    #[test]
    fn test_list_tasks() {
        let storage = Arc::new(InMemoryTaskStorage::new());
        for i in 0..3 {
            let mut task = Task::new(format!("ctx-{}", i));
            task.update_status(if i % 2 == 0 {
                TaskState::Working
            } else {
                TaskState::Completed
            });
            storage.store_task(task).unwrap();
        }

        let request = JsonRpcRequest::new(json!("req"), "ListTasks".to_string(), json!({}));
        let response = handle_tasks_list(request, storage).unwrap();
        assert!(response.is_success());
    }

    #[test]
    fn test_list_tasks_empty_storage() {
        let storage = Arc::new(InMemoryTaskStorage::new());
        let request = JsonRpcRequest::new(json!("req"), "ListTasks".to_string(), json!({}));
        let response = handle_tasks_list(request, storage).unwrap();
        assert!(response.is_success());
        let result = response.result.unwrap();
        assert_eq!(result["tasks"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn test_list_tasks_filter_by_context_id() {
        let storage = Arc::new(InMemoryTaskStorage::new());
        let mut t1 = Task::new("ctx-a".to_string());
        t1.update_status(TaskState::Working);
        let mut t2 = Task::new("ctx-b".to_string());
        t2.update_status(TaskState::Working);
        storage.store_task(t1).unwrap();
        storage.store_task(t2).unwrap();

        let request = JsonRpcRequest::new(
            json!("req"),
            "ListTasks".to_string(),
            json!({"contextId": "ctx-a"}),
        );
        let response = handle_tasks_list(request, storage).unwrap();
        let result = response.result.unwrap();
        let tasks = result["tasks"].as_array().unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0]["contextId"], "ctx-a");
    }

    #[test]
    fn test_list_tasks_filter_by_status() {
        let storage = Arc::new(InMemoryTaskStorage::new());
        let mut t1 = Task::new("ctx".to_string());
        t1.update_status(TaskState::Working);
        let mut t2 = Task::new("ctx".to_string());
        t2.update_status(TaskState::Completed);
        storage.store_task(t1).unwrap();
        storage.store_task(t2).unwrap();

        let request = JsonRpcRequest::new(
            json!("req"),
            "ListTasks".to_string(),
            json!({"status": "working"}),
        );
        let response = handle_tasks_list(request, storage).unwrap();
        let result = response.result.unwrap();
        let tasks = result["tasks"].as_array().unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0]["status"]["state"], "TASK_STATE_WORKING");
    }

    #[test]
    fn test_list_tasks_page_size_limits_results() {
        let storage = Arc::new(InMemoryTaskStorage::new());
        for _ in 0..5 {
            let mut t = Task::new("ctx".to_string());
            t.update_status(TaskState::Working);
            storage.store_task(t).unwrap();
        }
        let request = JsonRpcRequest::new(
            json!("req"),
            "ListTasks".to_string(),
            json!({"pageSize": 2}),
        );
        let response = handle_tasks_list(request, storage).unwrap();
        let result = response.result.unwrap();
        let tasks = result["tasks"].as_array().unwrap();
        assert_eq!(tasks.len(), 2);
        assert!(result.get("nextPageToken").is_some());
    }

    #[test]
    fn test_get_task_with_history_length() {
        let storage = Arc::new(InMemoryTaskStorage::new());
        let mut task = create_test_task();
        use crate::data::message::{Message, MessageRole, Part};
        for i in 0..3 {
            task.add_to_history(Message::new(
                MessageRole::User,
                vec![Part::text(format!("msg {i}"))],
                task.id.clone(),
            ));
        }
        let task_id = task.id.clone();
        storage.store_task(task).unwrap();

        let request = JsonRpcRequest::new(
            json!("req"),
            "GetTask".to_string(),
            json!({"id": task_id, "historyLength": 1}),
        );
        let response = handle_tasks_get(request, storage).unwrap();
        let result = response.result.unwrap();
        let history = result["history"].as_array().unwrap();
        assert_eq!(history.len(), 1, "historyLength=1 should trim to 1 message");
    }

    #[test]
    fn test_cancel_failed_task_returns_not_cancelable() {
        let storage = Arc::new(InMemoryTaskStorage::new());
        let mut task = create_test_task();
        task.update_status(TaskState::Failed);
        let task_id = task.id.clone();
        storage.store_task(task).unwrap();

        let request = JsonRpcRequest::new(
            json!("req"),
            "CancelTask".to_string(),
            json!({"id": task_id}),
        );
        assert!(handle_tasks_cancel(request, storage).is_err());
    }

    #[test]
    fn test_cancel_rejected_task_returns_not_cancelable() {
        let storage = Arc::new(InMemoryTaskStorage::new());
        let mut task = create_test_task();
        task.update_status(TaskState::Rejected);
        let task_id = task.id.clone();
        storage.store_task(task).unwrap();

        let request = JsonRpcRequest::new(
            json!("req"),
            "CancelTask".to_string(),
            json!({"id": task_id}),
        );
        assert!(handle_tasks_cancel(request, storage).is_err());
    }

    #[test]
    fn test_get_task_empty_id_returns_error() {
        let storage = Arc::new(InMemoryTaskStorage::new());
        let request = JsonRpcRequest::new(json!("req"), "GetTask".to_string(), json!({"id": ""}));
        assert!(handle_tasks_get(request, storage).is_err());
    }
}
