use crate::{
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::{ModelConfig, ModelFamily, ModelProfile};
    use crate::types::LlmRequest;
    use std::collections::BTreeMap;

    fn gpt5_model_config(profile: ModelProfile) -> ModelConfig {
        ModelConfig {
            model_id: "gpt-5".to_string(),
            family: ModelFamily::Gpt5,
            profile,
            capabilities: None,
            extensions: BTreeMap::new(),
        }
    }

    fn qwen3_model_config(profile: ModelProfile) -> ModelConfig {
        ModelConfig {
            model_id: "qwen3-vllm".to_string(),
            family: ModelFamily::Qwen3,
            profile,
            capabilities: None,
            extensions: BTreeMap::new(),
        }
    }

    #[test]
    fn test_prepare_request_happy_path_applies_mutators_and_validators() {
        let cfg = gpt5_model_config(ModelProfile::Gpt5 {
            reasoning_effort: Some("high".into()),
            responses_text_verbosity: None,
            responses_reasoning_object: None,
        });
        let req = LlmRequest {
            model: "gpt-5".to_string(),
            messages: vec![],
            temperature: Some(0.7),
            max_tokens: Some(1000),
            ..Default::default()
        };
        let mutators: Vec<Box<dyn RequestMutator>> = vec![Box::new(Gpt5Mutator)];
        let validators: Vec<Box<dyn RequestValidator>> = vec![Box::new(ProfileCapabilityValidator)];
        let out = prepare_request(
            &cfg,
            req,
            mutators.as_slice(),
            validators.as_slice(),
            Policy::Permissive,
        )
        .expect("prepare_request");
        assert!(out.temperature.is_none());
        assert!(out.max_tokens.is_none());
        let ext = out.extensions.as_ref().expect("extensions");
        assert_eq!(
            ext.get("max_completion_tokens").and_then(|v| v.as_u64()),
            Some(1000)
        );
        assert_eq!(
            ext.get("reasoning_effort").and_then(|v| v.as_str()),
            Some("high")
        );
    }

    #[test]
    fn test_gpt5_mutator_strips_temperature() {
        let cfg = gpt5_model_config(ModelProfile::Gpt5 {
            reasoning_effort: None,
            responses_text_verbosity: None,
            responses_reasoning_object: None,
        });
        let mut req = LlmRequest {
            temperature: Some(0.7),
            ..Default::default()
        };
        Gpt5Mutator.mutate(&cfg, &mut req).unwrap();
        assert!(req.temperature.is_none());
    }

    #[test]
    fn test_gpt5_mutator_maps_max_tokens_to_extension() {
        let cfg = gpt5_model_config(ModelProfile::Gpt5 {
            reasoning_effort: None,
            responses_text_verbosity: None,
            responses_reasoning_object: None,
        });
        let mut req = LlmRequest {
            max_tokens: Some(2048),
            ..Default::default()
        };
        Gpt5Mutator.mutate(&cfg, &mut req).unwrap();
        assert!(req.max_tokens.is_none());
        let ext = req.extensions.as_ref().unwrap();
        assert_eq!(
            ext.get("max_completion_tokens").and_then(|v| v.as_u64()),
            Some(2048)
        );
    }

    #[test]
    fn test_gpt5_mutator_injects_reasoning_effort() {
        let cfg = gpt5_model_config(ModelProfile::Gpt5 {
            reasoning_effort: Some("minimal".into()),
            responses_text_verbosity: None,
            responses_reasoning_object: None,
        });
        let mut req = LlmRequest::default();
        Gpt5Mutator.mutate(&cfg, &mut req).unwrap();
        let ext = req.extensions.as_ref().unwrap();
        assert_eq!(
            ext.get("reasoning_effort").and_then(|v| v.as_str()),
            Some("minimal")
        );
    }

    #[test]
    fn test_gpt5_mutator_injects_text_verbosity() {
        let cfg = gpt5_model_config(ModelProfile::Gpt5 {
            reasoning_effort: None,
            responses_text_verbosity: Some("medium".into()),
            responses_reasoning_object: None,
        });
        let mut req = LlmRequest::default();
        Gpt5Mutator.mutate(&cfg, &mut req).unwrap();
        let ext = req.extensions.as_ref().unwrap();
        let text = ext.get("text").and_then(|v| v.as_object()).unwrap();
        assert_eq!(
            text.get("verbosity").and_then(|v| v.as_str()),
            Some("medium")
        );
    }

    #[test]
    fn test_gpt5_mutator_injects_reasoning_object() {
        let cfg = gpt5_model_config(ModelProfile::Gpt5 {
            reasoning_effort: Some("high".into()),
            responses_text_verbosity: None,
            responses_reasoning_object: Some(true),
        });
        let mut req = LlmRequest::default();
        Gpt5Mutator.mutate(&cfg, &mut req).unwrap();
        let ext = req.extensions.as_ref().unwrap();
        let reasoning = ext.get("reasoning").and_then(|v| v.as_object()).unwrap();
        assert_eq!(
            reasoning.get("effort").and_then(|v| v.as_str()),
            Some("high")
        );
    }

    #[test]
    fn test_qwen_vllm_extras_injects_all_fields() {
        let cfg = qwen3_model_config(ModelProfile::Qwen3 {
            enable_thinking: Some(true),
            tool_call_parser: Some("hermes".into()),
            reasoning_parser: Some("deepseek_r1".into()),
            auto_tool_choice: Some(true),
            template_kwargs: Some(serde_json::json!({ "custom": "kv" })),
        });
        let mut req = LlmRequest::default();
        QwenVllmExtras.mutate(&cfg, &mut req).unwrap();
        let ext = req.extensions.as_ref().unwrap();
        assert_eq!(
            ext.get("tool_call_parser").and_then(|v| v.as_str()),
            Some("hermes")
        );
        assert_eq!(
            ext.get("reasoning_parser").and_then(|v| v.as_str()),
            Some("deepseek_r1")
        );
        assert_eq!(
            ext.get("enable_auto_tool_choice").and_then(|v| v.as_bool()),
            Some(true)
        );
        let kwargs = ext
            .get("chat_template_kwargs")
            .and_then(|v| v.as_object())
            .unwrap();
        assert_eq!(
            kwargs.get("enable_thinking").and_then(|v| v.as_bool()),
            Some(true)
        );
        assert_eq!(kwargs.get("custom").and_then(|v| v.as_str()), Some("kv"));
    }

    #[test]
    fn test_qwen_vllm_extras_default_thinking_false() {
        let cfg = qwen3_model_config(ModelProfile::Qwen3 {
            enable_thinking: None,
            tool_call_parser: None,
            reasoning_parser: None,
            auto_tool_choice: None,
            template_kwargs: None,
        });
        let mut req = LlmRequest::default();
        QwenVllmExtras.mutate(&cfg, &mut req).unwrap();
        let ext = req.extensions.as_ref().unwrap();
        let kwargs = ext
            .get("chat_template_kwargs")
            .and_then(|v| v.as_object())
            .unwrap();
        assert_eq!(
            kwargs.get("enable_thinking").and_then(|v| v.as_bool()),
            Some(false)
        );
    }

    #[test]
    fn test_profile_validator_strict_rejects_gpt5_temperature() {
        let cfg = gpt5_model_config(ModelProfile::Gpt5 {
            reasoning_effort: None,
            responses_text_verbosity: None,
            responses_reasoning_object: None,
        });
        let req = LlmRequest {
            temperature: Some(0.5),
            ..Default::default()
        };
        let v = ProfileCapabilityValidator;
        let err = v
            .validate(&cfg, &req, Policy::Strict)
            .expect_err("strict should reject temperature");
        assert!(
            err.to_string().contains("temperature"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn test_profile_validator_permissive_allows_gpt5_temperature() {
        let cfg = gpt5_model_config(ModelProfile::Gpt5 {
            reasoning_effort: None,
            responses_text_verbosity: None,
            responses_reasoning_object: None,
        });
        let req = LlmRequest {
            temperature: Some(0.5),
            ..Default::default()
        };
        ProfileCapabilityValidator
            .validate(&cfg, &req, Policy::Permissive)
            .expect("permissive allows temperature");
    }

    #[test]
    fn test_profile_validator_generic_passes_all() {
        let cfg = ModelConfig {
            model_id: "custom".to_string(),
            family: ModelFamily::OpenAI,
            profile: ModelProfile::Generic,
            capabilities: None,
            extensions: BTreeMap::new(),
        };
        let req = LlmRequest {
            temperature: Some(1.0),
            max_tokens: Some(99),
            ..Default::default()
        };
        ProfileCapabilityValidator
            .validate(&cfg, &req, Policy::Strict)
            .expect("generic profile ignores temperature policy");
    }

    #[test]
    fn test_non_matching_family_noop() {
        let cfg = qwen3_model_config(ModelProfile::Qwen3 {
            enable_thinking: None,
            tool_call_parser: Some("hermes".into()),
            reasoning_parser: None,
            auto_tool_choice: None,
            template_kwargs: None,
        });
        let mut req = LlmRequest {
            temperature: Some(0.3),
            max_tokens: Some(512),
            ..Default::default()
        };
        Gpt5Mutator.mutate(&cfg, &mut req).unwrap();
        assert_eq!(req.temperature, Some(0.3));
        assert_eq!(req.max_tokens, Some(512));
        assert!(req.extensions.is_none());
    }

    #[test]
    fn test_prepare_request_empty_mutators_validators() {
        let cfg = gpt5_model_config(ModelProfile::Gpt5 {
            reasoning_effort: None,
            responses_text_verbosity: None,
            responses_reasoning_object: None,
        });
        let req = LlmRequest {
            model: "gpt-5".to_string(),
            temperature: None,
            max_tokens: Some(100),
            ..Default::default()
        };
        let mutators: Vec<Box<dyn RequestMutator>> = vec![];
        let validators: Vec<Box<dyn RequestValidator>> = vec![];
        let out = prepare_request(
            &cfg,
            req.clone(),
            mutators.as_slice(),
            validators.as_slice(),
            Policy::Strict,
        )
        .unwrap();
        assert_eq!(out.model, req.model);
        assert_eq!(out.temperature, req.temperature);
        assert_eq!(out.max_tokens, req.max_tokens);
        assert_eq!(out.extensions, req.extensions);
    }
}
