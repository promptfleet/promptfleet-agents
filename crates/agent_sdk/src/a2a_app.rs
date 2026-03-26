//! A2A application facade: owns the HTTP server wiring around a shared [`crate::Agent`].
//!
//! Construct via [`A2aApp::from_agent`] or [`A2aApp::from_shared_agent`] when the `a2a-server`
//! feature is enabled.

#[cfg(feature = "a2a-server")]
use std::sync::Arc;

#[cfg(feature = "a2a-server")]
use crate::{Agent, SdkResult};

#[cfg(feature = "a2a-server")]
use crate::server::A2aServer;

#[cfg(feature = "agent-observability")]
use observability::{Obs, ObservabilityConfig as ObsConfig};

/// A2A application facade that owns A2A server wiring around a shared runtime.
pub struct A2aApp {
    #[cfg(feature = "a2a-server")]
    server: A2aServer,
    #[cfg(feature = "agent-observability")]
    obs: Option<Obs>,
}

impl A2aApp {
    #[cfg(feature = "a2a-server")]
    pub fn from_agent(agent: Agent) -> SdkResult<Self> {
        Self::from_shared_agent(Arc::new(agent))
    }

    #[cfg(feature = "a2a-server")]
    pub fn from_shared_agent(agent: Arc<Agent>) -> SdkResult<Self> {
        #[cfg(feature = "agent-observability")]
        let obs = agent.get_service::<Obs>().map(|arc| (*arc).clone());
        let server = A2aServer::from_shared_agent(agent)?;
        Ok(Self {
            server,
            #[cfg(feature = "agent-observability")]
            obs,
        })
    }

    #[cfg(all(feature = "a2a-server", feature = "agent-observability"))]
    pub fn from_agent_with_obs(agent: Agent, obs_cfg: Option<ObsConfig>) -> SdkResult<Self> {
        let obs = match obs_cfg {
            Some(cfg) => Obs::init(cfg).unwrap_or_else(|e| {
                log::warn!("obs:auto_init_failed err={}, using noop", e);
                Obs::noop()
            }),
            None => Obs::init_from_env().unwrap_or_else(|e| {
                log::warn!("obs:auto_init_from_env_failed err={}, using noop", e);
                Obs::noop()
            }),
        };
        let agent = Arc::new(agent.with_service(obs.clone()));
        let server = A2aServer::from_shared_agent(agent)?;
        Ok(Self {
            server,
            obs: Some(obs),
        })
    }

    #[cfg(feature = "agent-observability")]
    pub fn obs(&self) -> Option<&Obs> {
        self.obs.as_ref()
    }

    #[cfg(all(feature = "a2a-server", not(target_arch = "wasm32")))]
    pub fn router(self) -> axum::Router {
        self.server.build_router()
    }

    #[cfg(all(feature = "a2a-server", not(target_arch = "wasm32")))]
    pub async fn serve(self, addr: &str) -> anyhow::Result<()> {
        self.server
            .serve(addr)
            .await
            .map_err(|e| anyhow::anyhow!(e))
    }

    #[cfg(all(feature = "a2a-server", target_arch = "wasm32"))]
    pub fn serve(&self, req: spin_sdk::http::Request) -> SdkResult<spin_sdk::http::Response> {
        self.server.serve_request_unified(req)
    }

    #[cfg(all(feature = "a2a-server", target_arch = "wasm32"))]
    pub async fn serve_async(
        &self,
        req: spin_sdk::http::Request,
    ) -> SdkResult<spin_sdk::http::Response> {
        self.server.serve_request_unified_async(req).await
    }

    #[cfg(all(feature = "a2a-server", target_arch = "wasm32"))]
    pub async fn serve_async_flushed(
        &self,
        req: spin_sdk::http::Request,
    ) -> SdkResult<spin_sdk::http::Response> {
        let result = self.server.serve_request_unified_async(req).await;

        #[cfg(feature = "agent-observability")]
        if let Some(obs) = &self.obs {
            match obs.maybe_flush() {
                Ok(()) => log::debug!("obs:auto_flush ok=true"),
                Err(e) => log::warn!("obs:auto_flush ok=false err={}", e),
            }
        }

        result
    }
}

#[cfg(all(test, feature = "a2a-server"))]
mod tests {
    use super::A2aApp;
    use crate::Agent;
    use std::sync::Arc;

    #[test]
    fn from_shared_agent_builds() {
        let agent = Arc::new(Agent::new_runtime("a2a-app-test").expect("agent"));
        let app = A2aApp::from_shared_agent(agent).expect("app");
        let _ = app;
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn router_returns_axum() {
        let agent = Arc::new(Agent::new_runtime("router-test").expect("agent"));
        let app = A2aApp::from_shared_agent(agent).expect("app");
        let router = app.router();
        let _ = router;
    }
}
