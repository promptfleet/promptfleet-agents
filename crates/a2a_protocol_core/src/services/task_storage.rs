//! Task Storage Service
//!
//! Provides task storage and retrieval services for A2A protocol operations.
//! This is a pure domain service with no infrastructure dependencies.

use crate::{A2AError, A2AResult, data::message::Message, data::task::Task};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// **Context Information for Conversation Management**
///
/// Represents a conversation context that contains multiple tasks
/// following the A2A protocol specification.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ConversationContext {
    /// Unique context identifier (persistent across tasks)
    pub context_id: String,
    /// When the conversation started
    pub created_at: String,
    /// Last interaction time
    pub last_active: String,
    /// Number of tasks in this context
    pub task_count: u32,
}

impl ConversationContext {
    /// Create a new conversation context
    pub fn new(context_id: String) -> Self {
        let now = Self::current_timestamp();
        Self {
            context_id,
            created_at: now.clone(),
            last_active: now,
            task_count: 0,
        }
    }

    /// Update last active timestamp and increment task count
    pub fn update_activity(&mut self) {
        self.last_active = Self::current_timestamp();
        self.task_count += 1;
    }

    fn current_timestamp() -> String {
        #[cfg(feature = "time-stamps")]
        {
            chrono::Utc::now().to_rfc3339()
        }
        #[cfg(not(feature = "time-stamps"))]
        {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
                .to_string()
        }
    }
}

/// **Enhanced Task Storage Service Interface**
///
/// Defines the contract for task storage operations with proper A2A protocol
/// support for context-based operations (1 context : N tasks relationship).
pub trait TaskStorage: Send + Sync {
    // ============= EXISTING METHODS (MAINTAINED FOR COMPATIBILITY) =============

    /// Store a new or updated task
    fn store_task(&self, task: Task) -> A2AResult<()>;

    /// Retrieve a task by ID
    fn get_task(&self, task_id: &str) -> A2AResult<Option<Task>>;

    /// Update an existing task
    fn update_task(&self, task: Task) -> A2AResult<()>;

    /// List all tasks (for tasks/list method)
    fn list_tasks(&self) -> A2AResult<Vec<Task>>;

    /// Remove a task (if needed for cleanup)
    fn remove_task(&self, task_id: &str) -> A2AResult<bool>;

    /// Check if a task exists
    fn task_exists(&self, task_id: &str) -> A2AResult<bool>;

    // ============= NEW CONTEXT-BASED OPERATIONS (A2A PROTOCOL) =============

    /// Get all tasks within a specific context (conversation)
    ///
    /// This is the key method for A2A protocol compliance - allows retrieving
    /// all tasks that belong to the same conversation context.
    fn get_tasks_by_context(&self, context_id: &str) -> A2AResult<Vec<Task>>;

    /// Get the latest (most recently created) task in a context
    ///
    /// Useful for conversation continuity and determining the most recent
    /// task state within a conversation.
    fn get_latest_task_in_context(&self, context_id: &str) -> A2AResult<Option<Task>>;

    /// Get conversation history from all tasks in a context
    ///
    /// Aggregates message history from all tasks in a conversation context
    /// for LLM context management as specified in A2A protocol.
    fn get_context_history(&self, context_id: &str) -> A2AResult<Vec<Message>>;

    /// Get or create conversation context
    ///
    /// Manages conversation context lifecycle - creates new context if it
    /// doesn't exist, returns existing context otherwise.
    fn get_or_create_context(&self, context_id: &str) -> A2AResult<ConversationContext>;

    /// Update context activity (called when new task is added)
    ///
    /// Updates the conversation context metadata when tasks are added
    /// to track conversation lifecycle.
    fn update_context_activity(&self, context_id: &str) -> A2AResult<()>;

    /// List all conversation contexts
    ///
    /// Returns all active conversation contexts for management/debugging.
    fn list_contexts(&self) -> A2AResult<Vec<ConversationContext>>;
}

/// In-Memory Task Storage Implementation
///
/// Provides a simple in-memory task storage for development and testing.
/// In production, this would be replaced with persistent storage.
///
/// **Enhanced A2A Protocol Support**: Now includes separate context tracking
/// for proper conversation management with 1:N context:task relationship.
#[derive(Debug, Clone)]
pub struct InMemoryTaskStorage {
    /// Task storage: task_id -> Task
    tasks: Arc<RwLock<HashMap<String, Task>>>,
    /// Context storage: context_id -> ConversationContext
    contexts: Arc<RwLock<HashMap<String, ConversationContext>>>,
    /// Task insertion order tracking: context_id -> Vec<task_id> (newest last)
    task_order: Arc<RwLock<HashMap<String, Vec<String>>>>,
}

