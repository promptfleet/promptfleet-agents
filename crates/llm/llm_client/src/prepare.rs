use crate::{
    model_client::ApiMode,
    profile::{ModelConfig, ModelFamily, ModelProfile},
    types::LlmRequest,
};

#[derive(Debug, Clone, Copy)]
pub enum Policy {
    Strict,
    Permissive,
}

pub trait RequestMutator {
    fn name(&self) -> &'static str;
    fn mutate(&self, cfg: &ModelConfig, req: &mut LlmRequest) -> anyhow::Result<()>;
}

pub trait RequestValidator {
    fn name(&self) -> &'static str;
    fn validate(&self, cfg: &ModelConfig, req: &LlmRequest, policy: Policy) -> anyhow::Result<()>;
}

pub struct ProfileCapabilityValidator;

impl RequestValidator for ProfileCapabilityValidator {
    fn name(&self) -> &'static str {
        "profile_capability_validator"
    }

    fn validate(&self, cfg: &ModelConfig, req: &LlmRequest, policy: Policy) -> anyhow::Result<()> {
        match cfg.profile {
            ModelProfile::Generic => Ok(()),
            ModelProfile::Gpt5 { .. } => {
                if req.temperature.is_some() {
                    match policy {
                        Policy::Strict => anyhow::bail!("gpt-5 does not support temperature"),
                        Policy::Permissive => {
                            log::warn!("dropping unsupported field: temperature for gpt-5");
                        }
                    }
                }
                Ok(())
            }
            ModelProfile::Qwen3 { .. } => Ok(()),
        }
    }
}

pub struct Gpt5Mutator;

impl RequestMutator for Gpt5Mutator {
    fn name(&self) -> &'static str {
        "gpt5_mutator"
    }

    fn mutate(&self, cfg: &ModelConfig, req: &mut LlmRequest) -> anyhow::Result<()> {
        if let ModelFamily::Gpt5 = cfg.family {
            // Remove unsupported temperature (validator handles policy)
            if req.temperature.is_some() {
                req.temperature = None;
            }
            // Map max_tokens -> max_completion_tokens explicitly for GPT-5
            if let Some(mt) = req.max_tokens.take() {
                let map = req.extensions.get_or_insert_with(Default::default);
                map.insert(
                    "max_completion_tokens".to_string(),
                    serde_json::Value::from(mt),
                );
            }
            // Inject reasoning_effort if provided in profile (Chat API: top-level string)
            if let ModelProfile::Gpt5 {
                reasoning_effort,
                responses_text_verbosity,
                responses_reasoning_object,
            } = &cfg.profile
            {
                if let Some(val) = reasoning_effort {
                    let map = req.extensions.get_or_insert_with(Default::default);
                    map.insert(
                        "reasoning_effort".to_string(),
                        serde_json::Value::String(val.clone()),
                    );
                }
                // Optional Responses API enrichments (adapter merges extensions for both endpoints)
                if let Some(verbosity) = responses_text_verbosity {
                    let map = req.extensions.get_or_insert_with(Default::default);
                    let mut text_obj = serde_json::Map::new();
                    text_obj.insert(
                        "verbosity".to_string(),
                        serde_json::Value::String(verbosity.clone()),
                    );
                    map.insert("text".to_string(), serde_json::Value::Object(text_obj));
                }
                if responses_reasoning_object.unwrap_or(false) {
                    if let Some(val) = reasoning_effort.clone() {
                        let map = req.extensions.get_or_insert_with(Default::default);
                        let mut reasoning_obj = serde_json::Map::new();
                        reasoning_obj.insert("effort".to_string(), serde_json::Value::String(val));
                        map.insert(
                            "reasoning".to_string(),
                            serde_json::Value::Object(reasoning_obj),
                        );
                    }
                }
            }
        }
        Ok(())
    }
}

pub struct QwenVllmExtras;

impl RequestMutator for QwenVllmExtras {
    fn name(&self) -> &'static str {
        "qwen_vllm_extras"
    }

    fn mutate(&self, cfg: &ModelConfig, req: &mut LlmRequest) -> anyhow::Result<()> {
        if let ModelFamily::Qwen3 = cfg.family {
            if let ModelProfile::Qwen3 {
                enable_thinking,
                tool_call_parser,
                reasoning_parser,
                auto_tool_choice,
                template_kwargs,
            } = &cfg.profile
            {
                let map = req.extensions.get_or_insert_with(Default::default);
                if let Some(v) = tool_call_parser {
                    map.insert(
                        "tool_call_parser".to_string(),
                        serde_json::Value::String(v.clone()),
                    );
                }
                if let Some(v) = reasoning_parser {
                    map.insert(
                        "reasoning_parser".to_string(),
                        serde_json::Value::String(v.clone()),
                    );
                }
                if let Some(v) = auto_tool_choice {
                    map.insert(
                        "enable_auto_tool_choice".to_string(),
                        serde_json::Value::Bool(*v),
                    );
                }
                if let Some(v) = enable_thinking.or(Some(false)) {
                    // default explicit false unless user sets true
                    let mut chat_kwargs = serde_json::Map::new();
                    chat_kwargs.insert("enable_thinking".to_string(), serde_json::Value::Bool(v));
                    if let Some(extra) = template_kwargs {
                        if extra.is_object() {
                            if let Some(obj) = extra.as_object() {
                                for (k, val) in obj {
                                    chat_kwargs.insert(k.clone(), val.clone());
                                }
                            }
                        }
                    }
                    map.insert(
                        "chat_template_kwargs".to_string(),
                        serde_json::Value::Object(chat_kwargs),
                    );
                }
            }
        }
        Ok(())
    }
}

pub fn prepare_request(
    cfg: &ModelConfig,
    mut req: LlmRequest,
    mutators: &[Box<dyn RequestMutator>],
    validators: &[Box<dyn RequestValidator>],
    policy: Policy,
) -> anyhow::Result<LlmRequest> {
    // validate first to give immediate feedback, then allow mutators to shape fields
    for v in validators {
        v.validate(cfg, &req, policy)?;
    }
    for m in mutators {
        m.mutate(cfg, &mut req)?;
    }
    // validate again post-mutation
    for v in validators {
        v.validate(cfg, &req, policy)?;
    }
    Ok(req)
}
