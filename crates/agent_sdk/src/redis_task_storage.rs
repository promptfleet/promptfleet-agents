//! Redis/Valkey-backed TaskStorage for native multi-replica agents.
//!
//! Requires `redis-storage` feature and the `PF_TASK_VALKEY_URL` environment variable.
//! Keys are prefixed with `{tid}:{sid}:{aid}:` for tenant/space/agent isolation
//! in a shared Valkey instance (same scheme as `WasmKvTaskStorage`).

#[cfg(all(not(target_arch = "wasm32"), feature = "redis-storage"))]
mod inner {
    use std::sync::Mutex;

    use crate::agent::task_store::{ContinuationSnapshot, RuntimeTaskStore};
    use crate::error::{SdkError, SdkResult};
    use a2a_protocol_core::{
        data::message::Message,
        data::task::Task,
        services::{ConversationContext, TaskStorage},
        A2AError, A2AResult,
    };
    use redis::{Client, Commands, Connection};

    pub struct RedisTaskStorage {
        conn: Mutex<Connection>,
        prefix: String,
    }

    pub struct RedisRuntimeTaskStore {
        conn: Mutex<Connection>,
        prefix: String,
    }

    impl RedisTaskStorage {
        /// Connect to Redis/Valkey and create a prefixed task storage.
        ///
        /// # Arguments
        /// * `url` — Redis URL (e.g. `redis://host:6379`)
        /// * `prefix` — Key namespace `{tid}:{sid}:{aid}`
        pub fn new(url: &str, prefix: String) -> A2AResult<Self> {
            let client = Client::open(url)
                .map_err(|e| A2AError::internal(&format!("Redis client open failed: {}", e)))?;
            let conn = client
                .get_connection()
                .map_err(|e| A2AError::internal(&format!("Redis connect failed: {}", e)))?;
            log::info!(
                "RedisTaskStorage: connected to {} with prefix='{}'",
                url,
                prefix
            );
            Ok(Self {
                conn: Mutex::new(conn),
                prefix,
            })
        }

        // ── key helpers (same scheme as WasmKvTaskStorage) ──────────────

        fn key_task(&self, task_id: &str) -> String {
            format!("{}:task:{}", self.prefix, task_id)
        }
        fn key_ctx_index(&self, ctx_id: &str) -> String {
            format!("{}:ctx:{}:tasks", self.prefix, ctx_id)
        }
        fn key_ctx_meta(&self, ctx_id: &str) -> String {
            format!("{}:ctx:{}:meta", self.prefix, ctx_id)
        }
        fn task_key_pattern(&self) -> String {
            format!("{}:task:*", self.prefix)
        }
        fn ctx_meta_pattern(&self) -> String {
            format!("{}:ctx:*:meta", self.prefix)
        }

        // ── internal helpers ────────────────────────────────────────────

        fn conn(&self) -> A2AResult<std::sync::MutexGuard<'_, Connection>> {
            self.conn
                .lock()
                .map_err(|_| A2AError::internal("Redis connection lock poisoned"))
        }

        fn set_json<T: serde::Serialize>(&self, key: &str, value: &T) -> A2AResult<()> {
            let json = serde_json::to_string(value)
                .map_err(|e| A2AError::internal(&format!("JSON serialize failed: {}", e)))?;
            let mut c = self.conn()?;
            c.set::<_, _, ()>(key, &json)
                .map_err(|e| A2AError::internal(&format!("Redis SET failed: {}", e)))
        }

        fn get_json<T: serde::de::DeserializeOwned>(&self, key: &str) -> A2AResult<Option<T>> {
            let mut c = self.conn()?;
            let raw: Option<String> = c
                .get(key)
                .map_err(|e| A2AError::internal(&format!("Redis GET failed: {}", e)))?;
            match raw {
                None => Ok(None),
                Some(s) => {
                    let v = serde_json::from_str(&s).map_err(|e| {
                        A2AError::internal(&format!("JSON deserialize failed: {}", e))
                    })?;
                    Ok(Some(v))
                }
            }
        }

        fn del(&self, key: &str) -> A2AResult<()> {
            let mut c = self.conn()?;
            c.del::<_, ()>(key)
                .map_err(|e| A2AError::internal(&format!("Redis DEL failed: {}", e)))
        }

