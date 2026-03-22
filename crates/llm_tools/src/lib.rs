//! # llm_tools
//!
//! Lightweight, WASM-safe tool registry for LLM function-calling.
//!
//! This crate provides [`ToolRegistry`] — a name-indexed collection of
//! [`RegisteredTool`]s — together with the [`registry_from!`] macro that
//! collects tools at compile time.
//!
//! ## How it works
//!
//! 1. Annotate a plain Rust function with `#[llm_tool]` (from the
//!    [`llm_tool_macros`] crate). The proc macro generates companion
//!    functions (`<fn>_llm_tool_info`, `<fn>_llm_tool_exec`, etc.) that
//!    expose the tool's JSON Schema and executor.
//!
//! 2. Use [`registry_from!`] to sweep those generated functions into a
//!    [`ToolRegistry`]:
//!
//!    ```rust,ignore
//!    use llm_tools::registry_from;
//!
//!    let registry = registry_from!(get_weather, web_search);
//!    let schemas  = registry.schemas();   // Vec<ToolSchema> for the LLM
//!    let result   = registry.exec("get_weather", args);
//!    ```
//!
//! 3. Pass `registry.schemas()` to the LLM request and route tool calls
//!    back through `registry.exec()` or `registry.exec_call()`.
//!
//! The `tool_choice` submodule provides helpers for constructing the
//! `tool_choice` field accepted by most LLM APIs.

use std::collections::HashMap;
use std::sync::Arc;

pub use paste::paste;

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

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_exec(args: serde_json::Value) -> serde_json::Value {
        serde_json::json!({ "echo": args })
    }

    fn make_registry() -> ToolRegistry {
        let mut reg = ToolRegistry::default();
        reg.by_name.insert(
            "add".to_string(),
            RegisteredTool {
                name: "add".to_string(),
                description: "Add two numbers".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "a": { "type": "number" },
                        "b": { "type": "number" }
                    },
                    "required": ["a", "b"]
                }),
                exec: dummy_exec,
                needs_context: false,
                context_exec: None,
            },
        );
        reg.by_name.insert(
            "greet".to_string(),
            RegisteredTool {
                name: "greet".to_string(),
                description: String::new(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "name": { "type": "string" }
                    },
                    "required": ["name"]
                }),
                exec: dummy_exec,
                needs_context: false,
                context_exec: None,
            },
        );
        reg
    }

    #[test]
    fn schemas_returns_correct_count_and_fields() {
        let reg = make_registry();
        let schemas = reg.schemas();
        assert_eq!(schemas.len(), 2);

        let add_schema = schemas.iter().find(|s| s.name == "add").unwrap();
        assert_eq!(add_schema.description.as_deref(), Some("Add two numbers"));
        assert_eq!(add_schema.strict, Some(true));

        // Empty description → None
        let greet_schema = schemas.iter().find(|s| s.name == "greet").unwrap();
        assert!(greet_schema.description.is_none());
    }

    #[test]
    fn exec_with_valid_tool_returns_result() {
        let reg = make_registry();
        let result = reg.exec("add", serde_json::json!({"a": 1, "b": 2}));
        assert_eq!(result["echo"]["a"], 1);
        assert_eq!(result["echo"]["b"], 2);
    }

    #[test]
    fn exec_with_unknown_tool_returns_error() {
        let reg = make_registry();
        let result = reg.exec("nonexistent", serde_json::json!({}));
        let err = result["error"].as_str().unwrap();
        assert!(err.contains("unknown tool"), "got: {err}");
    }

    #[test]
    fn exec_call_delegates_to_exec() {
        let reg = make_registry();
        let call = llm_client::types::ToolCall {
            id: None,
            call_id: None,
            name: "greet".to_string(),
            arguments: serde_json::json!({"name": "world"}),
        };
        let result = reg.exec_call(&call);
        assert_eq!(result["echo"]["name"], "world");
    }

    #[test]
    fn names_returns_all_registered() {
        let reg = make_registry();
        let mut names = reg.names();
        names.sort();
        assert_eq!(names, vec!["add", "greet"]);
    }

    #[test]
    fn empty_registry_returns_empty() {
        let reg = ToolRegistry::default();
        assert!(reg.schemas().is_empty());
        assert!(reg.names().is_empty());
        let result = reg.exec("anything", serde_json::json!({}));
        assert!(result["error"].as_str().unwrap().contains("unknown tool"));
    }

    #[test]
    fn tool_choice_auto() {
        assert_eq!(tool_choice::auto(), serde_json::json!("auto"));
    }

    #[test]
    fn tool_choice_none() {
        assert_eq!(tool_choice::none(), serde_json::json!("none"));
    }

    #[test]
    fn tool_choice_function() {
        let v = tool_choice::function("my_tool");
        assert_eq!(v["type"], "function");
        assert_eq!(v["function"]["name"], "my_tool");
    }
}
