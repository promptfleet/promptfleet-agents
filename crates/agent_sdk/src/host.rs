//! Compose protocol adapters (A2A, optional AG-UI) around a shared [`crate::Agent`].
//!
//! [`AgentHostBuilder`] constructs an [`AgentHost`] with at least one enabled adapter.

use std::sync::Arc;

use crate::{Agent, SdkError, SdkResult};

#[derive(Default, Clone)]
pub struct AgentHostBuilder {
    agent: Option<Arc<Agent>>,
    #[cfg(feature = "a2a-server")]
    enable_a2a: bool,
    #[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
    agui_config: Option<crate::agui::AgUiConfig>,
}

impl AgentHostBuilder {
    pub fn new(agent: Agent) -> Self {
        Self {
            agent: Some(Arc::new(agent)),
            #[cfg(feature = "a2a-server")]
            enable_a2a: false,
            #[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
            agui_config: None,
        }
    }

    #[cfg(feature = "a2a-server")]
    pub fn with_a2a(mut self) -> Self {
        self.enable_a2a = true;
        self
    }

    #[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
    pub fn with_agui(mut self, config: crate::agui::AgUiConfig) -> Self {
        self.agui_config = Some(config);
        self
    }

    pub fn build(self) -> SdkResult<AgentHost> {
        let agent = self
            .agent
            .ok_or_else(|| SdkError::configuration("AgentHostBuilder requires an agent"))?;
        let host = AgentHost::new(agent);

        #[cfg(all(feature = "a2a-server", not(target_arch = "wasm32")))]
        let host = {
            let mut host = host;
            if self.enable_a2a {
                host.a2a = Some(crate::a2a::A2aApp::from_shared_agent(host.agent.clone())?);
            }
            host
        };

        #[cfg(all(feature = "a2a-server", target_arch = "wasm32"))]
        let host = {
            let mut host = host;
            if self.enable_a2a {
                host.a2a = Some(Arc::new(crate::a2a::A2aApp::from_shared_agent(
                    host.agent.clone(),
                )?));
            }
            host
        };

        #[cfg(not(feature = "a2a-server"))]
        let host = host;

        #[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
        let mut host = host;

        #[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
        if let Some(config) = self.agui_config {
            host.agui = Some(crate::agui::AgUiApp::from_shared_agent(
                host.agent.clone(),
                config,
            )?);
        }

        if !host.has_enabled_adapter() {
            return Err(SdkError::configuration(
                "AgentHostBuilder requires at least one protocol adapter",
            ));
        }

        Ok(host)
    }

    #[cfg(all(
        not(target_arch = "wasm32"),
        any(feature = "a2a-server", feature = "event-stream")
    ))]
    pub fn build_router(self) -> SdkResult<axum::Router> {
        self.build()?.build_router()
    }

    #[cfg(all(target_arch = "wasm32", feature = "a2a-server"))]
    pub fn build_router(self) -> SdkResult<spin_sdk::http::Router> {
        self.build()?.build_router()
    }
}

pub struct AgentHost {
    agent: Arc<Agent>,
    #[cfg(all(feature = "a2a-server", not(target_arch = "wasm32")))]
    a2a: Option<crate::a2a::A2aApp>,
    #[cfg(all(feature = "a2a-server", target_arch = "wasm32"))]
    a2a: Option<Arc<crate::a2a::A2aApp>>,
    #[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
    agui: Option<crate::agui::AgUiApp>,
}

impl AgentHost {
    fn new(agent: Arc<Agent>) -> Self {
        Self {
            agent,
            #[cfg(all(feature = "a2a-server", not(target_arch = "wasm32")))]
            a2a: None,
            #[cfg(all(feature = "a2a-server", target_arch = "wasm32"))]
            a2a: None,
            #[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
            agui: None,
        }
    }

    fn has_enabled_adapter(&self) -> bool {
        #[cfg(feature = "a2a-server")]
        if self.a2a.is_some() {
            return true;
        }

        #[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
        if self.agui.is_some() {
            return true;
        }

        false
    }

