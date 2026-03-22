use std::collections::HashMap;
use std::sync::Arc;

#[cfg(feature = "llm-engine")]
use crate::agent::tool_context::ToolContext;

#[cfg(target_arch = "wasm32")]
type ToolFuture = dyn core::future::Future<Output = Result<serde_json::Value, String>>;
#[cfg(not(target_arch = "wasm32"))]
type ToolFuture = dyn core::future::Future<Output = Result<serde_json::Value, String>> + Send;

#[derive(Clone)]
pub enum ToolExecutor {
    Simple(Arc<dyn Fn(serde_json::Value) -> std::pin::Pin<Box<ToolFuture>> + Send + Sync>),
    #[cfg(feature = "llm-engine")]
    WithContext(
        Arc<dyn Fn(serde_json::Value, ToolContext) -> std::pin::Pin<Box<ToolFuture>> + Send + Sync>,
    ),
}

impl std::fmt::Debug for ToolExecutor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Simple(_) => write!(f, "Simple(..)"),
            #[cfg(feature = "llm-engine")]
            Self::WithContext(_) => write!(f, "WithContext(..)"),
        }
    }
}

/// Minimal tool execution result for MVP
#[derive(Debug, Clone)]
pub struct ToolExecutionResult {
    pub name: String,
    pub output: serde_json::Value,
}

/// Tool specification exposed to the LLM (separate from skills)
#[derive(Clone)]
pub struct ToolSpec {
    pub name: String,
    pub description: Option<String>,
    pub parameters: serde_json::Value,
    /// Hints
    pub strict: bool,
    pub parallel_ok: bool,
    pub executor: ToolExecutor,
}

impl std::fmt::Debug for ToolSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolSpec")
            .field("name", &self.name)
            .field("strict", &self.strict)
            .field("parallel_ok", &self.parallel_ok)
            .finish()
    }
}

/// Minimal registry for LLM tools
#[derive(Default, Clone, Debug)]
pub struct ToolRegistry {
    tools: HashMap<String, ToolSpec>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    pub fn register(&mut self, tool: ToolSpec) {
        self.tools.insert(tool.name.clone(), tool);
    }

    pub fn get(&self, name: &str) -> Option<&ToolSpec> {
        self.tools.get(name)
    }

    pub fn list(&self) -> Vec<&ToolSpec> {
        self.tools.values().collect()
    }

    /// Merge another registry into this one.
    /// Later registrations overwrite earlier ones with the same name.
    pub fn merge(&mut self, other: ToolRegistry) {
        for (name, spec) in other.tools {
            self.tools.insert(name, spec);
        }
    }

    /// Number of registered tools.
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    /// Whether the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    /// Execute a tool by name (serial MVP)
    pub async fn execute(
        &self,
        name: &str,
        args: serde_json::Value,
    ) -> Result<ToolExecutionResult, String> {
        #[cfg(feature = "llm-engine")]
        {
            self.execute_inner(name, args, None).await
        }
        #[cfg(not(feature = "llm-engine"))]
        {
            self.execute_inner(name, args).await
        }
    }

    #[cfg(feature = "llm-engine")]
    pub async fn execute_with_context(
        &self,
        name: &str,
        args: serde_json::Value,
        ctx: Option<ToolContext>,
    ) -> Result<ToolExecutionResult, String> {
        self.execute_inner(name, args, ctx).await
    }

    async fn execute_inner(
        &self,
        name: &str,
        args: serde_json::Value,
        #[cfg(feature = "llm-engine")] ctx: Option<ToolContext>,
    ) -> Result<ToolExecutionResult, String> {
        let tool = self
            .tools
            .get(name)
            .ok_or_else(|| format!("Unknown tool: {}", name))?;
        let fut = match &tool.executor {
            ToolExecutor::Simple(exec) => (exec)(args),
            #[cfg(feature = "llm-engine")]
            ToolExecutor::WithContext(exec) => {
                let context = ctx.unwrap_or_default();
                (exec)(args, context)
            }
        };
        let out = fut.await?;
        Ok(ToolExecutionResult {
            name: name.to_string(),
            output: out,
        })
    }
}

