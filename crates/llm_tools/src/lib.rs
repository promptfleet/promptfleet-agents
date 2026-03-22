use std::collections::HashMap;
use std::sync::Arc;

pub use paste::paste; // re-export for downstream macro use

/// Type-erased context-aware executor.
/// The `Box<dyn Any + Send>` carries a `ToolContext` at runtime.
pub type ContextExecFn =
    dyn Fn(serde_json::Value, Box<dyn std::any::Any + Send>) -> serde_json::Value + Send + Sync;

#[derive(Clone)]
pub struct RegisteredTool {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
    pub exec: fn(serde_json::Value) -> serde_json::Value,
    pub needs_context: bool,
    pub context_exec: Option<Arc<ContextExecFn>>,
}

#[derive(Clone, Default)]
pub struct ToolRegistry {
    pub by_name: HashMap<String, RegisteredTool>,
}

impl ToolRegistry {
    pub fn schemas(&self) -> Vec<llm_client::types::ToolSchema> {
        self.by_name
            .values()
            .map(|t| llm_client::types::ToolSchema {
                name: t.name.clone(),
                description: if t.description.is_empty() {
                    None
                } else {
                    Some(t.description.clone())
                },
                parameters: t.parameters.clone(),
                strict: Some(true),
            })
            .collect()
    }

    pub fn exec(&self, name: &str, args: serde_json::Value) -> serde_json::Value {
        match self.by_name.get(name) {
            Some(t) => (t.exec)(args),
            None => serde_json::json!({"error": format!("unknown tool: {}", name)}),
        }
    }

    pub fn exec_call(&self, call: &llm_client::types::ToolCall) -> serde_json::Value {
        self.exec(&call.name, call.arguments.clone())
    }

    pub fn names(&self) -> Vec<String> {
        self.by_name.keys().cloned().collect()
    }
}

#[macro_export]
macro_rules! registry_from {
	( $( $tool:ident ),* $(,)? ) => {{
		let mut map: ::std::collections::HashMap<String, $crate::RegisteredTool> = ::std::collections::HashMap::new();
		$(
			$crate::paste! {
				let (name, desc, schema) = [<$tool _llm_tool_info>]();
				let needs_ctx = [<$tool _llm_tool_needs_context>]();
				let ctx_exec: ::std::sync::Arc<$crate::ContextExecFn> =
					::std::sync::Arc::new([<$tool _llm_tool_exec_ctx>]);
				let reg = $crate::RegisteredTool {
					name: name.clone(),
					description: desc.clone(),
					parameters: schema,
					exec: [<$tool _llm_tool_exec>],
					needs_context: needs_ctx,
					context_exec: Some(ctx_exec),
				};
				map.insert(name, reg);
			}
		)*
		$crate::ToolRegistry { by_name: map }
	}};
}

pub mod tool_choice {
    pub fn auto() -> serde_json::Value {
        serde_json::json!("auto")
    }
    pub fn none() -> serde_json::Value {
        serde_json::json!("none")
    }
    pub fn function(name: &str) -> serde_json::Value {
        serde_json::json!({
            "type": "function",
            "function": { "name": name }
        })
    }
}
