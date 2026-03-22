#[cfg(target_arch = "wasm32")]
use spin_sdk::key_value::Store;

#[cfg(target_arch = "wasm32")]
use crate::agent::task_store::{ContinuationSnapshot, RuntimeTaskStore};
#[cfg(target_arch = "wasm32")]
use crate::error::{SdkError, SdkResult};
#[cfg(target_arch = "wasm32")]
use a2a_protocol_core::{
    data::message::Message,
    data::task::Task,
    services::{ConversationContext, TaskStorage},
    A2AError, A2AResult,
};

#[cfg(target_arch = "wasm32")]
pub struct WasmKvTaskStorage {
    store: Store,
    /// Key prefix for tenant/space/agent isolation: `{tid}:{sid}:{aid}`
    prefix: String,
}

#[cfg(target_arch = "wasm32")]
pub struct WasmKvRuntimeTaskStore {
    store: Store,
    prefix: String,
}

#[cfg(target_arch = "wasm32")]
impl WasmKvTaskStorage {
    /// Create storage with a key prefix for isolation in shared Valkey.
    ///
    /// All keys are prefixed with `{prefix}:` to ensure tenant, space,
    /// and agent isolation when multiple agents share the same backing store.
    pub fn new(prefix: String) -> A2AResult<Self> {
        let store = Store::open_default()
            .map_err(|e| A2AError::internal(&format!("KV open_default failed: {}", e)))?;
        log::info!(
            "WasmKvTaskStorage: opened default store with prefix='{}'",
            prefix
        );
        Ok(Self { store, prefix })
    }

    /// Backward-compatible constructor (uses "local" prefix).
    pub fn new_default() -> A2AResult<Self> {
        Self::new("local:dev:default".to_string())
    }

    fn key_task(&self, task_id: &str) -> String {
        format!("{}:task:{}", self.prefix, task_id)
    }
    fn key_ctx_index(&self, ctx_id: &str) -> String {
        format!("{}:ctx:{}:tasks", self.prefix, ctx_id)
    }
    fn key_ctx_meta(&self, ctx_id: &str) -> String {
        format!("{}:ctx:{}:meta", self.prefix, ctx_id)
    }

    /// Key prefix for filtering in list operations
    fn task_key_prefix(&self) -> String {
        format!("{}:task:", self.prefix)
    }
    fn ctx_meta_prefix(&self) -> String {
        format!("{}:ctx:", self.prefix)
    }

    fn read_ctx_ids(&self, ctx_id: &str) -> Vec<String> {
        let key = self.key_ctx_index(ctx_id);
        match self.store.get_json::<Vec<String>>(&key) {
            Ok(Some(ids)) => ids,
            _ => Vec::new(),
        }
    }

    fn write_ctx_ids(&self, ctx_id: &str, ids: &Vec<String>) -> A2AResult<()> {
        let key = self.key_ctx_index(ctx_id);
        self.store
            .set_json(&key, ids)
            .map_err(|e| A2AError::internal(&format!("KV set_json failed: {}", e)))
    }

    fn upsert_context_meta(&self, ctx_id: &str) -> A2AResult<()> {
        let key = self.key_ctx_meta(ctx_id);
        let mut meta = match self.store.get_json::<ConversationContext>(&key) {
            Ok(Some(m)) => m,
            _ => ConversationContext::new(ctx_id.to_string()),
        };
        meta.update_activity();
        self.store
            .set_json(&key, &meta)
            .map_err(|e| A2AError::internal(&format!("KV set_json meta failed: {}", e)))
    }
}

#[cfg(target_arch = "wasm32")]
impl WasmKvRuntimeTaskStore {
    pub fn new(prefix: String) -> SdkResult<Self> {
        let store = Store::open_default().map_err(|e| {
            SdkError::agent_initialization(format!("KV open_default failed: {}", e))
        })?;
        Ok(Self { store, prefix })
    }