impl InMemoryTaskStorage {
    /// Create a new in-memory task storage
    pub fn new() -> Self {
        Self {
            tasks: Arc::new(RwLock::new(HashMap::new())),
            contexts: Arc::new(RwLock::new(HashMap::new())),
            task_order: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Get the number of stored tasks (for testing)
    pub fn task_count(&self) -> usize {
        self.tasks.read().unwrap().len()
    }

    /// Get the number of stored contexts (for testing)
    pub fn context_count(&self) -> usize {
        self.contexts.read().unwrap().len()
    }

    /// Clear all tasks and contexts (for testing)
    pub fn clear(&self) {
        self.tasks.write().unwrap().clear();
        self.contexts.write().unwrap().clear();
        self.task_order.write().unwrap().clear();
    }
}

impl Default for InMemoryTaskStorage {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskStorage for InMemoryTaskStorage {
    fn store_task(&self, task: Task) -> A2AResult<()> {
        let mut tasks = self
            .tasks
            .write()
            .map_err(|_| A2AError::internal("Failed to acquire task storage lock"))?;

        // Check if this is a new task (not an update)
        let is_new_task = !tasks.contains_key(&task.id);

        // Store the task
        tasks.insert(task.id.clone(), task.clone());

        // Update task order
        let mut task_order = self
            .task_order
            .write()
            .map_err(|_| A2AError::internal("Failed to acquire task order lock"))?;
        task_order
            .entry(task.context_id.clone())
            .or_insert_with(Vec::new)
            .push(task.id.clone());

        // Release task lock before acquiring context lock to avoid deadlock
        drop(tasks);
        drop(task_order);

        // Update context activity if this is a new task
        if is_new_task {
            let mut contexts = self
                .contexts
                .write()
                .map_err(|_| A2AError::internal("Failed to acquire context storage lock"))?;

            if let Some(context) = contexts.get_mut(&task.context_id) {
                context.update_activity();
            } else {
                // Create new context for this task
                let mut new_context = ConversationContext::new(task.context_id.clone());
                new_context.update_activity();
                contexts.insert(task.context_id.clone(), new_context);
            }
        }

        Ok(())
    }

    fn get_task(&self, task_id: &str) -> A2AResult<Option<Task>> {
        let tasks = self
            .tasks
            .read()
            .map_err(|_| A2AError::internal("Failed to acquire task storage lock"))?;

        Ok(tasks.get(task_id).cloned())
    }

    fn update_task(&self, task: Task) -> A2AResult<()> {
        let mut tasks = self
            .tasks
            .write()
            .map_err(|_| A2AError::internal("Failed to acquire task storage lock"))?;

        if tasks.contains_key(&task.id) {
            tasks.insert(task.id.clone(), task);
            Ok(())
        } else {
            Err(A2AError::method_execution_failed(
                "tasks/update",
                &format!("Task not found: {}", &task.id),
            ))
        }
    }

    fn list_tasks(&self) -> A2AResult<Vec<Task>> {
        let tasks = self
            .tasks
            .read()
            .map_err(|_| A2AError::internal("Failed to acquire task storage lock"))?;

        Ok(tasks.values().cloned().collect())
    }

    fn remove_task(&self, task_id: &str) -> A2AResult<bool> {
        let mut tasks = self
            .tasks
            .write()
            .map_err(|_| A2AError::internal("Failed to acquire task storage lock"))?;

        Ok(tasks.remove(task_id).is_some())
    }

    fn task_exists(&self, task_id: &str) -> A2AResult<bool> {
        let tasks = self
            .tasks
            .read()
            .map_err(|_| A2AError::internal("Failed to acquire task storage lock"))?;

        Ok(tasks.contains_key(task_id))
    }

    fn get_tasks_by_context(&self, context_id: &str) -> A2AResult<Vec<Task>> {
        let tasks = self
            .tasks
            .read()
            .map_err(|_| A2AError::internal("Failed to acquire task storage lock"))?;

        Ok(tasks
            .values()
            .cloned()
            .filter(|task| task.context_id == context_id)
            .collect())
    }

    fn get_latest_task_in_context(&self, context_id: &str) -> A2AResult<Option<Task>> {
        let task_order = self
            .task_order
            .read()
            .map_err(|_| A2AError::internal("Failed to acquire task order lock"))?;

        if let Some(task_ids) = task_order.get(context_id) {
            // Find the task with the highest insertion order (newest last)
            let latest_task_id = task_ids.last().cloned();
            if let Some(task_id) = latest_task_id {
                let tasks = self
                    .tasks
                    .read()
                    .map_err(|_| A2AError::internal("Failed to acquire task storage lock"))?;
                Ok(tasks.get(&task_id).cloned())
            } else {
                Ok(None)
            }
        } else {
            Ok(None)
        }
    }

    fn get_context_history(&self, context_id: &str) -> A2AResult<Vec<Message>> {
        let task_order = self
            .task_order
            .read()
            .map_err(|_| A2AError::internal("Failed to acquire task order lock"))?;
        let tasks = self
            .tasks
            .read()
            .map_err(|_| A2AError::internal("Failed to acquire task storage lock"))?;

        let mut messages: Vec<Message> = Vec::new();

        // Get tasks in insertion order
        if let Some(task_ids) = task_order.get(context_id) {
            for task_id in task_ids {
                if let Some(task) = tasks.get(task_id) {
                    if let Some(ref task_history) = task.history {
                        messages.extend(task_history.iter().cloned());
                    }
                }
            }
        }

        Ok(messages)
    }

    fn get_or_create_context(&self, context_id: &str) -> A2AResult<ConversationContext> {
        let mut contexts = self
            .contexts
            .write()
            .map_err(|_| A2AError::internal("Failed to acquire context storage lock"))?;

        if let Some(context) = contexts.get(context_id) {
            Ok(context.clone())
        } else {
            let new_context = ConversationContext::new(context_id.to_string());
            contexts.insert(context_id.to_string(), new_context.clone());
            Ok(new_context)
        }
    }

    fn update_context_activity(&self, context_id: &str) -> A2AResult<()> {
        let mut contexts = self
            .contexts
            .write()
            .map_err(|_| A2AError::internal("Failed to acquire context storage lock"))?;

        if let Some(context) = contexts.get_mut(context_id) {
            context.update_activity();
            Ok(())
        } else {
            Err(A2AError::method_execution_failed(
                "update_context_activity",
                &format!("Context not found: {}", context_id),
            ))
        }
    }

    fn list_contexts(&self) -> A2AResult<Vec<ConversationContext>> {
        let contexts = self
            .contexts
            .read()
            .map_err(|_| A2AError::internal("Failed to acquire context storage lock"))?;

        Ok(contexts.values().cloned().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::task::TaskState;

    #[test]
    fn test_in_memory_storage_basic_operations() {
        let storage = InMemoryTaskStorage::new();

        // Create a test task
        let task = Task::new("test-context".to_string());
        let task_id = task.id.clone();

        // Store task
        storage.store_task(task.clone()).unwrap();
        assert_eq!(storage.task_count(), 1);

        // Retrieve task
        let retrieved = storage.get_task(&task_id).unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().id, task_id);

        // Check existence
        assert!(storage.task_exists(&task_id).unwrap());
        assert!(!storage.task_exists("non-existent").unwrap());

        // List tasks
        let tasks = storage.list_tasks().unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].id, task_id);

        // Remove task
        assert!(storage.remove_task(&task_id).unwrap());
        assert!(!storage.remove_task(&task_id).unwrap());
        assert_eq!(storage.task_count(), 0);
    }

    #[test]
    fn test_update_task() {
        let storage = InMemoryTaskStorage::new();

        // Create and store task
        let mut task = Task::new("test-context".to_string());
        let task_id = task.id.clone();
        storage.store_task(task.clone()).unwrap();

        // Update task state
        task.update_status(TaskState::Working);
        storage.update_task(task.clone()).unwrap();

        // Verify update
        let retrieved = storage.get_task(&task_id).unwrap().unwrap();
        assert_eq!(retrieved.status.state, TaskState::Working);
    }

    #[test]
    fn test_update_nonexistent_task() {
        let storage = InMemoryTaskStorage::new();
        let task = Task::new("test-context".to_string());

        // Try to update non-existent task
        let result = storage.update_task(task);
        assert!(result.is_err());
    }

    #[test]
    fn test_get_nonexistent_task() {
        let storage = InMemoryTaskStorage::new();
        let result = storage.get_task("non-existent").unwrap();
        assert!(result.is_none());
    }

    // NEW COMPREHENSIVE TESTS FOR 100% COVERAGE

    #[test]
    fn test_default_constructor() {
        let storage = InMemoryTaskStorage::default();
        assert_eq!(storage.task_count(), 0);
        assert_eq!(storage.context_count(), 0);

        // Ensure default and new() produce equivalent results
        let storage_new = InMemoryTaskStorage::new();
        assert_eq!(storage.task_count(), storage_new.task_count());
        assert_eq!(storage.context_count(), storage_new.context_count());
    }

    #[test]
    fn test_clear_functionality() {
        let storage = InMemoryTaskStorage::new();

        // Add multiple tasks
        let task1 = Task::new("context1".to_string());
        let task2 = Task::new("context2".to_string());
        let task3 = Task::new("context3".to_string());

        storage.store_task(task1).unwrap();
        storage.store_task(task2).unwrap();
        storage.store_task(task3).unwrap();

        assert_eq!(storage.task_count(), 3);
        assert_eq!(storage.context_count(), 3); // Contexts auto-created

        // Clear all tasks
        storage.clear();
        assert_eq!(storage.task_count(), 0);
        assert_eq!(storage.context_count(), 0);

        // Verify all tasks are gone
        let tasks = storage.list_tasks().unwrap();
        assert!(tasks.is_empty());
    }

    #[test]
    fn test_comprehensive_storage_workflow() {
        let storage = InMemoryTaskStorage::new();

        // Create multiple tasks
        let mut task1 = Task::new("workflow-test-1".to_string());
        let task2 = Task::new("workflow-test-2".to_string());
        let task1_id = task1.id.clone();
        let task2_id = task2.id.clone();

        // Store tasks
        storage.store_task(task1.clone()).unwrap();
        storage.store_task(task2.clone()).unwrap();
        assert_eq!(storage.task_count(), 2);
        assert_eq!(storage.context_count(), 2); // Two different contexts

        // Update first task
        task1.update_status(TaskState::Working);
        storage.update_task(task1.clone()).unwrap();

        // Verify updated task
        let retrieved_task1 = storage.get_task(&task1_id).unwrap().unwrap();
        assert_eq!(retrieved_task1.status.state, TaskState::Working);

        // List all tasks
        let all_tasks = storage.list_tasks().unwrap();
        assert_eq!(all_tasks.len(), 2);

        // Check task existence
        assert!(storage.task_exists(&task1_id).unwrap());
        assert!(storage.task_exists(&task2_id).unwrap());
        assert!(!storage.task_exists("non-existent-id").unwrap());

        // Remove one task
        assert!(storage.remove_task(&task1_id).unwrap());
        assert!(!storage.task_exists(&task1_id).unwrap());
        assert_eq!(storage.task_count(), 1);

        // Try to remove the same task again
        assert!(!storage.remove_task(&task1_id).unwrap());

        // Clear remaining tasks
        storage.clear();
        assert_eq!(storage.task_count(), 0);
        assert_eq!(storage.context_count(), 0); // Contexts also cleared
        assert!(!storage.task_exists(&task2_id).unwrap());
    }

    #[test]
    fn test_concurrent_storage_operations() {
        use std::sync::Arc;
        use std::thread;

        let storage = Arc::new(InMemoryTaskStorage::new());
        let mut handles = vec![];

        // Spawn multiple threads to test concurrent access
        for i in 0..5 {
            let storage_clone = Arc::clone(&storage);
            let handle = thread::spawn(move || {
                let task = Task::new(format!("concurrent-task-{}", i));
                let task_id = task.id.clone();

                // Store task
                storage_clone.store_task(task).unwrap();

                // Verify task exists
                assert!(storage_clone.task_exists(&task_id).unwrap());

                // Retrieve task
                let retrieved = storage_clone.get_task(&task_id).unwrap();
                assert!(retrieved.is_some());

                task_id
            });
            handles.push(handle);
        }

        // Wait for all threads to complete and collect task IDs
        let task_ids: Vec<String> = handles.into_iter().map(|h| h.join().unwrap()).collect();

        // Verify all tasks are stored
        assert_eq!(storage.task_count(), 5);

        // List all tasks
        let all_tasks = storage.list_tasks().unwrap();
        assert_eq!(all_tasks.len(), 5);

        // Verify each task ID is present
        for task_id in task_ids {
            assert!(storage.task_exists(&task_id).unwrap());
        }
    }

    #[test]
    fn test_edge_cases_and_error_conditions() {
        let storage = InMemoryTaskStorage::new();

        // Test updating non-existent task with detailed error message
        let non_existent_task = Task::new("non-existent-context".to_string());
        let update_result = storage.update_task(non_existent_task.clone());
        assert!(update_result.is_err());

        // Verify error message contains task ID
        let error = update_result.unwrap_err();
        let error_message = format!("{}", error);
        assert!(error_message.contains(&non_existent_task.id));

        // Test removing non-existent task returns false
        assert!(!storage.remove_task("definitely-not-there").unwrap());

        // Test empty storage operations
        assert_eq!(storage.task_count(), 0);
        assert_eq!(storage.context_count(), 0);
        assert!(storage.list_tasks().unwrap().is_empty());
        assert!(!storage.task_exists("any-id").unwrap());

        // Test storage after clear on empty storage
        storage.clear();
        assert_eq!(storage.task_count(), 0);
        assert_eq!(storage.context_count(), 0);
    }

    #[test]
    fn test_task_replacement_behavior() {
        let storage = InMemoryTaskStorage::new();

        // Create original task
        let mut original_task = Task::new("replacement-test".to_string());
        let task_id = original_task.id.clone();

        // Store original task
        storage.store_task(original_task.clone()).unwrap();
        assert_eq!(storage.task_count(), 1);
        assert_eq!(storage.context_count(), 1); // Context should be created

        // Update task state and re-store (replacement)
        original_task.update_status(TaskState::Completed);
        storage.store_task(original_task.clone()).unwrap();

        // Verify only one task exists (replacement, not addition)
        assert_eq!(storage.task_count(), 1);
        assert_eq!(storage.context_count(), 1); // Context count unchanged

        // Verify task has updated state
        let retrieved = storage.get_task(&task_id).unwrap().unwrap();
        assert_eq!(retrieved.status.state, TaskState::Completed);
    }

    // ============= NEW A2A PROTOCOL COMPLIANCE TESTS =============

    #[test]
    fn test_context_based_operations() {
        let storage = InMemoryTaskStorage::new();

        // Create tasks in different contexts
        let task1_ctx1 = Task::new("context-1".to_string());
        let task2_ctx1 = Task::new("context-1".to_string());
        let task1_ctx2 = Task::new("context-2".to_string());

        let _task1_ctx1_id = task1_ctx1.id.clone();
        let task2_ctx1_id = task2_ctx1.id.clone();
        let _task1_ctx2_id = task1_ctx2.id.clone();

        // Store tasks
        storage.store_task(task1_ctx1).unwrap();
        storage.store_task(task2_ctx1).unwrap();
        storage.store_task(task1_ctx2).unwrap();

        assert_eq!(storage.task_count(), 3);
        assert_eq!(storage.context_count(), 2);

        // Test get_tasks_by_context
        let ctx1_tasks = storage.get_tasks_by_context("context-1").unwrap();
        assert_eq!(ctx1_tasks.len(), 2);

        let ctx2_tasks = storage.get_tasks_by_context("context-2").unwrap();
        assert_eq!(ctx2_tasks.len(), 1);

        // Test get_latest_task_in_context
        let latest_ctx1 = storage.get_latest_task_in_context("context-1").unwrap();
        assert!(latest_ctx1.is_some());
        // Should be task2_ctx1 since it was created later
        assert_eq!(latest_ctx1.unwrap().id, task2_ctx1_id);

        // Test list_contexts
        let contexts = storage.list_contexts().unwrap();
        assert_eq!(contexts.len(), 2);

        // Verify context activity tracking
        let ctx1_context = contexts
            .iter()
            .find(|c| c.context_id == "context-1")
            .unwrap();
        assert_eq!(ctx1_context.task_count, 2);

        let ctx2_context = contexts
            .iter()
            .find(|c| c.context_id == "context-2")
            .unwrap();
        assert_eq!(ctx2_context.task_count, 1);
    }

    #[test]
    fn test_conversation_history_aggregation() {
        use crate::data::message::{Message, MessageRole, Part};

        let storage = InMemoryTaskStorage::new();

        // Create tasks with message history
        let mut task1 = Task::new("conversation-test".to_string());
        let mut task2 = Task::new("conversation-test".to_string());

        // Add messages to task1 history using with_id() to preserve specific IDs
        let msg1 = Message::with_id(
            "msg-1".to_string(),
            MessageRole::User,
            vec![Part::text("Hello")],
        );
        let msg2 = Message::with_id(
            "msg-2".to_string(),
            MessageRole::Agent,
            vec![Part::text("Hi there!")],
        );
        task1.add_to_history(msg1);
        task1.add_to_history(msg2);

        // Add messages to task2 history
        let msg3 = Message::with_id(
            "msg-3".to_string(),
            MessageRole::User,
            vec![Part::text("How are you?")],
        );
        let msg4 = Message::with_id(
            "msg-4".to_string(),
            MessageRole::Agent,
            vec![Part::text("I'm doing well!")],
        );
        task2.add_to_history(msg3);
        task2.add_to_history(msg4);

        // Store tasks
        storage.store_task(task1).unwrap();
        storage.store_task(task2).unwrap();

        // Test conversation history aggregation
        let history = storage.get_context_history("conversation-test").unwrap();
        assert_eq!(history.len(), 4);

        // Verify message order is maintained
        assert_eq!(history[0].message_id, "msg-1");
        assert_eq!(history[1].message_id, "msg-2");
        assert_eq!(history[2].message_id, "msg-3");
        assert_eq!(history[3].message_id, "msg-4");
    }

    #[test]
    fn test_context_lifecycle_management() {
        let storage = InMemoryTaskStorage::new();

        // Test get_or_create_context with new context
        let context1 = storage.get_or_create_context("new-context").unwrap();
        assert_eq!(context1.context_id, "new-context");
        assert_eq!(context1.task_count, 0);
        assert_eq!(storage.context_count(), 1);

        // Test get_or_create_context with existing context
        let context1_again = storage.get_or_create_context("new-context").unwrap();
        assert_eq!(context1_again.context_id, "new-context");
        assert_eq!(storage.context_count(), 1); // Should not create duplicate

        // Add a task to update context activity
        let task = Task::new("new-context".to_string());
        storage.store_task(task).unwrap();

        // Verify context was updated
        let updated_context = storage.get_or_create_context("new-context").unwrap();
        assert_eq!(updated_context.task_count, 1);

        // Test update_context_activity directly
        storage.update_context_activity("new-context").unwrap();
        let context_after_update = storage.get_or_create_context("new-context").unwrap();
        assert_eq!(context_after_update.task_count, 2);

        // Test update_context_activity with non-existent context
        let result = storage.update_context_activity("non-existent");
        assert!(result.is_err());
    }

    #[test]
    fn test_empty_context_operations() {
        let storage = InMemoryTaskStorage::new();

        // Test operations on empty storage
        let tasks = storage.get_tasks_by_context("empty-context").unwrap();
        assert!(tasks.is_empty());

        let latest = storage.get_latest_task_in_context("empty-context").unwrap();
        assert!(latest.is_none());

        let history = storage.get_context_history("empty-context").unwrap();
        assert!(history.is_empty());

        let contexts = storage.list_contexts().unwrap();
        assert!(contexts.is_empty());
    }

    #[test]
    fn test_a2a_protocol_conversation_flow() {
        // This test simulates the A2A protocol example from the specification
        let storage = InMemoryTaskStorage::new();

        // First interaction: Generate sailboat image
        let mut task1 = Task::new("ctx-conversation-abc".to_string());
        task1.id = "task-boat-gen-123".to_string();
        task1.update_status(TaskState::Completed);

        // Second interaction: Color the boat red (same context, new task)
        let mut task2 = Task::new("ctx-conversation-abc".to_string());
        task2.id = "task-boat-color-456".to_string();
        task2.update_status(TaskState::Completed);

        // Store both tasks
        storage.store_task(task1).unwrap();
        storage.store_task(task2).unwrap();

        // Verify A2A protocol compliance
        assert_eq!(storage.task_count(), 2);
        assert_eq!(storage.context_count(), 1);

        // Verify context contains both tasks
        let context_tasks = storage
            .get_tasks_by_context("ctx-conversation-abc")
            .unwrap();
        assert_eq!(context_tasks.len(), 2);

        // Verify latest task is the color modification
        let latest = storage
            .get_latest_task_in_context("ctx-conversation-abc")
            .unwrap();
        assert_eq!(latest.unwrap().id, "task-boat-color-456");

        // Verify context metadata
        let context = storage
            .get_or_create_context("ctx-conversation-abc")
            .unwrap();
        assert_eq!(context.task_count, 2);
        assert_eq!(context.context_id, "ctx-conversation-abc");
    }
}
