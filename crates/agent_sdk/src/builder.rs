//! Agent Builder Facade
//!
//! Builds the protocol-neutral [`crate::Agent`] runtime.

use crate::{Agent, SdkError, SdkResult};

#[cfg(feature = "config-loader")]
use pf_config::{LoadOptions, load_config_untyped};

#[cfg(feature = "agent-observability")]
use observability::{Obs, ObservabilityConfig as ObsConfig};

#[cfg(feature = "agent-observability")]
use crate::ObservabilityRuntime;
#[cfg(feature = "context-window")]
use llm_context_core::{LongTermMemory, history::Summarizer};
/// Fluent builder to construct the protocol-neutral [`crate::Agent`] runtime.
pub struct AgentBuilder {
    config: crate::agent::config::AgentConfig,
    timeout_policy: Option<crate::timeout_policy::TimeoutPolicy>,
    #[cfg(feature = "structured-io")]
    structured_output_contract: Option<crate::structured::StructuredOutputContract>,
    #[cfg(feature = "context-window")]
    history_summarizer: Option<std::sync::Arc<dyn Summarizer>>,
    #[cfg(feature = "context-window")]
    history_memory: Option<std::sync::Arc<dyn LongTermMemory>>,
    #[cfg(feature = "agent-observability")]
    obs_config: Option<ObsConfig>,
}

impl AgentBuilder {
    /// Build from an in-memory [`crate::agent::config::AgentConfig`] (no JSON file required).
    pub fn from_config(config: crate::agent::config::AgentConfig) -> SdkResult<Self> {
        if config.name.is_empty() {
            return Err(SdkError::invalid_input("Agent name cannot be empty"));
        }
        if config.max_message_size == 0 {
            return Err(SdkError::invalid_input(
                "Max message size must be greater than zero",
            ));
        }
        Ok(Self {
            config,
            timeout_policy: Some(crate::timeout_policy::TimeoutPolicy::streaming_default()),
            #[cfg(feature = "structured-io")]
            structured_output_contract: None,
            #[cfg(feature = "context-window")]
            history_summarizer: None,
            #[cfg(feature = "context-window")]
            history_memory: None,
            #[cfg(feature = "agent-observability")]
            obs_config: None,
        })
    }

    /// Minimal builder with default [`crate::agent::config::AgentConfig`] except `name`.
    pub fn new(name: &str) -> SdkResult<Self> {
        let mut config = crate::agent::config::AgentConfig::default();
        config.name = name.to_string();
        Self::from_config(config)
    }

    pub fn from_config_path(path: &str) -> SdkResult<Self> {
        #[cfg(not(feature = "config-loader"))]
        {
            let _ = path;
            return Err(SdkError::feature_not_enabled("config-loader"));
        }

        #[cfg(feature = "config-loader")]
        {
            let mut opts = LoadOptions::default();
            opts.json_paths = vec![std::path::PathBuf::from(path)];
            opts.required = true;
            // Keep prefix consistent with pf_config defaults ("AGENT_"), so env vars like:
            // AGENT_AGENT__NAME, AGENT_OBSERVABILITY__SERVICE_NAME, etc work.
            let root = load_config_untyped(opts)
                .map_err(|e| SdkError::configuration(format!("config load failed: {}", e)))?;

            let config = agent_config_from_value(&root);

            #[cfg(feature = "agent-observability")]
            let obs_config = {
                let obs_cfg = ObsConfig::from_value(&root);
                // Only treat as present if explicitly configured or env indicates it.
                // Heuristic: enabled flags or non-default service name.
                let is_default = obs_cfg.service_name == ObsConfig::default().service_name
                    && !obs_cfg.otel.enabled
                    && !obs_cfg.prometheus.enabled;
                if is_default { None } else { Some(obs_cfg) }
            };

            Ok(Self {
                config,
                timeout_policy: Some(crate::timeout_policy::TimeoutPolicy::streaming_default()),
                #[cfg(feature = "structured-io")]
                structured_output_contract: None,
                #[cfg(feature = "context-window")]
                history_summarizer: None,
                #[cfg(feature = "context-window")]
                history_memory: None,
                #[cfg(feature = "agent-observability")]
                obs_config,
            })
        }
    }

    pub fn with_name(mut self, name: &str) -> Self {
        self.config.name = name.to_string();
        self
    }

    /// Override the default timeout policy.
    pub fn with_timeout_policy(mut self, policy: crate::timeout_policy::TimeoutPolicy) -> Self {
        self.timeout_policy = Some(policy);
        self
    }

    #[cfg(feature = "structured-io")]
    pub fn with_structured_output_contract(
        mut self,
        contract: crate::structured::StructuredOutputContract,
    ) -> Self {
        self.structured_output_contract = Some(contract);
        self
    }

    #[cfg(feature = "structured-io")]
    pub fn with_structured_output<T>(
        self,
        schema_name: impl Into<String>,
        artifact_name: impl Into<String>,
    ) -> Self
    where
        T: schemars::JsonSchema,
    {
        self.with_structured_output_contract(
            crate::structured::StructuredOutputContract::from_type::<T>(
                schema_name,
                artifact_name,
            ),
        )
    }

    #[cfg(feature = "context-window")]
    pub fn with_history_summarizer(mut self, summarizer: std::sync::Arc<dyn Summarizer>) -> Self {
        self.history_summarizer = Some(summarizer);
        self
    }

    #[cfg(feature = "context-window")]
    pub fn with_history_memory(mut self, memory: std::sync::Arc<dyn LongTermMemory>) -> Self {
        self.history_memory = Some(memory);
        self
    }

    /// Get the currently configured timeout policy.
    pub fn timeout_policy(&self) -> Option<&crate::timeout_policy::TimeoutPolicy> {
        self.timeout_policy.as_ref()
    }