/// IntoTools: ergonomic adapter for passing tools in different forms
pub trait IntoTools {
    fn into_tools(self) -> ToolRegistry;
}

impl IntoTools for ToolRegistry {
    fn into_tools(self) -> ToolRegistry {
        self
    }
}

impl IntoTools for Vec<ToolSpec> {
    fn into_tools(self) -> ToolRegistry {
        let mut reg = ToolRegistry::new();
        for t in self {
            reg.register(t);
        }
        reg
    }
}

#[cfg(feature = "llm-engine")]
impl IntoTools for llm_tools::ToolRegistry {
    fn into_tools(self) -> ToolRegistry {
        let mut reg = ToolRegistry::new();
        for name in self.names() {
            let schemas = self.schemas();
            if let Some(schema) = schemas.iter().find(|s| s.name == name) {
                let registered = self.by_name.get(&name);
                let needs_ctx = registered.map(|r| r.needs_context).unwrap_or(false);
                let ctx_exec = registered.and_then(|r| r.context_exec.clone());

                let executor = if needs_ctx {
                    if let Some(ctx_fn) = ctx_exec {
                        ToolExecutor::WithContext(Arc::new(move |args, tool_ctx| {
                            let ctx_fn = ctx_fn.clone();
                            Box::pin(async move {
                                let out = ctx_fn(args, Box::new(tool_ctx));
                                Ok(out)
                            })
                        }))
                    } else {
                        let name_clone = schema.name.clone();
                        let exec_reg = self.clone();
                        ToolExecutor::Simple(Arc::new(move |args| {
                            let reg = exec_reg.clone();
                            let tool_name = name_clone.clone();
                            Box::pin(async move { Ok(reg.exec(&tool_name, args)) })
                        }))
                    }
                } else {
                    let name_clone = schema.name.clone();
                    let exec_reg = self.clone();
                    ToolExecutor::Simple(Arc::new(move |args| {
                        let reg = exec_reg.clone();
                        let tool_name = name_clone.clone();
                        Box::pin(async move { Ok(reg.exec(&tool_name, args)) })
                    }))
                };

                let spec = ToolSpec {
                    name: schema.name.clone(),
                    description: schema.description.clone(),
                    parameters: schema.parameters.clone(),
                    strict: schema.strict.unwrap_or(true),
                    parallel_ok: false,
                    executor,
                };
                reg.register(spec);
            }
        }
        reg
    }
}

#[cfg(all(test, feature = "llm-engine"))]
mod tests {
    use super::*;
    use crate::agent::trace::AgentTraceEvent;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn execute_with_context_invokes_context_executor() {
        let mut registry = ToolRegistry::new();
        let seen = Arc::new(AtomicUsize::new(0));
        let seen2 = seen.clone();
        registry.register(ToolSpec {
            name: "ctx_tool".to_string(),
            description: None,
            parameters: serde_json::json!({"type":"object"}),
            strict: false,
            parallel_ok: false,
            executor: ToolExecutor::WithContext(Arc::new(move |_args, ctx| {
                let seen3 = seen2.clone();
                Box::pin(async move {
                    ctx.emit(AgentTraceEvent::ProgressUpdate {
                        message: "hello".to_string(),
                        progress_pct: None,
                        metadata: None,
                    });
                    seen3.fetch_add(1, Ordering::Relaxed);
                    Ok(serde_json::json!({"ok": true}))
                })
            })),
        });

        let out = registry
            .execute_with_context(
                "ctx_tool",
                serde_json::json!({}),
                Some(ToolContext::default()),
            )
            .await
            .expect("context tool should execute");
        assert_eq!(out.output["ok"], true);
        assert_eq!(seen.load(Ordering::Relaxed), 1);
    }
}
