//! Lightweight naming and validation primitives for the PromptFleet agent ecosystem.
//!
//! This crate provides [`AgentSlug`] (the canonical `tenant:space:agent` address)
//! and name normalisation helpers. It has **no** runtime dependencies beyond
//! `thiserror` and optional `serde` — intentionally kept small so WASM agents
//! and third-party integrations can depend on it without pulling in cloud
//! platform concerns.

use core::fmt::{Display, Formatter};
use core::str::FromStr;
use thiserror::Error;

#[cfg(feature = "serde")]
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Error)]
pub enum TypeError {
    #[error("invalid slug segment '{segment}': {reason}")]
    InvalidSlugSegment {
        segment: String,
        reason: &'static str,
    },
    #[error("invalid subject: {0}")]
    InvalidSubject(String),
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct AgentSlug {
    canonical: String, // tenant:space:agent (normalized)
}

impl AgentSlug {
    pub fn canonical(&self) -> &str {
        &self.canonical
    }
    pub fn into_string(self) -> String {
        self.canonical
    }

    pub fn from_canonical(input: &str) -> Result<Self, TypeError> {
        let parts: Vec<&str> = input.split(':').collect();
        if parts.len() != 3 {
            return Err(TypeError::InvalidSlugSegment {
                segment: input.to_string(),
                reason: "expected three segments",
            });
        }
        let t = normalize_name(parts[0])?;
        let s = normalize_name(parts[1])?;
        let a = normalize_name(parts[2])?;
        Ok(Self {
            canonical: format!("{}:{}:{}", t, s, a),
        })
    }

    pub fn from_names(tenant: &str, space: &str, agent: &str) -> Result<Self, TypeError> {
        let t = normalize_name(tenant)?;
        let s = normalize_name(space)?;
        let a = normalize_name(agent)?;
        Ok(Self {
            canonical: format!("{}:{}:{}", t, s, a),
        })
    }

    pub fn to_subject(&self) -> String {
        let parts: Vec<&str> = self.canonical.split(':').collect();
        format!(
            "tenants/{}/spaces/{}/agents/{}",
            parts[0], parts[1], parts[2]
        )
    }

    pub fn from_subject(subject: &str) -> Result<Self, TypeError> {
        let segments: Vec<&str> = subject.split('/').collect();
        if segments.len() != 6
            || segments[0] != "tenants"
            || segments[2] != "spaces"
            || segments[4] != "agents"
        {
            return Err(TypeError::InvalidSubject(subject.to_string()));
        }
        Self::from_names(segments[1], segments[3], segments[5])
    }
}

impl Display for AgentSlug {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.canonical)
    }
}

impl FromStr for AgentSlug {
    type Err = TypeError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_canonical(s)
    }
}

#[cfg(feature = "serde")]
impl Serialize for AgentSlug {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.canonical())
    }
}

#[cfg(feature = "serde")]
impl<'de> Deserialize<'de> for AgentSlug {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        AgentSlug::from_canonical(&s).map_err(serde::de::Error::custom)
    }
}

/// Normalize a name segment to a K8s-label-safe, lowercase string.
///
/// Public so downstream crates can apply the same normalisation rules.
pub fn normalize_name(input: &str) -> Result<String, TypeError> {
    let lowered = input.trim().to_lowercase();
    let mut out = String::with_capacity(lowered.len());
    let mut last_was_sep = false;
    for ch in lowered.chars() {
        let is_allowed =
            ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, '-' | '_' | '.');
        if is_allowed {
            out.push(ch);
            last_was_sep = false;
        } else if ch.is_whitespace() || ch == ':' || ch == '/' {
            if !last_was_sep {
                out.push('-');
                last_was_sep = true;
            }
        } else if !last_was_sep {
            out.push('-');
            last_was_sep = true;
        }
    }
    let normalized = out.trim_matches(['-', '.']).to_string();
    if normalized.is_empty() {
        return Err(TypeError::InvalidSlugSegment {
            segment: input.to_string(),
            reason: "empty after normalization",
        });
    }
    if normalized.len() > 63 {
        return Err(TypeError::InvalidSlugSegment {
            segment: input.to_string(),
            reason: "segment too long (>63)",
        });
    }
    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_normalization_and_conversions() {
        let slug = AgentSlug::from_names(" Acme ", "Prod ", " Trading Agent ").unwrap();
        assert_eq!(slug.canonical(), "acme:prod:trading-agent");
        let subj = slug.to_subject();
        assert_eq!(subj, "tenants/acme/spaces/prod/agents/trading-agent");
        let back = AgentSlug::from_subject(&subj).unwrap();
        assert_eq!(back, slug);
    }

    #[test]
    fn slug_from_canonical() {
        let slug: AgentSlug = "acme:dev:echo-bot".parse().unwrap();
        assert_eq!(slug.canonical(), "acme:dev:echo-bot");
    }

    #[test]
    fn slug_rejects_wrong_segment_count() {
        assert!(AgentSlug::from_canonical("only-two:segments").is_err());
        assert!(AgentSlug::from_canonical("too:many:segments:here").is_err());
    }

    #[test]
    fn normalize_name_basics() {
        assert_eq!(normalize_name("Hello World").unwrap(), "hello-world");
        assert_eq!(normalize_name("  UPPER  ").unwrap(), "upper");
        assert!(normalize_name("").is_err());
    }

    #[test]
    fn normalize_name_rejects_too_long() {
        let long = "a".repeat(64);
        assert!(normalize_name(&long).is_err());
        assert!(normalize_name(&"a".repeat(63)).is_ok());
    }

    #[test]
    fn subject_roundtrip() {
        let slug = AgentSlug::from_names("acme", "staging", "weather-bot").unwrap();
        let subject = slug.to_subject();
        assert_eq!(subject, "tenants/acme/spaces/staging/agents/weather-bot");
        assert_eq!(AgentSlug::from_subject(&subject).unwrap(), slug);
    }

    #[test]
    fn subject_rejects_invalid() {
        assert!(AgentSlug::from_subject("invalid/path").is_err());
        assert!(AgentSlug::from_subject("tenants/a/wrong/b/agents/c").is_err());
    }
}
