use proc_macro::TokenStream;
use quote::quote;
use syn::{
    parse::Parse, parse::ParseStream, parse_macro_input, Ident, Item, LitStr, Path,
    Result as SynResult, Token,
};

struct PfArgs {
    init: Path,
}

impl Parse for PfArgs {
    fn parse(input: ParseStream) -> SynResult<Self> {
        let key: Ident = input.parse()?; // expect 'init'
        if key != "init" {
            return Err(syn::Error::new(key.span(), "expected 'init'"));
        }
        let _eq: Token![=] = input.parse()?;
        // Accept either a string literal or a path
        if input.peek(LitStr) {
            let lit: LitStr = input.parse()?;
            let path: Path = syn::parse_str(&lit.value())?;
            Ok(PfArgs { init: path })
        } else {
            let path: Path = input.parse()?;
            Ok(PfArgs { init: path })
        }
    }
}

#[proc_macro_attribute]
pub fn pf_agent(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = parse_macro_input!(attr as PfArgs);
    let parsed_item: Item = match syn::parse(item.clone()) {
        Ok(it) => it,
        Err(e) => return e.to_compile_error().into(),
    };
    let init_path = args.init;

    let expanded = quote! {
        #parsed_item

        #[cfg(target_arch = "wasm32")]
        static __PF_AGENT_APP: std::sync::OnceLock<agent_sdk::a2a::A2aApp> = std::sync::OnceLock::new();

        #[cfg(target_arch = "wasm32")]
        fn __pf_get_app() -> &'static agent_sdk::a2a::A2aApp {
            __PF_AGENT_APP.get_or_init(|| {
                let agent = #init_path().expect("pf_agent init failed");
                agent_sdk::a2a::A2aApp::from_agent(agent).expect("pf_agent A2aApp build failed")
            })
        }

        #[cfg(target_arch = "wasm32")]
        #[spin_sdk::http_component]
        fn handle_request(req: spin_sdk::http::Request) -> anyhow::Result<impl spin_sdk::http::IntoResponse> {
            Ok(__pf_get_app().serve(req)?)
        }
    };
    TokenStream::from(expanded)
}
