#[cfg(feature = "cargo-toml")]
pub fn load_table(
    path: &std::path::Path,
    table_path: &[&str],
) -> Result<serde_json::Value, crate::ConfigError> {
    use std::fs;
    use toml::Value as TomlValue;

    let content = fs::read_to_string(path)?;
    let doc: TomlValue = toml::from_str(&content)?;

    // Walk table path
    let mut current = &doc;
    for seg in table_path {
        current = current.get(*seg).ok_or_else(|| {
            crate::ConfigError::NotFound(format!("Missing TOML path segment: {}", seg))
        })?;
    }

    // Convert to serde_json::Value
    let json_val = toml_to_json(current);
    Ok(json_val)
}

#[cfg(feature = "cargo-toml")]
fn toml_to_json(tv: &toml::Value) -> serde_json::Value {
    match tv {
        toml::Value::String(s) => serde_json::Value::String(s.clone()),
        toml::Value::Integer(i) => serde_json::json!(*i),
        toml::Value::Float(f) => serde_json::json!(*f),
        toml::Value::Boolean(b) => serde_json::json!(*b),
        toml::Value::Datetime(dt) => serde_json::Value::String(dt.to_string()),
        toml::Value::Array(arr) => serde_json::Value::Array(arr.iter().map(toml_to_json).collect()),
        toml::Value::Table(map) => {
            let mut m = serde_json::Map::new();
            for (k, v) in map.iter() {
                m.insert(k.clone(), toml_to_json(v));
            }
            serde_json::Value::Object(m)
        }
    }
}
