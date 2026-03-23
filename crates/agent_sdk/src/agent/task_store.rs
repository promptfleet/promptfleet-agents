//! Runtime continuation-store seam.
//!
//! This module is intentionally protocol-neutral. It owns only derived
//! continuation snapshots and their freshness sidecars.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use agent_core::{AgentMessage, TaskPhase};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{SdkError, SdkResult};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct ContinuationStrategyDescriptor {
    pub kind: String,
    pub version: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub composition: Option<Vec<String>>,
}

impl Default for ContinuationStrategyDescriptor {
    fn default() -> Self {
        Self {
            kind: "canonical_pass_through".to_string(),
            version: 1,
            composition: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct ContinuationArtifactRef {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct ContinuationSnapshot {
    pub task_id: String,
    pub context_id: String,
    pub source_revision: u64,
    pub strategy: ContinuationStrategyDescriptor,
    pub task_phase: TaskPhase,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_status_message: Option<AgentMessage>,
    #[serde(default)]
    pub artifact_refs: Vec<ContinuationArtifactRef>,
    #[serde(default)]
    pub metadata_extract: HashMap<String, Value>,
    #[serde(default)]
    pub payload: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

/// Minimal runtime-facing storage contract for derived continuation state.
pub(crate) trait RuntimeTaskStore: Send + Sync {
    /// Load the most recent continuation snapshot for a conversation.
    fn get_latest_snapshot(&self, context_id: &str) -> SdkResult<Option<ContinuationSnapshot>>;

    /// Update or clear the latest continuation snapshot for a conversation.
    fn set_latest_snapshot(
        &self,
        context_id: &str,
        snapshot: Option<ContinuationSnapshot>,
    ) -> SdkResult<()>;

    /// Load the current canonical revision sidecar for a task.
    fn get_task_revision(&self, task_id: &str) -> SdkResult<Option<u64>>;

    /// Persist the canonical revision sidecar for a task.
    fn set_task_revision(&self, task_id: &str, revision: u64) -> SdkResult<()>;

    /// Delete the canonical revision sidecar for a task.
    fn delete_task_revision(&self, task_id: &str) -> SdkResult<()>;
}

/// In-memory continuation store used when no shared persistence is configured.
struct InMemoryRuntimeTaskStore {
    latest_by_context: RwLock<HashMap<String, ContinuationSnapshot>>,
    revisions_by_task: RwLock<HashMap<String, u64>>,
}

impl InMemoryRuntimeTaskStore {
    fn new() -> Self {
        Self {
            latest_by_context: RwLock::new(HashMap::new()),
            revisions_by_task: RwLock::new(HashMap::new()),
        }
    }
}

impl RuntimeTaskStore for InMemoryRuntimeTaskStore {
    fn get_latest_snapshot(&self, context_id: &str) -> SdkResult<Option<ContinuationSnapshot>> {
        self.latest_by_context
            .read()
            .map(|contexts| contexts.get(context_id).cloned())
            .map_err(|_| {
                SdkError::method_execution("task_store", "runtime task store lock poisoned")
            })
    }

    fn set_latest_snapshot(
        &self,
        context_id: &str,
        snapshot: Option<ContinuationSnapshot>,
    ) -> SdkResult<()> {
        let mut contexts = self.latest_by_context.write().map_err(|_| {
            SdkError::method_execution("task_store", "runtime task store lock poisoned")
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
            .map_err(|_| {
                SdkError::method_execution("task_store", "runtime task revision lock poisoned")
            })
    }

    fn set_task_revision(&self, task_id: &str, revision: u64) -> SdkResult<()> {
        let mut revisions = self.revisions_by_task.write().map_err(|_| {
            SdkError::method_execution("task_store", "runtime task revision lock poisoned")
        })?;
        revisions.insert(task_id.to_string(), revision);
        Ok(())
    }

    fn delete_task_revision(&self, task_id: &str) -> SdkResult<()> {
        let mut revisions = self.revisions_by_task.write().map_err(|_| {
            SdkError::method_execution("task_store", "runtime task revision lock poisoned")
        })?;
        revisions.remove(task_id);
        Ok(())
    }
}

/// Build the default runtime task store using the configured persistence mode.
pub(crate) fn build_default_runtime_task_store(
    storage_prefix: String,
) -> SdkResult<Arc<dyn RuntimeTaskStore>> {
    #[cfg(all(target_arch = "wasm32", feature = "a2a-server"))]
    {
        return Ok(Arc::new(
            crate::wasm_kv_task_storage::WasmKvRuntimeTaskStore::new(storage_prefix).map_err(
                |e| {
                    crate::error::SdkError::agent_initialization(format!(
                        "runtime KV storage init failed: {}",
                        e
                    ))
                },
            )?,
        ));
    }

    #[cfg(all(not(target_arch = "wasm32"), feature = "redis-storage"))]
    {
        let storage_mode = std::env::var("PF_TASK_STORAGE_MODE")
            .unwrap_or_else(|_| "best_effort".to_string())
            .to_ascii_lowercase();
        let require_shared_storage = storage_mode == "required";

        if let Ok(url) = std::env::var("PF_TASK_VALKEY_URL") {
            match crate::redis_task_storage::RedisRuntimeTaskStore::new(&url, storage_prefix) {
                Ok(runtime_store) => return Ok(Arc::new(runtime_store)),
                Err(e) => {
                    if require_shared_storage {
                        return Err(crate::error::SdkError::agent_initialization(format!(
                            "PF_TASK_STORAGE_MODE=required but RedisRuntimeTaskStore init failed: {}",
                            e
                        )));
                    }
                    log::warn!(
                        "Redis runtime task store init failed ({}), falling back to in-memory",
                        e
                    );
                }
            }
        } else if require_shared_storage {
            return Err(crate::error::SdkError::agent_initialization(
                "PF_TASK_STORAGE_MODE=required but PF_TASK_VALKEY_URL is not set",
            ));
        } else {
            log::debug!("PF_TASK_VALKEY_URL not set; using in-memory runtime task store");
        }
    }

    #[cfg(not(all(target_arch = "wasm32", feature = "a2a-server")))]
    {
        let _ = storage_prefix;
        Ok(Arc::new(InMemoryRuntimeTaskStore::new()))
    }
}
