use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{
    parse::Parser, parse_macro_input, punctuated::Punctuated, Expr, ExprLit, ItemFn, Lit, Meta,
    MetaNameValue, Token,
};

/// #[llm_tool(name="get_weather", description="...", context)]
#[proc_macro_attribute]
pub fn llm_tool(attrs: TokenStream, item: TokenStream) -> TokenStream {
    let parser = Punctuated::<Meta, Token![,]>::parse_terminated;
    let metas = match parser.parse(attrs) {
        Ok(v) => v,
        Err(_) => Punctuated::new(),
    };
    let mut tool_name: Option<String> = None;
    let mut tool_desc: Option<String> = None;
    let mut force_context = false;
    for m in metas.iter() {
        match m {
            Meta::NameValue(MetaNameValue { path, value, .. }) => {
                if path.is_ident("name") {
                    if let Expr::Lit(ExprLit {
                        lit: Lit::Str(ref s),
                        ..
                    }) = value
                    {
                        tool_name = Some(s.value());
                    }
                }
                if path.is_ident("description") {
                    if let Expr::Lit(ExprLit {
                        lit: Lit::Str(ref s),
                        ..
                    }) = value
                    {
                        tool_desc = Some(s.value());
                    }
                }
            }
            Meta::Path(path) if path.is_ident("context") => {
                force_context = true;
            }
            _ => {}
        }
    }

    let func = parse_macro_input!(item as ItemFn);
    let fn_ident = func.sig.ident.clone();
    let fn_name_str = fn_ident.to_string();
    let tool_name_str = tool_name.unwrap_or(fn_name_str.clone());
    let desc_str = tool_desc.unwrap_or_default();

    // Build a synthetic Params struct based on inputs (all must be typed)
    let params_ident = format_ident!("{}Params", fn_ident);
    let inputs = func.sig.inputs.clone();
    let mut fields = Vec::new();
    let mut call_args = Vec::new();
    let mut has_context_param = false;
    for (idx, input) in inputs.iter().enumerate() {
        match input {
            syn::FnArg::Typed(pt) => {
                let pat = &pt.pat;
                let ty = &pt.ty;
                let is_context = match &**ty {
                    syn::Type::Path(tp) => tp
                        .path
                        .segments
                        .last()
                        .map(|seg| seg.ident == "ToolContext")
                        .unwrap_or(false),
                    _ => false,
                };

                if is_context {
                    has_context_param = true;
                    call_args.push(quote! { <#ty as Default>::default() });
                    continue;
                }

                let field_ident = match &**pat {
                    syn::Pat::Ident(pi) => pi.ident.clone(),
                    _ => format_ident!("arg{}", idx),
                };
                fields.push(quote! { pub #field_ident: #ty });
                call_args.push(quote! { parsed.#field_ident });
            }
            _ => {}
        }
    }

    if force_context && !has_context_param {
        let err = quote! {
            compile_error!("#[llm_tool(context)] requires a ToolContext function parameter.");
        };
        return TokenStream::from(quote! { #err #func });
    }

    let registry_fn_ident = format_ident!("{}_llm_tool_info", fn_ident);
    let executor_fn_ident = format_ident!("{}_llm_tool_exec", fn_ident);
    let needs_context_fn_ident = format_ident!("{}_llm_tool_needs_context", fn_ident);
    let context_exec_fn_ident = format_ident!("{}_llm_tool_exec_ctx", fn_ident);

    let use_context = has_context_param || force_context;

    // Build call args for the context-aware executor: replace ToolContext::default()
    // with the actual downcast context from Box<dyn Any>.
    let call_args_ctx: Vec<_> = inputs
        .iter()
        .enumerate()
        .filter_map(|(idx, input)| {
            match input {
                syn::FnArg::Typed(pt) => {
                    let pat = &pt.pat;
                    let ty = &pt.ty;
                    let is_context = match &**ty {
                        syn::Type::Path(tp) => tp
                            .path
                            .segments
                            .last()
                            .map(|seg| seg.ident == "ToolContext")
                            .unwrap_or(false),
                        _ => false,
                    };
                    if is_context {
                        Some(quote! {
                            *__ctx_any.downcast::<#ty>().unwrap_or_else(|_| Box::new(<#ty as Default>::default()))
                        })
                    } else {
                        let field_ident = match &**pat {
                            syn::Pat::Ident(pi) => pi.ident.clone(),
                            _ => format_ident!("arg{}", idx),
                        };
                        Some(quote! { parsed.#field_ident })
                    }
                }
                _ => None,
            }
        })
        .collect();

    let gen = quote! {
        #func

        #[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
        pub struct #params_ident {
            #(#fields,)*
        }

        pub fn #registry_fn_ident() -> (String, String, serde_json::Value) {
            use schemars::JsonSchema;
            let schema = schemars::schema_for!(#params_ident);
            let json = serde_json::to_value(&schema.schema).unwrap_or(serde_json::json!({"type":"object"}));
            (#tool_name_str.to_string(), #desc_str.to_string(), json)
        }

        pub fn #executor_fn_ident(args: serde_json::Value) -> serde_json::Value {
            let parsed: #params_ident = match serde_json::from_value(args) {
                Ok(v) => v,
                Err(e) => return serde_json::json!({"error": format!("invalid arguments: {}", e)}),
            };
            let out = #fn_ident( #( #call_args ),* );
            serde_json::to_value(out).unwrap_or(serde_json::json!({"result":"<non-serializable>"}))
        }

        pub fn #needs_context_fn_ident() -> bool {
            #use_context
        }

        pub fn #context_exec_fn_ident(args: serde_json::Value, __ctx_any: Box<dyn ::std::any::Any + Send>) -> serde_json::Value {
            let parsed: #params_ident = match serde_json::from_value(args) {
                Ok(v) => v,
                Err(e) => return serde_json::json!({"error": format!("invalid arguments: {}", e)}),
            };
            let out = #fn_ident( #( #call_args_ctx ),* );
            serde_json::to_value(out).unwrap_or(serde_json::json!({"result":"<non-serializable>"}))
        }
    };
    gen.into()
}
