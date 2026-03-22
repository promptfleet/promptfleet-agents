use crate::{ConfigError, EnvKeyTransform};
use serde_json::{json, Value};

pub fn read_env(
    enable_dotenv: bool,
    prefix: Option<&'static str>,
    key_transform: EnvKeyTransform,
) -> Result<Value, ConfigError> {
    // Load .env first if enabled
    #[cfg(feature = "dotenv")]
    if enable_dotenv {
        let _ = dotenvy::dotenv();
    }

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
    // Try JSON object/array
    if (trimmed.starts_with('{') && trimmed.ends_with('}'))
        || (trimmed.starts_with('[') && trimmed.ends_with(']'))
    {
        if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
            return v;
        }
    }
    // Bool
    match trimmed.to_ascii_lowercase().as_str() {
        "true" => return Value::Bool(true),
        "false" => return Value::Bool(false),
        _ => {}
    }
    // Integer
    if let Ok(i) = trimmed.parse::<i64>() {
        return json!(i);
    }
    // Float
    if let Ok(f) = trimmed.parse::<f64>() {
        return json!(f);
    }
    // Fallback to string
    Value::String(trimmed.to_string())
}
