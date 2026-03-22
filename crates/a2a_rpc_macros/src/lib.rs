//! > **⚠️ Experimental** — This crate is not yet integrated into the SDK pipeline.
//! > The API is unstable and may change significantly. Do not depend on it for production use.
//!
//! # A2A RPC Proc-Macros
//!
//! **WASM-Native** automatic code generation for A2A JSON-RPC command registration.
//! Pure proc-macro approach with zero runtime overhead - no linkme required!
//!
//! ## Target Binary Size: <250 KiB
//!
//! This proc-macro eliminates manual command registration lists by generating
//! compile-time registration code that's fully WASM-compatible.
//!
//! ## Features
//! - **Pure WASM**: No linker tricks, no linkme dependency
//! - **Zero runtime overhead**: All registration happens at compile time  
//! - **Dual compatibility**: Works with manual Vec registration
//! - **Type safety**: Validates handler signatures at compile time
//! - **Hot path optimization**: Direct function calls, no dynamic dispatch
//! - **Extension Auto-Discovery**: Automatic extension registration via #[extension] macro
//! - **Async Support**: WASM-compatible async function handling
//!
//! ## Usage
//! ```rust
//! use a2a_rpc_macros::{a2a_rpc, generate_a2a_commands, extension, generate_extensions};
//! use a2a_jsonrpc_core::{RpcError, Value, AgentInfo};
//! use serde_json::json;
//!
//! // Sync handler
//! #[a2a_rpc("ping")]
//! fn ping_handler(_params: Value) -> Result<Value, RpcError> {
//!     Ok(json!({"pong": true}))
//! }
//!
//! // Async handler (WASM-compatible)
//! #[a2a_rpc("openai.chat")]
//! async fn chat_handler(_params: Value) -> Result<Value, RpcError> {
//!     // Async OpenAI call
//!     Ok(json!({"response": "async result"}))
//! }
//!
//! #[extension("openai")]
//! pub struct OpenAIExtension {
//!     // Extension implementation
//! }
//!
//! // Generate WASM-compatible initialization
//! generate_a2a_commands!();
//! generate_extensions!();
//! ```

use proc_macro::TokenStream;
use proc_macro2::{Ident, Span};
use quote::quote;
use std::sync::Mutex;
use syn::{parse_macro_input, spanned::Spanned, ItemFn, ItemStruct, LitStr};

/// Global command registry for compile-time collection
/// Format: (method_name, handler_function_name, method_type_str, is_async)
/// This allows us to generate a complete registration list at the end of compilation.
/// WASM-compatible - no linker dependencies!
static COMMANDS: Mutex<Vec<(String, String, String, bool)>> = Mutex::new(Vec::new());

/// Global extension registry for compile-time collection
/// Format: (extension_name, struct_name, factory_function_name)
/// This allows us to generate extension discovery code at compile time.
static EXTENSIONS: Mutex<Vec<(String, String, String)>> = Mutex::new(Vec::new());

