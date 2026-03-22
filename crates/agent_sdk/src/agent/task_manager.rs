//! Task Management Module
//!
//! Handles task context creation and management for conversation continuity
//! following clean architecture principles.

use std::sync::Arc;
use std::sync::RwLock;

use a2a_protocol_core::data::task::Task;
use a2a_protocol_core::services::TaskStorage;

use super::{message::TaskContext, task_store::RuntimeTaskStore};
use crate::error::SdkResult;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TaskReusePolicy {
    ReuseTerminalTask,
    StartNewTaskAfterTerminal,
}

/// **Task Manager**
///
/// Handles task context creation, retrieval, and persistence for conversation continuity.
/// Provides a clean abstraction over the runtime task-store seam.
pub struct TaskManager {
    /// Runtime task store for conversation continuity
    runtime_store: Arc<dyn RuntimeTaskStore>,
    canonical_store: RwLock<Option<Arc<dyn TaskStorage>>>,
}

impl TaskManager {
    /// Create a new task manager
    pub(crate) fn new(runtime_store: Arc<dyn RuntimeTaskStore>) -> Self {
        Self {
            runtime_store,
            canonical_store: RwLock::new(None),
        }
    }

    /// Get or create task context for conversation continuity
    ///
    /// This method handles both existing task retrieval and new task creation
    /// based on the provided context_id.
    pub(crate) async fn get_or_create_task_context(
        &self,
        context_id: &Option<String>,
        reuse_policy: TaskReusePolicy,
    ) -> SdkResult<TaskContext> {
        match context_id {
            Some(ctx_id) => {
                let snapshot = self.runtime_store.get_latest_snapshot(ctx_id)?;
                if let Some(task) = self.load_canonical_task(ctx_id, snapshot.as_ref())? {
                    if reuse_policy == TaskReusePolicy::StartNewTaskAfterTerminal
                        && task.is_terminal()
                    {
                        Ok(self.prepare_new_task_context(&task, ctx_id.clone(), snapshot.as_ref())?)
                    } else {
                        Ok(self.prepare_task_context(&task, ctx_id.clone(), snapshot.as_ref())?)
                    }
                } else if let Some(snapshot) = snapshot {
                    if reuse_policy == TaskReusePolicy::StartNewTaskAfterTerminal
                        && is_terminal_phase(&snapshot.task_phase)
                    {
                        Ok(self.prepare_new_task_context_from_snapshot(snapshot))
                    } else {
                        Ok(TaskContext {
                            task_id: snapshot.task_id.clone(),
                            context_id: Some(snapshot.context_id.clone()),
                            runtime_history: Vec::new(),
                            task_phase: snapshot.task_phase.clone(),
                            artifacts: Vec::new(),
                            task_metadata: snapshot.metadata_extract.clone(),
                            created_at: snapshot.created_at.clone(),
                            updated_at: snapshot.updated_at.clone(),
                            continuation: Some(
                                crate::agent::history_policy::continuation_state_from_snapshot(
                                    &snapshot,
                                ),
                            ),
                        })
                    }
                } else {
                    Ok(TaskContext::create_new(Some(ctx_id.clone())))
                }
            }
            None => Ok(TaskContext::create_new(None)),
        }
    }

    pub(crate) fn runtime_store(&self) -> Arc<dyn RuntimeTaskStore> {
        self.runtime_store.clone()
    }

    pub(crate) fn attach_canonical_task_storage(&self, storage: Arc<dyn TaskStorage>) -> SdkResult<()> {
        let mut guard = self.canonical_store.write().map_err(|_| {
            crate::SdkError::method_execution("task_manager", "canonical task store lock poisoned")
        })?;
        *guard = Some(storage);
        Ok(())
    }

    fn load_canonical_task(
        &self,
        context_id: &str,
        snapshot: Option<&crate::agent::task_store::ContinuationSnapshot>,
    ) -> SdkResult<Option<a2a_protocol_core::data::task::Task>> {
        let guard = self.canonical_store.read().map_err(|_| {
            crate::SdkError::method_execution("task_manager", "canonical task store lock poisoned")
        })?;
        let Some(storage) = guard.as_ref() else {
            return Ok(None);
        };

        if let Some(snapshot) = snapshot {
            if let Some(task) = storage.get_task(&snapshot.task_id).map_err(crate::SdkError::from)? {
                return Ok(Some(task));
            }
        }

        storage
            .get_latest_task_in_context(context_id)
            .map_err(crate::SdkError::from)
    }