    fn key_runtime_latest(&self, ctx_id: &str) -> String {
        format!("{}:runtime:ctx:{}:latest", self.prefix, ctx_id)
    }

    fn key_task_revision(&self, task_id: &str) -> String {
        format!("{}:runtime:task:{}:revision", self.prefix, task_id)
    }
}

#[cfg(target_arch = "wasm32")]
impl RuntimeTaskStore for WasmKvRuntimeTaskStore {
    fn get_latest_snapshot(&self, context_id: &str) -> SdkResult<Option<ContinuationSnapshot>> {
        self.store
            .get_json::<ContinuationSnapshot>(&self.key_runtime_latest(context_id))
            .map_err(|e| {
                SdkError::method_execution(
                    "runtime_task_store",
                    format!("KV get_json continuation snapshot failed: {}", e),
                )
            })
    }

    fn set_latest_snapshot(
        &self,
        context_id: &str,
        snapshot: Option<ContinuationSnapshot>,
    ) -> SdkResult<()> {
        let key = self.key_runtime_latest(context_id);
        match snapshot {
            Some(snapshot) => self.store.set_json(&key, &snapshot).map_err(|e| {
                SdkError::method_execution(
                    "runtime_task_store",
                    format!("KV set_json continuation snapshot failed: {}", e),
                )
            }),
            None => self.store.delete(&key).map_err(|e| {
                SdkError::method_execution(
                    "runtime_task_store",
                    format!("KV delete continuation snapshot failed: {}", e),
                )
            }),
        }
    }

    fn get_task_revision(&self, task_id: &str) -> SdkResult<Option<u64>> {
        self.store
            .get_json::<u64>(&self.key_task_revision(task_id))
            .map_err(|e| {
                SdkError::method_execution(
                    "runtime_task_store",
                    format!("KV get_json task revision failed: {}", e),
                )
            })
    }

    fn set_task_revision(&self, task_id: &str, revision: u64) -> SdkResult<()> {
        self.store
            .set_json(&self.key_task_revision(task_id), &revision)
            .map_err(|e| {
                SdkError::method_execution(
                    "runtime_task_store",
                    format!("KV set_json task revision failed: {}", e),
                )
            })
    }

    fn delete_task_revision(&self, task_id: &str) -> SdkResult<()> {
        self.store
            .delete(&self.key_task_revision(task_id))
            .map_err(|e| {
                SdkError::method_execution(
                    "runtime_task_store",
                    format!("KV delete task revision failed: {}", e),
                )
            })
    }
}

#[cfg(target_arch = "wasm32")]
impl TaskStorage for WasmKvTaskStorage {
    fn store_task(&self, task: Task) -> A2AResult<()> {
        let tkey = self.key_task(&task.id);
        log::trace!(
            "KV store_task: about to write key={} ctx={}",
            tkey,
            task.context_id
        );
        self.store
            .set_json(&tkey, &task)
            .map_err(|e| A2AError::internal(&format!("KV set_json task failed: {}", e)))?;

        // immediate read-back sanity
        match self.store.get_json::<Task>(&tkey) {
            Ok(Some(_)) => log::debug!("KV store_task: verified write for key={}", tkey),
            Ok(None) => log::warn!(
                "KV store_task: write not visible immediately for key={}",
                tkey
            ),
            Err(e) => log::warn!("KV store_task: read-back error for key={} err={}", tkey, e),
        }

        // Update context index
        let mut ids = self.read_ctx_ids(&task.context_id);
        if !ids.iter().any(|id| id == &task.id) {
            ids.push(task.id.clone());
            self.write_ctx_ids(&task.context_id, &ids)?;
        }

        // Update context meta
        self.upsert_context_meta(&task.context_id)?;

        // keys snapshot
        if let Ok(keys) = self.store.get_keys() {
            let prefix = self.task_key_prefix();
            let own_count = keys.iter().filter(|k| k.starts_with(&prefix)).count();
            log::trace!(
                "KV store_task: total_keys={} own_tasks={}",
                keys.len(),
                own_count,
            );
        }

        Ok(())
    }