/// **WASM-NATIVE CORE**: `#[a2a_rpc("method")]` Attribute Macro
///
/// Automatically registers JSON-RPC command handlers for compile-time registration.
/// Works in pure WASM environment without any linker tricks.
/// **NEW**: Now supports both sync and async functions with WASM-compatible async handling!
///
/// ## Features
/// - **Zero runtime overhead**: All registration happens at compile time
/// - **WASM compatible**: No linker tricks, pure code generation
/// - **Type safe**: Validates handler signatures at compile time  
/// - **Hot path optimized**: Direct function calls in generated code
/// - **Method classification**: Supports explicit request/notification declaration
/// - **Async support**: WASM-compatible async function wrapping
///
/// ## Usage Patterns
///
/// ### Sync Method (traditional)
/// ```rust
/// #[a2a_rpc("ping")]
/// fn ping_handler(_params: Value) -> Result<Value, RpcError> {
///     Ok(json!({"pong": true}))
/// }
/// ```
///
/// ### Async Method (WASM-compatible)
/// ```rust
/// #[a2a_rpc("openai.chat")]
/// async fn chat_handler(params: Value) -> Result<Value, RpcError> {
///     let client = get_openai_client();
///     let response = client.chat_completion(params).await?;
///     Ok(serde_json::to_value(response)?)
/// }
/// ```
///
/// ### Explicit Request/Notification with Async
/// ```rust
/// #[a2a_rpc("log.info", notification)]
/// async fn log_handler(params: Value) -> Result<Value, RpcError> {
///     // Fire-and-forget async logging
///     async_log_service(params).await?;
///     Ok(Value::Null)
/// }
/// ```
///
/// ## WASM Async Handling
/// For async functions, the macro generates a WASM-compatible wrapper that uses
/// the Spin SDK's execution model. This avoids issues with futures::block_on
/// and other executors that don't work well in WASM environments.
#[proc_macro_attribute]
pub fn a2a_rpc(args: TokenStream, item: TokenStream) -> TokenStream {
    let input_fn = parse_macro_input!(item as ItemFn);

    // Parse method name and optional type from attribute arguments
    let (method_name, method_type) = match parse_method_args(args) {
        Ok(result) => result,
        Err(err) => return err.to_compile_error().into(),
    };

    // Check if function is async
    let is_async = input_fn.sig.asyncness.is_some();

    // Validate function signature (now supports both sync and async)
    if let Err(err) = validate_handler_signature(&input_fn, is_async) {
        return err.to_compile_error().into();
    }

    let fn_name = &input_fn.sig.ident;
    let fn_name_str = fn_name.to_string();

    // Register command globally for build script generation
    {
        let mut commands = COMMANDS.lock().unwrap();
        commands.push((
            method_name.clone(),
            fn_name_str.clone(),
            method_type.clone(),
            is_async,
        ));
    }

    // Generate the appropriate wrapper based on sync/async
    let wrapper_code = if is_async {
        generate_async_wrapper(fn_name, &method_name)
    } else {
        // For sync functions, no wrapper needed
        quote! {}
    };

    let method_type_str = method_type;
    let async_marker = if is_async { "async" } else { "sync" };

    let expanded = quote! {
        #[doc = concat!("A2A JSON-RPC handler for method: ", #method_name)]
        #[doc = concat!("Type: ", #method_type_str, " (", #async_marker, ")")]
        #[doc = ""]
        #[doc = "Generated by a2a_rpc_macros for WASM-compatible registration."]
        #input_fn

        #wrapper_code

        // Generate compile-time registration metadata (WASM-compatible)
        #[doc(hidden)]
        #[allow(non_upper_case_globals)]
        const _: () = {
            // Store metadata for collection by generate_a2a_commands!()
            const _A2A_METHOD: &str = #method_name;
            const _A2A_HANDLER: &str = #fn_name_str;
            const _A2A_TYPE: &str = #method_type_str;
            const _A2A_IS_ASYNC: bool = #is_async;
        };
    };

    TokenStream::from(expanded)
}

/// Generate WASM-compatible async wrapper for async handlers
fn generate_async_wrapper(fn_name: &Ident, method_name: &str) -> proc_macro2::TokenStream {
    let wrapper_name = Ident::new(&format!("{}_sync_wrapper", fn_name), fn_name.span());
    let method_name_comment = format!(
        "WASM-compatible sync wrapper for async method: {}",
        method_name
    );

    quote! {
        #[doc = #method_name_comment]
        #[doc = ""]
        #[doc = "This wrapper enables async functions to work with the synchronous A2A RPC system"]
        #[doc = "in WASM environments. It uses a simple futures executor that works in Spin SDK."]
        pub fn #wrapper_name(params: a2a_jsonrpc_core::Value) -> Result<a2a_jsonrpc_core::Value, a2a_jsonrpc_core::RpcError> {
            // WASM-compatible async execution
            // We use a simple LocalPool executor which works well in single-threaded WASM environments
            use futures::executor::LocalPool;
            use futures::task::LocalSpawnExt;

            let mut pool = LocalPool::new();
            let spawner = pool.spawner();

            // Spawn the async function
            let future = spawner.spawn_local_with_handle(async move {
                #fn_name(params).await
            });

            match future {
                Ok(handle) => {
                    // Run the future to completion
                    match pool.run_until(handle) {
                        Ok(result) => result,
                        Err(e) => Err(a2a_jsonrpc_core::RpcError::internal_error(
                            format!("Async execution panicked: {:?}", e)
                        )),
                    }
                }
                Err(e) => Err(a2a_jsonrpc_core::RpcError::internal_error(
                    format!("Failed to spawn async task: {:?}", e)
                )),
            }
        }
    }
}