    #[cfg(all(
        not(target_arch = "wasm32"),
        any(feature = "a2a-server", feature = "event-stream")
    ))]
    pub fn build_router(self) -> SdkResult<axum::Router> {
        let mut router = axum::Router::new();

        #[cfg(feature = "a2a-server")]
        if let Some(a2a) = self.a2a {
            router = router.merge(a2a.router());
        }

        #[cfg(feature = "event-stream")]
        if let Some(agui) = self.agui {
            router = router.merge(agui.router());
        }

        Ok(router)
    }

    #[cfg(all(target_arch = "wasm32", feature = "a2a-server"))]
    pub fn build_router(&self) -> SdkResult<spin_sdk::http::Router> {
        self.build_spin_router()
    }

    #[cfg(all(target_arch = "wasm32", feature = "a2a-server"))]
    pub fn serve(&self, req: spin_sdk::http::Request) -> SdkResult<spin_sdk::http::Response> {
        Ok(self.build_spin_router()?.handle(req))
    }

    #[cfg(all(target_arch = "wasm32", feature = "a2a-server"))]
    pub async fn serve_async(
        &self,
        req: spin_sdk::http::Request,
    ) -> SdkResult<spin_sdk::http::Response> {
        Ok(self.build_spin_router()?.handle_async(req).await)
    }

    #[cfg(all(target_arch = "wasm32", feature = "a2a-server"))]
    pub async fn serve_async_flushed(
        &self,
        req: spin_sdk::http::Request,
    ) -> SdkResult<spin_sdk::http::Response> {
        self.serve_async(req).await
    }

    #[cfg(all(target_arch = "wasm32", feature = "a2a-server"))]
    fn build_spin_router(&self) -> SdkResult<spin_sdk::http::Router> {
        let Some(a2a) = self.a2a.clone() else {
            return Err(SdkError::configuration(
                "AgentHost requires at least one wasm-capable protocol adapter",
            ));
        };

        let mut router = spin_sdk::http::Router::new();
        for path in [
            "/",
            "/jsonrpc",
            "/.well-known/agent-card.json",
            "/v1/agent/card:get",
            "/health",
        ] {
            let app = a2a.clone();
            router.any_async(path, move |req: spin_sdk::http::Request, _params| {
                let app = app.clone();
                async move { handle_wasm_a2a(req, app).await }
            });
        }

        Ok(router)
    }
}

#[cfg(all(target_arch = "wasm32", feature = "a2a-server"))]
async fn handle_wasm_a2a(
    req: spin_sdk::http::Request,
    app: Arc<crate::a2a::A2aApp>,
) -> spin_sdk::http::Response {
    match app.serve_async_flushed(req).await {
        Ok(response) => response,
        Err(err) => spin_sdk::http::Response::new(500, format!("Agent host routing failed: {err}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_requires_adapter() {
        let agent = Agent::new_runtime("host-no-adapter").expect("create agent");
        let err = AgentHostBuilder::new(agent)
            .build()
            .err()
            .expect("host without adapters should fail");
        assert!(err
            .to_string()
            .contains("requires at least one protocol adapter"));
    }

    #[cfg(all(not(target_arch = "wasm32"), feature = "a2a-server"))]
    #[test]
    fn native_host_builds_router_with_a2a() {
        let agent = Agent::new_runtime("native-a2a-host").expect("create agent");
        let _router = AgentHostBuilder::new(agent)
            .with_a2a()
            .build_router()
            .expect("build router");
    }

    #[cfg(all(target_arch = "wasm32", feature = "a2a-server"))]
    #[test]
    fn wasm_host_builds_with_a2a() {
        let agent = Agent::new_runtime("wasm-a2a-host").expect("create agent");
        let host = AgentHostBuilder::new(agent)
            .with_a2a()
            .build()
            .expect("build host");
        let req = spin_sdk::http::Request::get("/health");
        let _ = host.serve(req).expect("serve request");
    }
}
