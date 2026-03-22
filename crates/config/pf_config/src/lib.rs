//! pf_config - Layered configuration loader
//!
//! Sources (in precedence, low -> high):
//! - Cargo.toml section (feature: cargo-toml)
//! - JSON files
//! - .env (feature: dotenv)
//! - OS environment variables
//!
//! This crate is target-agnostic by design; initial implementation is native-only.

mod env;
mod error;
mod json;
mod merge;

#[cfg(feature = "cargo-toml")]
mod cargo_toml;

pub use crate::error::ConfigError;

use serde::de::DeserializeOwned;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug)]
pub enum EnvKeyTransform {
    /// Convert PREFIX_A__B__C to nested { "a": { "b": { "c": value } } }
    DoubleUnderscoreToNested,
}

#[derive(Clone, Debug)]
pub struct LoadOptions {
    pub json_paths: Vec<PathBuf>,
    pub enable_dotenv: bool,
    pub env_prefix: Option<&'static str>,
    pub env_key_transform: EnvKeyTransform,
    pub required: bool,
    #[cfg(feature = "cargo-toml")]
    pub cargo: Option<CargoTomlOptions>,
}

#[cfg(feature = "cargo-toml")]
#[derive(Clone, Debug)]
pub struct CargoTomlOptions {
    pub path: PathBuf,
    pub table_path: Vec<&'static str>,
}

impl Default for LoadOptions {
    fn default() -> Self {
        Self {
            json_paths: vec![PathBuf::from("./config.json")],
            enable_dotenv: true,
            env_prefix: Some("AGENT_"),
            env_key_transform: EnvKeyTransform::DoubleUnderscoreToNested,
            required: false,
            #[cfg(feature = "cargo-toml")]
            cargo: None,
        }
    }
}

pub fn load_config_untyped(opts: LoadOptions) -> Result<serde_json::Value, ConfigError> {
    // 1) Cargo.toml section (optional, lowest precedence)
    let mut acc = serde_json::Value::Null;

    #[cfg(feature = "cargo-toml")]
    if let Some(c) = &opts.cargo {
        let from_toml = cargo_toml::load_table(&c.path, &c.table_path)?;
        acc = merge::deep_merge(acc, from_toml);
    }

    // 2) JSON files (in order; later wins)
    for p in &opts.json_paths {
        if let Some(v) = json::try_load_json_file(p)? {
            acc = merge::deep_merge(acc, v);
        }
    }

    // 3) .env (optional) + 4) OS env into map, then merge
    let env_map = env::read_env(opts.enable_dotenv, opts.env_prefix, opts.env_key_transform)?;
    acc = merge::deep_merge(acc, env_map);

    if acc.is_null() && opts.required {
        return Err(ConfigError::NotFound(
            "no configuration sources found".to_string(),
        ));
    }

    Ok(acc)
}

pub fn load_config<T: DeserializeOwned>(opts: LoadOptions) -> Result<T, ConfigError> {
    let v = load_config_untyped(opts)?;
    let cfg: T = serde_json::from_value(v).map_err(ConfigError::TypeMismatch)?;
    Ok(cfg)
}