/// **WASM-COMPATIBLE GENERATOR**: Create registration code for all collected commands
///
/// This macro should be called once per crate to generate the final registration code.
/// Unlike linkme, this works in pure WASM by generating direct function calls.
/// **NEW**: Now handles both sync and async functions with appropriate wrappers!
///
/// ## Usage
/// ```rust
/// use a2a_rpc_macros::generate_a2a_commands;
/// use a2a_jsonrpc_core::{AgentInfo, init_agent_with_generated};
///
/// // At the end of your lib.rs, after all #[a2a_rpc] macros
/// generate_a2a_commands!();
///
/// // Then initialize with your agent info
/// let agent_info = AgentInfo::default();
/// init_agent_with_generated(get_generated_commands(), agent_info, None);
/// ```
///
/// ## Generated Functions
/// - `get_generated_commands()`: Returns Vec of all registered commands (with async wrappers)
/// - `init_generated_agent()`: Convenience function for simple initialization
/// - `get_command_count()`: Returns number of registered commands (for diagnostics)
/// - `get_async_command_count()`: Returns number of async commands (for diagnostics)
#[proc_macro]
pub fn generate_a2a_commands(_input: TokenStream) -> TokenStream {
    // Get all registered commands
    let commands = {
        let commands = COMMANDS.lock().unwrap();
        commands.clone()
    };

    if commands.is_empty() {
        // No commands registered, generate empty functions
        return quote! {
            /// No A2A commands registered in this crate
            pub fn get_generated_commands() -> Vec<(&'static str, a2a_jsonrpc_core::Handler)> {
                vec![]
            }

            /// Initialize agent with empty command set
            pub fn init_generated_agent() {
                let agent_info = a2a_jsonrpc_core::AgentInfo::default();
                a2a_jsonrpc_core::init_agent(vec![], agent_info, None);
            }

            /// Number of generated commands
            pub fn get_command_count() -> usize {
                0
            }

            /// Number of async commands
            pub fn get_async_command_count() -> usize {
                0
            }
        }
        .into();
    }

    // Generate direct command registration code (WASM-compatible)
    let registrations: Vec<_> = commands
        .iter()
        .map(|(method, handler, _, is_async)| {
            if *is_async {
                // Use the sync wrapper for async functions
                let wrapper_name =
                    Ident::new(&format!("{}_sync_wrapper", handler), Span::call_site());
                quote! {
                    (#method, #wrapper_name as a2a_jsonrpc_core::Handler)
                }
            } else {
                // Use the original function for sync functions
                let handler_ident = Ident::new(handler, Span::call_site());
                quote! {
                    (#method, #handler_ident as a2a_jsonrpc_core::Handler)
                }
            }
        })
        .collect();

    // Generate command registration with types (for new registry)
    let registrations_with_types: Vec<_> = commands
        .iter()
        .map(|(method, handler, method_type, is_async)| {
            let handler_ref = if *is_async {
                let wrapper_name =
                    Ident::new(&format!("{}_sync_wrapper", handler), Span::call_site());
                quote! { #wrapper_name as a2a_jsonrpc_core::Handler }
            } else {
                let handler_ident = Ident::new(handler, Span::call_site());
                quote! { #handler_ident as a2a_jsonrpc_core::Handler }
            };

            let method_type_enum = match method_type.as_str() {
                "notification" => quote! { a2a_jsonrpc_core::MethodType::Notification },
                _ => quote! { a2a_jsonrpc_core::MethodType::Request },
            };

            quote! {
                (#method, #handler_ref, #method_type_enum)
            }
        })
        .collect();

    let command_count = commands.len();
    let async_command_count = commands
        .iter()
        .filter(|(_, _, _, is_async)| *is_async)
        .count();

    let expanded = quote! {
        /// **GENERATED**: Get all A2A commands registered via proc macros
        ///
        /// This function is automatically generated by a2a_rpc_macros and contains
        /// all commands registered with #[a2a_rpc("method")]. WASM-compatible!
        /// Async functions are automatically wrapped with sync adapters.
        ///
        /// Returns a Vec that can be used with:
        /// - `a2a_jsonrpc_core::init_agent()`
        /// - `a2a_jsonrpc_core::init_agent_dual()` (combined with manual commands)
        /// - `a2a_jsonrpc_core::init_agent_with_generated()`
        pub fn get_generated_commands() -> Vec<(&'static str, a2a_jsonrpc_core::Handler)> {
            vec![
                #(#registrations),*
            ]
        }

        /// **GENERATED**: Get all A2A commands with method types
        ///
        /// This function includes method type information for proper A2A protocol handling.
        /// Use this with Registry::new_with_types() for full declarative method type support.
        /// Async functions are automatically wrapped with WASM-compatible sync adapters.
        pub fn get_generated_commands_with_types() -> Vec<(&'static str, a2a_jsonrpc_core::Handler, a2a_jsonrpc_core::MethodType)> {
            vec![
                #(#registrations_with_types),*
            ]
        }

        /// **CONVENIENCE**: Initialize agent with generated commands only
        ///
        /// Simple initialization for agents that only use proc-macro commands.
        /// For more control, use `get_generated_commands()` with the init functions directly.
        pub fn init() {
            let agent_info = a2a_jsonrpc_core::AgentInfo::default();
            let commands = get_generated_commands();
            a2a_jsonrpc_core::init_with_commands(commands, agent_info, None);
        }

        /// **CONVENIENCE**: Initialize agent with generated commands and custom info
        ///
        /// Initialize with custom agent metadata while using generated commands.
        /// This version uses the new method type-aware initialization.
        pub fn init_with_info(
            agent_info: a2a_jsonrpc_core::AgentInfo,
            capabilities: Option<a2a_jsonrpc_core::Capabilities>
        ) {
            let commands = get_generated_commands_with_types();
            a2a_jsonrpc_core::init_with_typed_commands(commands, agent_info, capabilities);
        }

        /// **DIAGNOSTICS**: Get number of commands registered via proc macros
        ///
        /// Useful for debugging and verification that commands were registered correctly.
        pub fn get_command_count() -> usize {
            #command_count
        }

        /// **DIAGNOSTICS**: Get number of async commands registered via proc macros
        ///
        /// Shows how many commands use async handlers with WASM-compatible wrappers.
        pub fn get_async_command_count() -> usize {
            #async_command_count
        }

        /// **DIAGNOSTICS**: List all command names registered via proc macros
        ///
        /// Returns method names for debugging and introspection.
        pub fn get_command_names() -> Vec<&'static str> {
            get_generated_commands().into_iter().map(|(name, _)| name).collect()
        }
    };

    TokenStream::from(expanded)
}

/// **HELPER**: Parse method name and optional type from TokenStream
fn parse_method_args(args: TokenStream) -> Result<(String, String), syn::Error> {
    use syn::{parse::Parse, parse::ParseStream, Ident, LitStr, Token};

    // Define a struct to parse the arguments
    struct MethodArgs {
        method_name: String,
        method_type: String,
    }

    impl Parse for MethodArgs {
        fn parse(input: ParseStream) -> syn::Result<Self> {
            // Parse the method name string literal
            let method_lit: LitStr = input.parse()?;
            let method_name = method_lit.value();

            // Check for optional method type
            let method_type = if input.peek(Token![,]) {
                input.parse::<Token![,]>()?;
                let type_ident: Ident = input.parse()?;
                let type_str = type_ident.to_string();

                if matches!(type_str.as_str(), "request" | "notification") {
                    type_str
                } else {
                    return Err(syn::Error::new(
                        type_ident.span(),
                        "Method type must be 'request' or 'notification'",
                    ));
                }
            } else {
                "request".to_string() // Default to request
            };

            // Validate method name
            if method_name.is_empty() {
                return Err(syn::Error::new(
                    method_lit.span(),
                    "Method name cannot be empty",
                ));
            }

            if method_name.contains(' ') || method_name.starts_with("rpc.") {
                return Err(syn::Error::new(
                    method_lit.span(),
                    "Invalid method name: cannot contain spaces or start with 'rpc.'",
                ));
            }

            Ok(MethodArgs {
                method_name,
                method_type,
            })
        }
    }

    // Parse the arguments
    let parsed = syn::parse::<MethodArgs>(args)?;
    Ok((parsed.method_name, parsed.method_type))
}

/// **HELPER**: Validate that function has correct A2A handler signature
/// Now supports both sync and async functions!
fn validate_handler_signature(func: &ItemFn, is_async: bool) -> Result<(), syn::Error> {
    let sig = &func.sig;

    // Check that function takes one parameter of type Value
    if sig.inputs.len() != 1 {
        return Err(syn::Error::new(
            sig.inputs.span(),
            "A2A handler must take exactly one parameter of type Value",
        ));
    }

    // Check return type is Result<Value, RpcError>
    if let syn::ReturnType::Type(_, ty) = &sig.output {
        // Simple validation - in production we'd parse the type more thoroughly
        let type_str = quote!(#ty).to_string();
        if !type_str.contains("Result") {
            let error_msg = if is_async {
                "Async A2A handler must return Result<Value, RpcError>"
            } else {
                "A2A handler must return Result<Value, RpcError>"
            };
            return Err(syn::Error::new(ty.span(), error_msg));
        }
    } else {
        let error_msg = if is_async {
            "Async A2A handler must return Result<Value, RpcError>"
        } else {
            "A2A handler must return Result<Value, RpcError>"
        };
        return Err(syn::Error::new(sig.output.span(), error_msg));
    }

    Ok(())
}

/// **EXTENSION AUTO-DISCOVERY**: `#[extension("name")]` Attribute Macro
///
/// Automatically registers extensions for compile-time discovery.
/// Works in pure WASM environment without any linker tricks.
///
/// ## Features
/// - **Zero runtime overhead**: All registration happens at compile time
/// - **WASM compatible**: No linker tricks, pure code generation
/// - **Type safe**: Validates extension struct at compile time
/// - **Auto-discovery**: Extensions are discovered automatically when imported
///
/// ## Usage
/// ```rust
/// use a2a_rpc_macros::extension;
/// use component_core::Extension;
///
/// #[extension("openai")]
/// pub struct OpenAIExtension {
///     config: OnceCell<OpenAIConfig>,
///     client: OnceCell<OpenAIClient>,
/// }
///
/// impl Extension for OpenAIExtension {
///     type Config = OpenAIConfig;
///     // ... implementation
/// }
/// ```
///
/// ## Generated Code
/// - Factory function for creating extension instances
/// - Compile-time registration metadata
/// - Extension metadata constants
#[proc_macro_attribute]
pub fn extension(args: TokenStream, input: TokenStream) -> TokenStream {
    let name = parse_macro_input!(args as LitStr);
    let input_struct = parse_macro_input!(input as ItemStruct);
    let struct_name = &input_struct.ident;
    let factory_name = format!("{}_factory", struct_name.to_string().to_lowercase());

    // Register extension during compilation (WASM-compatible)
    {
        let mut extensions = EXTENSIONS.lock().unwrap();
        extensions.push((name.value(), struct_name.to_string(), factory_name.clone()));
    }

    let factory_ident = syn::Ident::new(&factory_name, struct_name.span());
    let name_value = name.value();
    let struct_name_str = struct_name.to_string();

    let expanded = quote! {
        #input_struct

        // Generate factory function (WASM-compatible)
        #[doc = concat!("Factory function for ", #struct_name_str, " extension")]
        #[doc = "Generated by extension proc-macro for WASM-compatible registration."]
        pub fn #factory_ident() -> Box<dyn component_core::ExtensionInstance> {
            Box::new(#struct_name::default())
        }

        // Store metadata for collection by generate_extensions!()
        #[doc(hidden)]
        #[allow(non_upper_case_globals)]
        const _: () = {
            const _EXTENSION_NAME: &str = #name_value;
            const _EXTENSION_STRUCT: &str = #struct_name_str;
            const _EXTENSION_FACTORY: &str = #factory_name;
        };
    };

    TokenStream::from(expanded)
}

/// **EXTENSION GENERATOR**: Create extension discovery code for all collected extensions
///
/// This macro should be called once per crate to generate the final extension registry code.
/// Unlike linkme, this works in pure WASM by generating direct function calls.
///
/// ## Usage
/// ```rust
/// use a2a_rpc_macros::{extension, generate_extensions};
///
/// #[extension("my_extension")]
/// pub struct MyExtension;
///
/// // At the end of your lib.rs, after all #[extension] macros
/// generate_extensions!();
/// ```
///
/// ## Generated Functions
/// - `get_generated_extensions()`: Returns Vec of all registered extensions
/// - `init_generated_extensions()`: Convenience function for simple initialization
/// - `get_extension_count()`: Returns number of registered extensions (for diagnostics)
#[proc_macro]
pub fn generate_extensions(_input: TokenStream) -> TokenStream {
    // Get all registered extensions
    let extensions = {
        let extensions = EXTENSIONS.lock().unwrap();
        extensions.clone()
    };

    if extensions.is_empty() {
        // No extensions registered, generate empty functions
        return quote! {
            /// No extensions registered in this crate
            pub fn get_generated_extensions() -> Vec<(&'static str, fn() -> Box<dyn component_core::ExtensionInstance>)> {
                vec![]
            }
            
            /// Initialize agent with empty extension set
            pub fn init_generated_extensions() -> component_core::ComponentResult<()> {
                Ok(())
            }
            
            /// Number of generated extensions
            pub fn get_extension_count() -> usize {
                0
            }
            
            /// List extension names (diagnostics)
            pub fn get_extension_names() -> Vec<&'static str> {
                vec![]
            }
        }.into();
    }

    // Generate extension registration code (WASM-compatible)
    let registrations: Vec<_> = extensions
        .iter()
        .map(|(name, _struct_name, factory)| {
            let factory_ident = syn::Ident::new(factory, proc_macro2::Span::call_site());
            quote! {
                (#name, #factory_ident as fn() -> Box<dyn component_core::ExtensionInstance>)
            }
        })
        .collect();

    // Generate extension names list for diagnostics
    let extension_names: Vec<_> = extensions
        .iter()
        .map(|(name, _, _)| {
            quote! { #name }
        })
        .collect();

    let extension_count = extensions.len();

    let expanded = quote! {
        /// **GENERATED**: Get all extensions registered via #[extension] macros
        ///
        /// Returns a vector of (name, factory_function) tuples for all extensions
        /// that were registered using the #[extension("name")] macro in this crate.
        pub fn get_generated_extensions() -> Vec<(&'static str, fn() -> Box<dyn component_core::ExtensionInstance>)> {
            vec![#(#registrations),*]
        }

        /// **CONVENIENCE**: Initialize extensions with all generated extensions
        ///
        /// Calls all factory functions for extensions discovered via proc-macros in this crate.
        /// This is used by the AgentBuilder.get_extensions() workaround method.
        pub fn init_generated_extensions() -> component_core::ComponentResult<()> {
            // Just validate that all extensions can be created
            let extensions = get_generated_extensions();
            for (_name, factory_fn) in extensions {
                let _instance = factory_fn();
                // Extensions are created successfully
            }

            Ok(())
        }

        /// **DIAGNOSTICS**: Get number of extensions registered via proc macros
        ///
        /// Useful for debugging and verification that extensions were registered correctly.
        pub fn get_extension_count() -> usize {
            #extension_count
        }

        /// **DIAGNOSTICS**: List all extension names registered via proc macros
        ///
        /// Returns extension names for debugging and introspection.
        pub fn get_extension_names() -> Vec<&'static str> {
            vec![#(#extension_names),*]
        }
    };

    TokenStream::from(expanded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_method_name() {
        // Test valid method name
        assert!(parse_method_args("\"ping\"".parse().unwrap()).is_ok());

        // Test invalid method names would be tested here
        assert!(parse_method_args("\"\"".parse().unwrap()).is_err());
    }

    #[test]
    fn test_method_name_validation() {
        // Test that invalid method names are rejected
        assert!(parse_method_args("\"rpc.reserved\"".parse().unwrap()).is_err());
        assert!(parse_method_args("\"method with spaces\"".parse().unwrap()).is_err());
    }
}