    pub fn build(self) -> SdkResult<Agent> {
        #[cfg(feature = "agent-observability")]
        let obs = if let Some(cfg) = self.obs_config {
            Some(Obs::init(cfg).map_err(|e| {
                SdkError::configuration(format!("observability init failed: {}", e))
            })?)
        } else {
            None
        };

        #[cfg(feature = "agent-observability")]
        let obs_runtime = obs.as_ref().map(|o| {
            let rt = ObservabilityRuntime::new(o.clone());
            rt.start_background_flush_loop();
            rt
        });

        let mut agent = Agent::new_with_config(self.config)?;

        // Register the handle into the agent's service container so server/client adapters can reuse it.
        #[cfg(feature = "agent-observability")]
        if let Some(obs) = &obs {
            agent = agent.with_service(obs.clone());
        }

        // Also register the runtime (flush orchestration).
        #[cfg(feature = "agent-observability")]
        if let Some(rt) = obs_runtime {
            agent = agent.with_service(rt);
        }

        if let Some(timeout_policy) = self.timeout_policy {
            agent = agent.with_service(timeout_policy);
        }

        #[cfg(feature = "structured-io")]
        if let Some(contract) = self.structured_output_contract {
            agent.configure_structured_output(contract)?;
        }

        #[cfg(feature = "context-window")]
        agent.configure_history_policy_runtime(self.history_summarizer, self.history_memory);

        Ok(agent)
    }
}

#[cfg(feature = "config-loader")]
fn agent_config_from_value(root: &serde_json::Value) -> crate::agent::config::AgentConfig {
    let mut cfg = crate::agent::config::AgentConfig::default();
    let v = root.get("agent").unwrap_or(root);
    if let Some(obj) = v.as_object() {
        if let Some(name) = obj.get("name").and_then(|v| v.as_str()) {
            cfg.name = name.to_string();
        }
        if let Some(desc) = obj.get("description").and_then(|v| v.as_str()) {
            cfg.description = desc.to_string();
        }
        if let Some(ver) = obj.get("version").and_then(|v| v.as_str()) {
            cfg.version = ver.to_string();
        }
        if let Some(ms) = obj.get("max_message_size").and_then(|v| v.as_u64()) {
            cfg.max_message_size = ms;
        }
        if let Some(streaming) = obj.get("streaming").and_then(|v| v.as_bool()) {
            cfg.streaming = streaming;
        }
        if let Some(batch) = obj.get("batch_processing").and_then(|v| v.as_bool()) {
            cfg.batch_processing = batch;
        }
        if let Some(ct) = obj.get("concurrent_tasks").and_then(|v| v.as_u64()) {
            cfg.concurrent_tasks = Some(ct as u32);
        }
        if let Some(stateless) = obj.get("stateless_methods").and_then(|v| v.as_bool()) {
            cfg.stateless_methods = stateless;
        }
        if let Some(url) = obj.get("base_url").and_then(|v| v.as_str()) {
            cfg.base_url = Some(url.to_string());
        }
        if let Some(history_policy) = obj.get("history_policy") {
            if let Ok(policy) = serde_json::from_value(history_policy.clone()) {
                cfg.history_policy = Some(policy);
            }
        }
    }
    cfg
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_config_valid_builds_agent() {
        let mut cfg = crate::agent::config::AgentConfig::default();
        cfg.name = "builder-test".to_string();
        let agent = AgentBuilder::from_config(cfg).unwrap().build().unwrap();
        assert_eq!(agent.config().name, "builder-test");
    }

    #[test]
    fn new_sets_name() {
        let agent = AgentBuilder::new("hello-agent").unwrap().build().unwrap();
        assert_eq!(agent.config().name, "hello-agent");
    }

    #[test]
    fn from_config_rejects_empty_name() {
        let mut cfg = crate::agent::config::AgentConfig::default();
        cfg.name = String::new();
        assert!(AgentBuilder::from_config(cfg).is_err());
    }

    #[test]
    fn with_timeout_policy_propagates() {
        let policy = crate::timeout_policy::TimeoutPolicy::streaming_default();
        let b = AgentBuilder::new("t")
            .unwrap()
            .with_timeout_policy(policy.clone());
        assert!(b.timeout_policy().is_some());
    }

    #[cfg(feature = "structured-io")]
    #[test]
    fn with_structured_output_contract_builds() {
        #[derive(schemars::JsonSchema)]
        struct Output {
            verdict: String,
        }

        let agent = AgentBuilder::new("structured-builder")
            .unwrap()
            .with_structured_output::<Output>("output", "analysis_output")
            .build()
            .unwrap();
        assert!(agent.config().name == "structured-builder");
    }

    #[cfg(all(feature = "config-loader", not(target_arch = "wasm32")))]
    #[test]
    fn from_config_path_valid_json() {
        use std::io::Write;
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        writeln!(
            tmp,
            r#"{{"agent":{{"name":"file-agent","max_message_size":1048576,"description":"d"}}}}"#
        )
        .unwrap();
        let path = tmp.path().to_str().unwrap();
        let agent = AgentBuilder::from_config_path(path)
            .unwrap()
            .build()
            .unwrap();
        assert_eq!(agent.config().name, "file-agent");
    }

    #[cfg(all(feature = "config-loader", not(target_arch = "wasm32")))]
    #[test]
    fn from_config_path_invalid_json_returns_error() {
        use std::io::Write;
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        writeln!(tmp, "{}", r#"not valid json {"#).unwrap();
        let path = tmp.path().to_str().unwrap();
        let err = AgentBuilder::from_config_path(path);
        assert!(
            err.is_err(),
            "expected configuration error for invalid JSON"
        );
    }
}
