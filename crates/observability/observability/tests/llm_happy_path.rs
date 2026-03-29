use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use observability::{LogLevel, ObsHandle, attr, metric, span, value};
use observability_core::{ObservabilityPlugin, ObservabilityResult, SpanGuard, SpanStatus};

#[derive(Debug, Default, Clone)]
struct TestState {
    spans_started: Vec<(String, String, HashMap<String, String>)>, // (id, name, attrs)
    spans_ended: Vec<String>,
    span_status: HashMap<String, SpanStatus>,
    metrics: Vec<(String, f64, HashMap<String, String>)>, // (name, value, labels)
    logs: Vec<(LogLevel, String, HashMap<String, String>)>,
}

#[derive(Clone, Default)]
struct TestPlugin {
    state: Arc<Mutex<TestState>>,
    next_id: Arc<Mutex<u64>>,
}

impl TestPlugin {
    fn new() -> Self {
        Self::default()
    }

    fn alloc_id(&self) -> String {
        let mut g = self.next_id.lock().unwrap();
        *g += 1;
        format!("span-{}", *g)
    }
}

impl ObservabilityPlugin for TestPlugin {
    fn start_span(&self, name: &str, attributes: &[(&str, &str)]) -> SpanGuard {
        let id = self.alloc_id();
        let attrs: HashMap<String, String> = attributes
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();

        self.state
            .lock()
            .unwrap()
            .spans_started
            .push((id.clone(), name.to_string(), attrs));

        SpanGuard::new(id, Arc::new(self.clone()) as Arc<dyn ObservabilityPlugin>)
    }

    fn end_span(&self, span_id: &str) {
        self.state
            .lock()
            .unwrap()
            .spans_ended
            .push(span_id.to_string());
    }

    fn add_span_attribute(&self, span_id: &str, key: &str, value: &str) {
        let mut st = self.state.lock().unwrap();
        if let Some((_, _, attrs)) = st
            .spans_started
            .iter_mut()
            .rev()
            .find(|(id, _, _)| id == span_id)
        {
            attrs.insert(key.to_string(), value.to_string());
        }
    }

    fn set_span_status(&self, span_id: &str, status: SpanStatus) {
        self.state
            .lock()
            .unwrap()
            .span_status
            .insert(span_id.to_string(), status);
    }

    fn record_metric(&self, name: &str, value: f64, labels: &[(&str, &str)]) {
        let lbls: HashMap<String, String> = labels
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        self.state
            .lock()
            .unwrap()
            .metrics
            .push((name.to_string(), value, lbls));
    }

    fn write_log(&self, _message: &str) {}

    fn log_structured(
        &self,
        level: observability_core::traits::LogLevel,
        message: &str,
        fields: &serde_json::Value,
    ) {
        let mut map = HashMap::new();
        if let serde_json::Value::Object(obj) = fields {
            for (k, v) in obj {
                map.insert(k.clone(), v.to_string());
            }
        }
        self.state
            .lock()
            .unwrap()
            .logs
            .push((level, message.to_string(), map));
    }

    fn flush(&self) -> ObservabilityResult<()> {
        Ok(())
    }

    fn plugin_type(&self) -> &'static str {
        "test"
    }
}

struct TestObs {
    plugin: TestPlugin,
}

impl TestObs {
    fn new(plugin: TestPlugin) -> Self {
        Self { plugin }
    }

    fn state(&self) -> TestState {
        self.plugin.state.lock().unwrap().clone()
    }
}

impl ObsHandle for TestObs {
    fn span(&self, name: &str, attrs: &[(&str, &str)]) -> SpanGuard {
        self.plugin.start_span(name, attrs)
    }

    fn metric(&self, name: &str, value: f64, labels: &[(&str, &str)]) {
        self.plugin.record_metric(name, value, labels)
    }

    #[cfg(feature = "logging")]
    fn log(
        &self,
        level: LogLevel,
        message: &str,
        fields: &serde_json::Value,
    ) -> ObservabilityResult<()> {
        let mut map = HashMap::new();
        if let serde_json::Value::Object(obj) = fields {
            for (k, v) in obj {
                map.insert(k.clone(), v.to_string());
            }
        }
        self.plugin
            .state
            .lock()
            .unwrap()
            .logs
            .push((level, message.to_string(), map));
        Ok(())
    }

    fn log_kv(&self, level: LogLevel, message: &str, fields: &[(&str, &str)]) {
        let map: HashMap<String, String> = fields
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        self.plugin
            .state
            .lock()
            .unwrap()
            .logs
            .push((level, message.to_string(), map));
    }

    fn flush(&self) -> ObservabilityResult<()> {
        self.plugin.flush()
    }

    fn health(&self) -> observability::ObsHealth {
        observability::ObsHealth {
            logging: true,
            otel: None,
            prometheus: None,
            notes: vec!["test backend".to_string()],
        }
    }
}

#[test]
fn llm_happy_path_emits_span_and_token_metrics() {
    let plugin = TestPlugin::new();
    let obs = TestObs::new(plugin);

    let _span = obs.span(
        span::LLM_REQUEST,
        &[
            (attr::COMPONENT, "llm_client"),
            (attr::LLM_PROVIDER, "openai-compatible"),
            (attr::LLM_MODEL, "gpt-4o"),
            (attr::LLM_OPERATION, "chat_completions"),
            (attr::STATUS, value::STATUS_OK),
        ],
    );

    // Requests counter
    obs.metric(
        metric::LLM_REQUESTS_TOTAL,
        1.0,
        &[
            ("provider", "openai-compatible"),
            ("model", "gpt-4o"),
            ("operation", "chat_completions"),
            ("status", "ok"),
        ],
    );

    // Token counters (split by direction)
    obs.metric(
        metric::LLM_TOKENS_TOTAL,
        123.0,
        &[
            ("provider", "openai-compatible"),
            ("model", "gpt-4o"),
            ("direction", value::DIRECTION_INPUT),
        ],
    );
    obs.metric(
        metric::LLM_TOKENS_TOTAL,
        456.0,
        &[
            ("provider", "openai-compatible"),
            ("model", "gpt-4o"),
            ("direction", value::DIRECTION_OUTPUT),
        ],
    );

    let st = obs.state();

    assert!(
        st.spans_started
            .iter()
            .any(|(_, name, _)| name == span::LLM_REQUEST),
        "expected llm.request span"
    );

    assert!(
        st.metrics.iter().any(|(name, _, labels)| {
            name == metric::LLM_TOKENS_TOTAL
                && labels.get("direction").map(|s| s.as_str()) == Some("input")
        }),
        "expected llm_tokens_total input metric"
    );
    assert!(
        st.metrics.iter().any(|(name, _, labels)| {
            name == metric::LLM_TOKENS_TOTAL
                && labels.get("direction").map(|s| s.as_str()) == Some("output")
        }),
        "expected llm_tokens_total output metric"
    );
}