    fn prepare_task_context(
        &self,
        task: &Task,
        context_id: String,
        snapshot: Option<&crate::agent::task_store::ContinuationSnapshot>,
    ) -> SdkResult<TaskContext> {
        let mut task_ctx = crate::conversions::task_context_from_a2a_task(task, context_id);
        if let Some(snapshot) = snapshot {
            let current_revision = self.runtime_store.get_task_revision(&task.id)?;
            if current_revision == Some(snapshot.source_revision) {
                task_ctx.continuation = Some(
                    crate::agent::history_policy::continuation_state_from_snapshot(snapshot),
                );
            }
        }
        Ok(task_ctx)
    }

    fn prepare_new_task_context(
        &self,
        task: &Task,
        context_id: String,
        snapshot: Option<&crate::agent::task_store::ContinuationSnapshot>,
    ) -> SdkResult<TaskContext> {
        let mut task_ctx = TaskContext::create_new(Some(context_id));
        task_ctx.runtime_history = task
            .history
            .clone()
            .unwrap_or_default()
            .into_iter()
            .map(crate::conversions::agent_message_from_a2a)
            .collect();
        task_ctx.task_metadata = task.metadata.clone().unwrap_or_default();
        if let Some(snapshot) = snapshot {
            for (key, value) in &snapshot.metadata_extract {
                task_ctx
                    .task_metadata
                    .entry(key.clone())
                    .or_insert_with(|| value.clone());
            }
            let current_revision = self.runtime_store.get_task_revision(&task.id)?;
            if current_revision == Some(snapshot.source_revision) {
                task_ctx.continuation = Some(
                    crate::agent::history_policy::continuation_state_from_snapshot(snapshot),
                );
            }
        }
        Ok(task_ctx)
    }

    fn prepare_new_task_context_from_snapshot(
        &self,
        snapshot: crate::agent::task_store::ContinuationSnapshot,
    ) -> TaskContext {
        let mut task_ctx = TaskContext::create_new(Some(snapshot.context_id.clone()));
        task_ctx.task_metadata = snapshot.metadata_extract.clone();
        task_ctx.continuation = Some(
            crate::agent::history_policy::continuation_state_from_snapshot(&snapshot),
        );
        task_ctx
    }
}