    fn get_task(&self, task_id: &str) -> A2AResult<Option<Task>> {
        let key = self.key_task(task_id);
        log::trace!("KV get_task: reading key={}", key);
        self.store
            .get_json::<Task>(&key)
            .map_err(|e| A2AError::internal(&format!("KV get_json task failed: {}", e)))
    }

    fn update_task(&self, task: Task) -> A2AResult<()> {
        self.store_task(task)
    }

    fn list_tasks(&self) -> A2AResult<Vec<Task>> {
        let keys = self
            .store
            .get_keys()
            .map_err(|e| A2AError::internal(&format!("KV get_keys failed: {}", e)))?;
        let prefix = self.task_key_prefix();
        log::trace!(
            "KV list_tasks: total_keys={} filtering prefix='{}'",
            keys.len(),
            prefix
        );
        let mut out = Vec::new();
        for k in keys.iter().filter(|k| k.starts_with(&prefix)) {
            if let Ok(Some(t)) = self.store.get_json::<Task>(k) {
                out.push(t);
            }
        }
        Ok(out)
    }

    fn remove_task(&self, task_id: &str) -> A2AResult<bool> {
        if let Some(task) = self.get_task(task_id)? {
            let mut ids = self.read_ctx_ids(&task.context_id);
            ids.retain(|id| id != task_id);
            self.write_ctx_ids(&task.context_id, &ids)?;
        }
        let key = self.key_task(task_id);
        self.store
            .delete(&key)
            .map_err(|e| A2AError::internal(&format!("KV delete failed: {}", e)))?;
        Ok(true)
    }

    fn task_exists(&self, task_id: &str) -> A2AResult<bool> {
        Ok(self.get_task(task_id)?.is_some())
    }

    fn get_tasks_by_context(&self, context_id: &str) -> A2AResult<Vec<Task>> {
        let ids = self.read_ctx_ids(context_id);
        let mut out = Vec::new();
        for id in ids {
            if let Some(t) = self.get_task(&id)? {
                out.push(t);
            }
        }
        Ok(out)
    }

    fn get_latest_task_in_context(&self, context_id: &str) -> A2AResult<Option<Task>> {
        let ids = self.read_ctx_ids(context_id);
        if let Some(last) = ids.last() {
            return self.get_task(last);
        }
        Ok(None)
    }

    fn get_context_history(&self, context_id: &str) -> A2AResult<Vec<Message>> {
        let tasks = self.get_tasks_by_context(context_id)?;
        let mut history = Vec::new();
        for mut t in tasks {
            if let Some(h) = t.history.take() {
                history.extend(h);
            }
        }
        Ok(history)
    }

    fn get_or_create_context(&self, context_id: &str) -> A2AResult<ConversationContext> {
        let key = self.key_ctx_meta(context_id);
        if let Ok(Some(meta)) = self.store.get_json::<ConversationContext>(&key) {
            return Ok(meta);
        }
        let meta = ConversationContext::new(context_id.to_string());
        self.store
            .set_json(&key, &meta)
            .map_err(|e| A2AError::internal(&format!("KV set_json meta failed: {}", e)))?;
        Ok(meta)
    }

    fn update_context_activity(&self, context_id: &str) -> A2AResult<()> {
        self.upsert_context_meta(context_id)
    }

    fn list_contexts(&self) -> A2AResult<Vec<ConversationContext>> {
        let keys = self
            .store
            .get_keys()
            .map_err(|e| A2AError::internal(&format!("KV get_keys failed: {}", e)))?;
        let prefix = self.ctx_meta_prefix();
        let mut out = Vec::new();
        for k in keys
            .iter()
            .filter(|k| k.starts_with(&prefix) && k.ends_with(":meta"))
        {
            if let Ok(Some(meta)) = self.store.get_json::<ConversationContext>(k) {
                out.push(meta);
            }
        }
        Ok(out)
    }
}
