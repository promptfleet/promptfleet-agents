use thiserror::Error;

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON parse error: {0}")]
    JsonParse(#[from] serde_json::Error),

    #[cfg(feature = "cargo-toml")]
    #[error("TOML parse error: {0}")]
    TomlParse(#[from] toml::de::Error),

    #[error("Type mismatch while deserializing: {0}")]
    TypeMismatch(serde_json::Error),

    #[error("Not found: {0}")]
    NotFound(String),
}