fn is_terminal_phase(phase: &agent_core::TaskPhase) -> bool {
    matches!(
        phase,
        agent_core::TaskPhase::Completed
            | agent_core::TaskPhase::Failed
            | agent_core::TaskPhase::Cancelled
    )
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use a2a_protocol_core::data::{Message, MessageRole, Task, TaskState};
    use a2a_protocol_core::services::{InMemoryTaskStorage, TaskStorage};
    use serde_json::{Value, json};

    use super::{TaskManager, TaskReusePolicy};
    use crate::agent::task_store::{
        ContinuationArtifactRef, ContinuationSnapshot, ContinuationStrategyDescriptor, RuntimeTaskStore,
    };
    use crate::SdkResult;

    struct TestRuntimeTaskStore {
        latest_by_context: std::sync::RwLock<HashMap<String, ContinuationSnapshot>>,
        revisions_by_task: std::sync::RwLock<HashMap<String, u64>>,
    }

    impl TestRuntimeTaskStore {
        fn new() -> Self {
            Self {
                latest_by_context: std::sync::RwLock::new(HashMap::new()),
                revisions_by_task: std::sync::RwLock::new(HashMap::new()),
            }
        }
    }

    impl RuntimeTaskStore for TestRuntimeTaskStore {
        fn get_latest_snapshot(&self, context_id: &str) -> SdkResult<Option<ContinuationSnapshot>> {
            self.latest_by_context
                .read()
                .map(|contexts| contexts.get(context_id).cloned())
                .map_err(|_| crate::SdkError::method_execution("test_runtime_task_store", "lock poisoned"))
        }

        fn set_latest_snapshot(
            &self,
            context_id: &str,
            snapshot: Option<ContinuationSnapshot>,
        ) -> SdkResult<()> {
            let mut contexts = self.latest_by_context.write().map_err(|_| {
                crate::SdkError::method_execution("test_runtime_task_store", "lock poisoned")
            })?;
            match snapshot {
                Some(snapshot) => {
                    contexts.insert(context_id.to_string(), snapshot);
                }
                None => {
                    contexts.remove(context_id);
                }
            }
            Ok(())
        }

        fn get_task_revision(&self, task_id: &str) -> SdkResult<Option<u64>> {
            self.revisions_by_task
                .read()
                .map(|revisions| revisions.get(task_id).copied())
                .map_err(|_| crate::SdkError::method_execution("test_runtime_task_store", "lock poisoned"))
        }

        fn set_task_revision(&self, task_id: &str, revision: u64) -> SdkResult<()> {
            let mut revisions = self.revisions_by_task.write().map_err(|_| {
                crate::SdkError::method_execution("test_runtime_task_store", "lock poisoned")
            })?;
            revisions.insert(task_id.to_string(), revision);
            Ok(())
        }

        fn delete_task_revision(&self, task_id: &str) -> SdkResult<()> {
            let mut revisions = self.revisions_by_task.write().map_err(|_| {
                crate::SdkError::method_execution("test_runtime_task_store", "lock poisoned")
            })?;
            revisions.remove(task_id);
            Ok(())
        }
    }

    #[tokio::test]
    async fn starts_new_task_after_terminal_when_policy_requests_it() {
        let runtime_store: Arc<dyn RuntimeTaskStore> = Arc::new(TestRuntimeTaskStore::new());
        let task_manager = TaskManager::new(runtime_store.clone());
        let canonical: Arc<dyn TaskStorage> = Arc::new(InMemoryTaskStorage::new());
        task_manager
            .attach_canonical_task_storage(canonical.clone())
            .unwrap();

        let context_id = "ctx-terminal".to_string();
        let task_id = "task-terminal".to_string();
        let mut task = Task::with_id(task_id.clone(), context_id.clone());
        task.add_to_history(
            Message::text(MessageRole::User, "first", task_id.clone()).with_context(context_id.clone()),
        );
        task.add_to_history(
            Message::text(MessageRole::Agent, "done", task_id.clone()).with_context(context_id.clone()),
        );
        task.update_status(TaskState::Completed);
        canonical.store_task(task).unwrap();

        runtime_store
            .set_task_revision(&task_id, 1)
            .unwrap();
        runtime_store
            .set_latest_snapshot(
                &context_id,
                Some(ContinuationSnapshot {
                    task_id: task_id.clone(),
                    context_id: context_id.clone(),
                    source_revision: 1,
                    strategy: ContinuationStrategyDescriptor::default(),
                    task_phase: agent_core::TaskPhase::Completed,
                    latest_status_message: Some(agent_core::AgentMessage::agent_text("done")),
                    artifact_refs: vec![ContinuationArtifactRef {
                        name: "answer".to_string(),
                        description: None,
                    }],
                    metadata_extract: HashMap::from([("source".to_string(), json!("test"))]),
                    payload: Value::Null,
                    created_at: None,
                    updated_at: None,
                }),
            )
            .unwrap();

        let resumed = task_manager
            .get_or_create_task_context(
                &Some(context_id.clone()),
                TaskReusePolicy::ReuseTerminalTask,
            )
            .await
            .unwrap();
        assert_eq!(resumed.task_id, task_id);

        let fresh = task_manager
            .get_or_create_task_context(
                &Some(context_id.clone()),
                TaskReusePolicy::StartNewTaskAfterTerminal,
            )
            .await
            .unwrap();
        assert_ne!(fresh.task_id, task_id);
        assert_eq!(fresh.context_id.as_deref(), Some(context_id.as_str()));
        assert_eq!(fresh.runtime_history.len(), 2);
        assert_eq!(fresh.task_phase, agent_core::TaskPhase::Pending);
        assert!(fresh.artifacts.is_empty());
        assert_eq!(fresh.task_metadata.get("source"), Some(&json!("test")));
        assert!(fresh.continuation.is_some());
    }
}
