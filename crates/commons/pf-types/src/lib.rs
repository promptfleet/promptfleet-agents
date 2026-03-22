pub mod labels {
    pub const A2A_TID: &str = "a2a.promptfleet.io/tid";
    pub const A2A_SID: &str = "a2a.promptfleet.io/sid";
    pub const A2A_AID: &str = "a2a.promptfleet.io/aid";
    pub const A2A_SLUG: &str = "a2a.promptfleet.io/slug"; // annotation preferred due to ':'
}

pub mod annotations {
    pub const A2A_SUBJECT: &str = "a2a.promptfleet.io/subject";
}

use core::fmt::{Display, Formatter};
use core::str::FromStr;
use thiserror::Error;
use ulid::Ulid;

#[cfg(feature = "serde")]
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Error)]
pub enum TypeError {
    #[error("invalid ULID: {0}")]
    InvalidUlid(String),
    #[error("invalid prefix: expected {expected}, got {got}")]
    InvalidPrefix { expected: &'static str, got: String },
    #[error("invalid slug segment '{segment}': {reason}")]
    InvalidSlugSegment {
        segment: String,
        reason: &'static str,
    },
    #[error("invalid subject: {0}")]
    InvalidSubject(String),
}

macro_rules! id_newtype {
    ($name:ident, $prefix:literal) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
        pub struct $name(Ulid);

        impl $name {
            pub fn new(ulid: Ulid) -> Self {
                Self(ulid)
            }
            pub fn ulid(&self) -> Ulid {
                self.0
            }
            pub fn to_ulid_string(&self) -> String {
                self.0.to_string()
            }
            pub fn prefix() -> &'static str {
                $prefix
            }
        }

        impl Display for $name {
            fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
                write!(f, concat!($prefix, "{}"), self.0)
            }
        }

        impl FromStr for $name {
            type Err = TypeError;
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                let s_trim = s.trim();
                let raw = if let Some(rest) = s_trim.strip_prefix($prefix) {
                    rest
                } else {
                    s_trim
                };
                let raw_norm = raw.to_lowercase();
                let ulid = Ulid::from_string(&raw_norm)
                    .map_err(|_| TypeError::InvalidUlid(s_trim.to_string()))?;
                Ok(Self(ulid))
            }
        }

        #[cfg(feature = "serde")]
        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(&self.to_string())
            }
        }

        #[cfg(feature = "serde")]
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let s = String::deserialize(deserializer)?;
                s.parse().map_err(serde::de::Error::custom)
            }
        }
    };
}

id_newtype!(AgentId, "A-");
id_newtype!(TenantId, "T-");
id_newtype!(SpaceId, "S-");
id_newtype!(UserId, "U-");

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum CloudRole {
    Owner,
    Admin,
    Operator,
    Developer,
    Viewer,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum Action {
    Create,
    Read,
    Update,
    Delete,
    Deploy,
    ManageCredentials,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum ResourceKind {
    Agent,
    Credential,
    VaultBinding,
    Space,
}

pub trait AuthContext {
    fn user_id(&self) -> &UserId;
    fn tenant_id(&self) -> &TenantId;
    fn space_id(&self) -> &SpaceId;
    fn role(&self) -> CloudRole;
    fn can(&self, action: Action, resource: ResourceKind) -> bool;
}

pub fn cloud_role_allows(role: CloudRole, action: Action, resource: ResourceKind) -> bool {
    use Action::*;
    use CloudRole::*;
    use ResourceKind::*;

    match role {
        Owner => true,
        Admin => !matches!((action, resource), (ManageCredentials, Space)),
        Operator => matches!(
            (action, resource),
            (Create | Read | Update | Delete | Deploy, Agent) | (Read, Credential)
        ),
        Developer => matches!(
            (action, resource),
            (Create | Read | Update | Deploy, Agent) | (Read, Credential)
        ),
        Viewer => matches!((action, resource), (Read, Agent | Credential | Space)),
    }
}

pub mod redis_keys {
    use super::AgentId;
    pub fn agent_live(aid: &AgentId) -> String {
        // agent:live:{aid} -> use bare ULID for compactness
        format!("agent:live:{}", aid.to_ulid_string())
    }
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

    pub fn to_k8s_label_value(&self) -> String {
        // label-safe variant (no ':'); dot-separated is readable
        self.canonical.replace(':', ".")
    }

    pub fn to_subject(&self) -> String {
        let parts: Vec<&str> = self.canonical.split(':').collect();
        format!(
            "tenants/{}/spaces/{}/agents/{}",
            parts[0], parts[1], parts[2]
        )
    }

    pub fn from_subject(subject: &str) -> Result<Self, TypeError> {
        // tenants/{t}/spaces/{s}/agents/{a}
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

fn normalize_name(input: &str) -> Result<String, TypeError> {
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
        } else {
            // drop disallowed character; collapse to '-'
            if !last_was_sep {
                out.push('-');
                last_was_sep = true;
            }
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
    fn id_roundtrip_accepts_prefixed_and_bare() {
        let u = Ulid::new();
        let a1: AgentId = format!("A-{}", u).parse().unwrap();
        let a2: AgentId = u.to_string().parse().unwrap();
        assert_eq!(a1.ulid(), a2.ulid());
        assert_eq!(a1.to_string(), format!("A-{}", u));
    }

    #[test]
    fn redis_key_format() {
        let u = Ulid::new();
        let aid: AgentId = u.to_string().parse().unwrap();
        let key = redis_keys::agent_live(&aid);
        assert_eq!(key, format!("agent:live:{}", u));
    }

    #[test]
    fn slug_normalization_and_conversions() {
        let slug = AgentSlug::from_names(" Acme ", "Prod ", " Trading Agent ").unwrap();
        assert_eq!(slug.canonical(), "acme:prod:trading-agent");
        assert_eq!(slug.to_k8s_label_value(), "acme.prod.trading-agent");
        let subj = slug.to_subject();
        assert_eq!(subj, "tenants/acme/spaces/prod/agents/trading-agent");
        let back = AgentSlug::from_subject(&subj).unwrap();
        assert_eq!(back, slug);
    }

    #[test]
    fn user_id_roundtrip() {
        let u = Ulid::new();
        let uid: UserId = format!("U-{}", u).parse().unwrap();
        assert_eq!(uid.to_string(), format!("U-{}", u));
    }

    #[test]
    fn cloud_role_permissions() {
        assert!(cloud_role_allows(
            CloudRole::Owner,
            Action::ManageCredentials,
            ResourceKind::Space
        ));
        assert!(!cloud_role_allows(
            CloudRole::Viewer,
            Action::Delete,
            ResourceKind::Agent
        ));
        assert!(cloud_role_allows(
            CloudRole::Developer,
            Action::Deploy,
            ResourceKind::Agent
        ));
    }
}