        /// SCAN for keys matching a pattern (safe for production, unlike KEYS).
        fn scan_keys(&self, pattern: &str) -> A2AResult<Vec<String>> {
            let mut c = self.conn()?;
            let iter: redis::Iter<'_, String> = c
                .scan_match(pattern)
                .map_err(|e| A2AError::internal(&format!("Redis SCAN failed: {}", e)))?;
            Ok(iter.collect())
        }

        fn read_ctx_ids(&self, ctx_id: &str) -> A2AResult<Vec<String>> {
            Ok(self
                .get_json::<Vec<String>>(&self.key_ctx_index(ctx_id))?
                .unwrap_or_default())
        }

        fn write_ctx_ids(&self, ctx_id: &str, ids: &[String]) -> A2AResult<()> {
            self.set_json(&self.key_ctx_index(ctx_id), &ids)
        }

        fn upsert_context_meta(&self, ctx_id: &str) -> A2AResult<()> {
            let key = self.key_ctx_meta(ctx_id);
            let mut meta = self
                .get_json::<ConversationContext>(&key)?
                .unwrap_or_else(|| ConversationContext::new(ctx_id.to_string()));
            meta.update_activity();
            self.set_json(&key, &meta)
        }
    }

    impl RedisRuntimeTaskStore {
        /// Connect to Redis/Valkey and create a prefixed runtime task snapshot store.
        pub fn new(url: &str, prefix: String) -> SdkResult<Self> {
            let client = Client::open(url).map_err(|e| {
                SdkError::agent_initialization(format!("Redis client open failed: {}", e))
            })?;
            let conn = client.get_connection().map_err(|e| {
                SdkError::agent_initialization(format!("Redis connect failed: {}", e))
            })?;
            Ok(Self {
                conn: Mutex::new(conn),
                prefix,
            })
        }

        fn key_runtime_latest(&self, context_id: &str) -> String {
            format!("{}:runtime:ctx:{}:latest", self.prefix, context_id)
        }

        fn key_runtime_revision(&self, task_id: &str) -> String {
            format!("{}:runtime:task:{}:revision", self.prefix, task_id)
        }

        fn conn(&self) -> SdkResult<std::sync::MutexGuard<'_, Connection>> {
            self.conn.lock().map_err(|_| {
                SdkError::method_execution(
                    "runtime_task_store",
                    "Redis runtime store lock poisoned",
                )
            })
        }

        fn set_json<T: serde::Serialize>(&self, key: &str, value: &T) -> SdkResult<()> {
            let json = serde_json::to_string(value).map_err(|e| {
                SdkError::method_execution(
                    "runtime_task_store",
                    format!("JSON serialize failed: {}", e),
                )
            })?;
            let mut c = self.conn()?;
            c.set::<_, _, ()>(key, &json).map_err(|e| {
                SdkError::method_execution("runtime_task_store", format!("Redis SET failed: {}", e))
            })
        }

        fn get_json<T: serde::de::DeserializeOwned>(&self, key: &str) -> SdkResult<Option<T>> {
            let mut c = self.conn()?;
            let raw: Option<String> = c.get(key).map_err(|e| {
                SdkError::method_execution("runtime_task_store", format!("Redis GET failed: {}", e))
            })?;
            match raw {
                None => Ok(None),
                Some(s) => serde_json::from_str(&s).map(Some).map_err(|e| {
                    SdkError::method_execution(
                        "runtime_task_store",
                        format!("JSON deserialize failed: {}", e),
                    )
                }),
            }
        }

        fn del(&self, key: &str) -> SdkResult<()> {
            let mut c = self.conn()?;
            c.del::<_, ()>(key).map_err(|e| {
                SdkError::method_execution("runtime_task_store", format!("Redis DEL failed: {}", e))
            })
        }
    }

    impl std::fmt::Debug for RedisTaskStorage {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("RedisTaskStorage")
                .field("prefix", &self.prefix)
                .finish()
        }
    }

    impl RuntimeTaskStore for RedisRuntimeTaskStore {
        fn get_latest_snapshot(&self, context_id: &str) -> SdkResult<Option<ContinuationSnapshot>> {
            self.get_json(&self.key_runtime_latest(context_id))
        }

        fn set_latest_snapshot(
            &self,
            context_id: &str,
            snapshot: Option<ContinuationSnapshot>,
        ) -> SdkResult<()> {
            let key = self.key_runtime_latest(context_id);
            match snapshot {
                Some(snapshot) => self.set_json(&key, &snapshot),
                None => self.del(&key),
            }
        }

        fn get_task_revision(&self, task_id: &str) -> SdkResult<Option<u64>> {
            self.get_json(&self.key_runtime_revision(task_id))
        }

        fn set_task_revision(&self, task_id: &str, revision: u64) -> SdkResult<()> {
            self.set_json(&self.key_runtime_revision(task_id), &revision)
        }

        fn delete_task_revision(&self, task_id: &str) -> SdkResult<()> {
            self.del(&self.key_runtime_revision(task_id))
        }
    }

    impl TaskStorage for RedisTaskStorage {
        fn store_task(&self, task: Task) -> A2AResult<()> {
            let tkey = self.key_task(&task.id);
            self.set_json(&tkey, &task)?;

            // Update context index
            let mut ids = self.read_ctx_ids(&task.context_id)?;
            if !ids.iter().any(|id| id == &task.id) {
                ids.push(task.id.clone());
                self.write_ctx_ids(&task.context_id, &ids)?;
            }

            // Update context meta
            self.upsert_context_meta(&task.context_id)?;
            Ok(())
        }

        fn get_task(&self, task_id: &str) -> A2AResult<Option<Task>> {
            self.get_json(&self.key_task(task_id))
        }

        fn update_task(&self, task: Task) -> A2AResult<()> {
            self.store_task(task)
        }

        fn list_tasks(&self) -> A2AResult<Vec<Task>> {
            let keys = self.scan_keys(&self.task_key_pattern())?;
            let mut out = Vec::with_capacity(keys.len());
            for k in &keys {
                if let Some(t) = self.get_json::<Task>(k)? {
                    out.push(t);
                }
            }
            Ok(out)
        }

        fn remove_task(&self, task_id: &str) -> A2AResult<bool> {
            if let Some(task) = self.get_task(task_id)? {
                let mut ids = self.read_ctx_ids(&task.context_id)?;
                ids.retain(|id| id != task_id);
                self.write_ctx_ids(&task.context_id, &ids)?;
            }
            self.del(&self.key_task(task_id))?;
            Ok(true)
        }

        fn task_exists(&self, task_id: &str) -> A2AResult<bool> {
            let mut c = self.conn()?;
            let exists: bool = c
                .exists(&self.key_task(task_id))
                .map_err(|e| A2AError::internal(&format!("Redis EXISTS failed: {}", e)))?;
            Ok(exists)
        }

        fn get_tasks_by_context(&self, context_id: &str) -> A2AResult<Vec<Task>> {
            let ids = self.read_ctx_ids(context_id)?;
            let mut out = Vec::with_capacity(ids.len());
            for id in &ids {
                if let Some(t) = self.get_task(id)? {
                    out.push(t);
                }
            }
            Ok(out)
        }

        fn get_latest_task_in_context(&self, context_id: &str) -> A2AResult<Option<Task>> {
            let ids = self.read_ctx_ids(context_id)?;
            match ids.last() {
                Some(id) => self.get_task(id),
                None => Ok(None),
            }
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
            if let Some(meta) = self.get_json::<ConversationContext>(&key)? {
                return Ok(meta);
            }
            let meta = ConversationContext::new(context_id.to_string());
            self.set_json(&key, &meta)?;
            Ok(meta)
        }

        fn update_context_activity(&self, context_id: &str) -> A2AResult<()> {
            self.upsert_context_meta(context_id)
        }

        fn list_contexts(&self) -> A2AResult<Vec<ConversationContext>> {
            let keys = self.scan_keys(&self.ctx_meta_pattern())?;
            let mut out = Vec::with_capacity(keys.len());
            for k in &keys {
                if let Some(meta) = self.get_json::<ConversationContext>(k)? {
                    out.push(meta);
                }
            }
            Ok(out)
        }
    }
}

#[cfg(all(not(target_arch = "wasm32"), feature = "redis-storage"))]
pub use inner::RedisRuntimeTaskStore;
#[cfg(all(not(target_arch = "wasm32"), feature = "redis-storage"))]
pub use inner::RedisTaskStorage;
