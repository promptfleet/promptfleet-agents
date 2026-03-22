use crate::ConfigError;
use serde_json::Value;
use std::path::Path;

pub fn try_load_json_file(path: &Path) -> Result<Option<Value>, ConfigError> {
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(path)?;
    if content.trim().is_empty() {
        return Ok(None);
    }
    let v: Value = serde_json::from_str(&content)?;
    Ok(Some(v))
}
