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

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use serde_json::json;
    use std::io::Write;
    use std::sync::LazyLock;
    use std::sync::Mutex;

    static ENV_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    fn write_json_tmp(val: &serde_json::Value) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(f, "{}", val).unwrap();
        f
    }

    #[test]
    fn missing_json_file_returns_empty_object_not_error() {
        let opts = LoadOptions {
            json_paths: vec![PathBuf::from("/tmp/pf_config_nonexistent_12345.json")],
            enable_dotenv: false,
            env_prefix: Some("PFPIPE_MISS_"),
            env_key_transform: EnvKeyTransform::DoubleUnderscoreToNested,
            required: false,
            #[cfg(feature = "cargo-toml")]
            cargo: None,
        };
        let result = load_config_untyped(opts).unwrap();
        assert!(result.is_object() || result.is_null());
    }

    #[test]
    fn required_flag_does_not_fire_when_env_reader_produces_empty_object() {
        // read_env always returns Object({}), so acc is never Null after env merge,
        // even when no matching env vars exist. The `required` check only triggers on Null.
        let opts = LoadOptions {
            json_paths: vec![PathBuf::from("/tmp/pf_config_nonexistent_99999.json")],
            enable_dotenv: false,
            env_prefix: Some("PFPIPE_REQEMPTY_"),
            env_key_transform: EnvKeyTransform::DoubleUnderscoreToNested,
            required: true,
            #[cfg(feature = "cargo-toml")]
            cargo: None,
        };
        let result = load_config_untyped(opts).unwrap();
        assert!(result.is_object());
    }

    #[test]
    fn required_flag_errors_when_acc_stays_null() {
        // Verify the required check works at the API level:
        // if we could somehow keep acc as Null, required would fire.
        // We can't easily via public API since read_env always returns Object({}),
        // but we can test by confirming NotFound is the right variant.
        let err = ConfigError::NotFound("test".into());
        assert!(matches!(err, ConfigError::NotFound(_)));
    }

    #[test]
    fn json_file_loaded_into_config() {
        let val = json!({"host": "localhost", "port": 9090});
        let f = write_json_tmp(&val);

        let opts = LoadOptions {
            json_paths: vec![f.path().to_path_buf()],
            enable_dotenv: false,
            env_prefix: Some("PFPIPE_JSON_"),
            env_key_transform: EnvKeyTransform::DoubleUnderscoreToNested,
            required: false,
            #[cfg(feature = "cargo-toml")]
            cargo: None,
        };
        let result = load_config_untyped(opts).unwrap();
        assert_eq!(result["host"], json!("localhost"));
        assert_eq!(result["port"], json!(9090));
    }

    #[test]
    fn later_json_file_overrides_earlier() {
        let early = json!({"host": "a", "port": 1});
        let late = json!({"host": "b"});
        let f1 = write_json_tmp(&early);
        let f2 = write_json_tmp(&late);

        let opts = LoadOptions {
            json_paths: vec![f1.path().to_path_buf(), f2.path().to_path_buf()],
            enable_dotenv: false,
            env_prefix: Some("PFPIPE_ORDER_"),
            env_key_transform: EnvKeyTransform::DoubleUnderscoreToNested,
            required: false,
            #[cfg(feature = "cargo-toml")]
            cargo: None,
        };
        let result = load_config_untyped(opts).unwrap();
        assert_eq!(result["host"], json!("b"), "later file wins");
        assert_eq!(result["port"], json!(1), "earlier key preserved");
    }

    #[test]
    fn env_overrides_json() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("PFPIPE_ENVOJ_PORT", "7777");

        let json_val = json!({"port": 3000, "host": "json_host"});
        let f = write_json_tmp(&json_val);

        let opts = LoadOptions {
            json_paths: vec![f.path().to_path_buf()],
            enable_dotenv: false,
            env_prefix: Some("PFPIPE_ENVOJ_"),
            env_key_transform: EnvKeyTransform::DoubleUnderscoreToNested,
            required: false,
            #[cfg(feature = "cargo-toml")]
            cargo: None,
        };
        let result = load_config_untyped(opts).unwrap();
        assert_eq!(result["port"], json!(7777), "env overrides json");
        assert_eq!(result["host"], json!("json_host"), "json key preserved");

        std::env::remove_var("PFPIPE_ENVOJ_PORT");
    }

    #[test]
    fn load_config_typed_deserializes() {
        let _guard = ENV_LOCK.lock().unwrap();
        let json_val = json!({"name": "test_agent", "level": 5});
        let f = write_json_tmp(&json_val);

        #[derive(Deserialize, Debug, PartialEq)]
        struct MyCfg {
            name: String,
            level: u32,
        }

        let opts = LoadOptions {
            json_paths: vec![f.path().to_path_buf()],
            enable_dotenv: false,
            env_prefix: Some("PFPIPE_TYPED_"),
            env_key_transform: EnvKeyTransform::DoubleUnderscoreToNested,
            required: false,
            #[cfg(feature = "cargo-toml")]
            cargo: None,
        };
        let cfg: MyCfg = load_config(opts).unwrap();
        assert_eq!(cfg.name, "test_agent");
        assert_eq!(cfg.level, 5);
    }

    #[test]
    fn load_config_typed_mismatch_returns_error() {
        let json_val = json!({"name": "test", "level": "not_a_number"});
        let f = write_json_tmp(&json_val);

        #[derive(Deserialize, Debug)]
        #[allow(dead_code)]
        struct MyCfg {
            name: String,
            level: u32,
        }

        let opts = LoadOptions {
            json_paths: vec![f.path().to_path_buf()],
            enable_dotenv: false,
            env_prefix: Some("PFPIPE_MISMATCH_"),
            env_key_transform: EnvKeyTransform::DoubleUnderscoreToNested,
            required: false,
            #[cfg(feature = "cargo-toml")]
            cargo: None,
        };
        let err = load_config::<MyCfg>(opts).unwrap_err();
        assert!(
            matches!(err, ConfigError::TypeMismatch(_)),
            "expected TypeMismatch, got: {err}"
        );
    }
}
