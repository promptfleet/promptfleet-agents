use crate::{ConfigError, EnvKeyTransform};
use serde_json::{json, Value};

pub fn read_env(
    enable_dotenv: bool,
    prefix: Option<&'static str>,
    key_transform: EnvKeyTransform,
) -> Result<Value, ConfigError> {
    #[cfg(feature = "dotenv")]
    if enable_dotenv {
        let _ = dotenvy::dotenv();
    }
    #[cfg(not(feature = "dotenv"))]
    let _ = enable_dotenv;

    let mut root = serde_json::Map::new();
    let prefix_upper = prefix.map(|p| p.to_ascii_uppercase());

    for (key, val) in std::env::vars() {
        if let Some(pfx) = &prefix_upper {
            if !key.starts_with(pfx) {
                continue;
            }
        }
        let trimmed_key = if let Some(pfx) = &prefix_upper {
            key.trim_start_matches(pfx)
                .trim_start_matches('_')
                .to_string()
        } else {
            key.clone()
        };
        if trimmed_key.is_empty() {
            continue;
        }

        // Transform KEY like HTTP__PORT or LOG__LEVEL
        let parts: Vec<String> = match key_transform {
            EnvKeyTransform::DoubleUnderscoreToNested => trimmed_key
                .split("__")
                .map(|s| s.to_ascii_lowercase())
                .collect(),
        };
        if parts.is_empty() {
            continue;
        }

        let parsed_value = coerce_env_value(&val);

        // Insert into nested map
        insert_nested(&mut root, &parts, parsed_value);
    }

    Ok(Value::Object(root))
}

fn insert_nested(root: &mut serde_json::Map<String, Value>, keys: &[String], value: Value) {
    if keys.len() == 1 {
        root.insert(keys[0].clone(), value);
        return;
    }
    let head = &keys[0];
    let tail = &keys[1..];
    let entry = root
        .entry(head.clone())
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    if let Value::Object(map) = entry {
        insert_nested(map, tail, value);
    } else {
        // Overwrite non-object with object nesting
        let mut new_map = serde_json::Map::new();
        insert_nested(&mut new_map, tail, value);
        root.insert(head.clone(), Value::Object(new_map));
    }
}

fn coerce_env_value(s: &str) -> Value {
    let trimmed = s.trim();
    if (trimmed.starts_with('{') && trimmed.ends_with('}'))
        || (trimmed.starts_with('[') && trimmed.ends_with(']'))
    {
        if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
            return v;
        }
    }
    match trimmed.to_ascii_lowercase().as_str() {
        "true" => return Value::Bool(true),
        "false" => return Value::Bool(false),
        _ => {}
    }
    if let Ok(i) = trimmed.parse::<i64>() {
        return json!(i);
    }
    if let Ok(f) = trimmed.parse::<f64>() {
        return json!(f);
    }
    Value::String(trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::LazyLock;
    use std::sync::Mutex;

    static ENV_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    fn with_env_vars<F, R>(vars: &[(&str, &str)], f: F) -> R
    where
        F: FnOnce() -> R,
    {
        let _guard = ENV_LOCK.lock().unwrap();
        for (k, v) in vars {
            std::env::set_var(k, v);
        }
        let result = f();
        for (k, _) in vars {
            std::env::remove_var(k);
        }
        result
    }

    #[test]
    fn prefix_filtering_strips_prefix() {
        let result = with_env_vars(
            &[
                ("PFCFG1_NAME", "alice"),
                ("PFCFG1_PORT", "8080"),
                ("UNRELATED_KEY", "ignored"),
            ],
            || {
                read_env(
                    false,
                    Some("PFCFG1_"),
                    EnvKeyTransform::DoubleUnderscoreToNested,
                )
            },
        )
        .unwrap();

        assert_eq!(result["name"], json!("alice"));
        assert_eq!(result["port"], json!(8080));
        assert!(result.get("unrelated_key").is_none());
        assert!(result.get("UNRELATED_KEY").is_none());
    }

    #[test]
    fn double_underscore_creates_nested_objects() {
        let result = with_env_vars(
            &[("PFCFG2_HTTP__PORT", "3000"), ("PFCFG2_DB__HOST", "pg")],
            || {
                read_env(
                    false,
                    Some("PFCFG2_"),
                    EnvKeyTransform::DoubleUnderscoreToNested,
                )
            },
        )
        .unwrap();

        assert_eq!(result["http"]["port"], json!(3000));
        assert_eq!(result["db"]["host"], json!("pg"));
    }

    #[test]
    fn triple_nesting_via_double_underscore() {
        let result = with_env_vars(&[("PFCFG3_A__B__C", "deep")], || {
            read_env(
                false,
                Some("PFCFG3_"),
                EnvKeyTransform::DoubleUnderscoreToNested,
            )
        })
        .unwrap();

        assert_eq!(result["a"]["b"]["c"], json!("deep"));
    }

    #[test]
    fn coerce_integer() {
        assert_eq!(coerce_env_value("42"), json!(42));
        assert_eq!(coerce_env_value("-7"), json!(-7));
        assert_eq!(coerce_env_value("0"), json!(0));
    }

    #[test]
    fn coerce_float() {
        assert_eq!(coerce_env_value("3.14"), json!(3.14));
        assert_eq!(coerce_env_value("-0.5"), json!(-0.5));
    }

    #[test]
    fn coerce_boolean() {
        assert_eq!(coerce_env_value("true"), json!(true));
        assert_eq!(coerce_env_value("True"), json!(true));
        assert_eq!(coerce_env_value("FALSE"), json!(false));
    }

    #[test]
    fn coerce_plain_string() {
        assert_eq!(coerce_env_value("hello"), json!("hello"));
        assert_eq!(coerce_env_value("  spaced  "), json!("spaced"));
    }

    #[test]
    fn coerce_json_object() {
        let v = coerce_env_value(r#"{"a": 1}"#);
        assert_eq!(v, json!({"a": 1}));
    }

    #[test]
    fn coerce_json_array() {
        let v = coerce_env_value("[1, 2, 3]");
        assert_eq!(v, json!([1, 2, 3]));
    }

    #[test]
    fn coerce_malformed_json_falls_back_to_string() {
        let v = coerce_env_value("{not json}");
        assert_eq!(v, json!("{not json}"));
    }

    #[test]
    fn no_prefix_reads_all_vars() {
        let result = with_env_vars(&[("PFCFG4_NOPREFIX_KEY", "val")], || {
            read_env(false, None, EnvKeyTransform::DoubleUnderscoreToNested)
        })
        .unwrap();

        assert_eq!(result["pfcfg4_noprefix_key"], json!("val"));
    }

    #[test]
    fn insert_nested_overwrites_non_object_with_nesting() {
        let mut root = serde_json::Map::new();
        root.insert("a".to_string(), json!("flat"));
        insert_nested(
            &mut root,
            &["a".to_string(), "b".to_string()],
            json!("deep"),
        );
        assert_eq!(root["a"]["b"], json!("deep"));
    }
}
