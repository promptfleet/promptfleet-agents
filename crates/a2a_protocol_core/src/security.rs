//! A2A v1.0 Security Scheme Types

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Security scheme union (v1.0).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SecurityScheme {
    #[serde(rename = "apiKey")]
    ApiKey(ApiKeySecurityScheme),
    #[serde(rename = "http")]
    Http(HttpAuthSecurityScheme),
    #[serde(rename = "oauth2")]
    OAuth2(OAuth2SecurityScheme),
    #[serde(rename = "openIdConnect")]
    OpenIdConnect(OpenIdConnectSecurityScheme),
    #[serde(rename = "mutualTLS")]
    MutualTls(MutualTlsSecurityScheme),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeySecurityScheme {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub location: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpAuthSecurityScheme {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub scheme: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bearer_format: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuth2SecurityScheme {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub flows: OAuthFlows,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oauth2_metadata_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenIdConnectSecurityScheme {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub open_id_connect_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MutualTlsSecurityScheme {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthFlows {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authorization_code: Option<AuthorizationCodeOAuthFlow>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_credentials: Option<ClientCredentialsOAuthFlow>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_code: Option<DeviceCodeOAuthFlow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthorizationCodeOAuthFlow {
    pub authorization_url: String,
    pub token_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scopes: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pkce_required: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientCredentialsOAuthFlow {
    pub token_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scopes: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceCodeOAuthFlow {
    pub device_authorization_url: String,
    pub token_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scopes: Option<HashMap<String, String>>,
}

/// Security requirement: map of scheme name -> required scopes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityRequirement {
    #[serde(flatten)]
    pub schemes: HashMap<String, Vec<String>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_http_bearer_scheme() {
        let scheme = SecurityScheme::Http(HttpAuthSecurityScheme {
            description: Some("Bearer auth".to_string()),
            scheme: "Bearer".to_string(),
            bearer_format: Some("JWT".to_string()),
        });
        let json = serde_json::to_value(&scheme).unwrap();
        assert_eq!(json["type"], "http");
        assert_eq!(json["scheme"], "Bearer");
    }

    #[test]
    fn test_api_key_scheme() {
        let scheme = SecurityScheme::ApiKey(ApiKeySecurityScheme {
            description: None,
            location: "header".to_string(),
            name: "X-API-Key".to_string(),
        });
        let json = serde_json::to_value(&scheme).unwrap();
        assert_eq!(json["type"], "apiKey");
        assert_eq!(json["name"], "X-API-Key");
    }

    #[test]
    fn test_oauth2_authorization_code_roundtrip() {
        let scheme = SecurityScheme::OAuth2(OAuth2SecurityScheme {
            description: Some("OAuth2 with PKCE".to_string()),
            flows: OAuthFlows {
                authorization_code: Some(AuthorizationCodeOAuthFlow {
                    authorization_url: "https://auth.example.com/authorize".to_string(),
                    token_url: "https://auth.example.com/token".to_string(),
                    refresh_url: None,
                    scopes: Some([("read".to_string(), "Read access".to_string())].into()),
                    pkce_required: Some(true),
                }),
                client_credentials: None,
                device_code: None,
            },
            oauth2_metadata_url: None,
        });
        let json = serde_json::to_value(&scheme).unwrap();
        assert_eq!(json["type"], "oauth2");
        assert_eq!(json["flows"]["authorizationCode"]["pkceRequired"], true);
        let deser: SecurityScheme = serde_json::from_value(json).unwrap();
        assert!(matches!(deser, SecurityScheme::OAuth2(_)));
    }

    #[test]
    fn test_oauth2_client_credentials_roundtrip() {
        let scheme = SecurityScheme::OAuth2(OAuth2SecurityScheme {
            description: None,
            flows: OAuthFlows {
                authorization_code: None,
                client_credentials: Some(ClientCredentialsOAuthFlow {
                    token_url: "https://auth.example.com/token".to_string(),
                    refresh_url: None,
                    scopes: None,
                }),
                device_code: None,
            },
            oauth2_metadata_url: None,
        });
        let json = serde_json::to_value(&scheme).unwrap();
        assert_eq!(json["type"], "oauth2");
        assert!(json["flows"]["clientCredentials"].is_object());
    }

    #[test]
    fn test_openid_connect_roundtrip() {
        let scheme = SecurityScheme::OpenIdConnect(OpenIdConnectSecurityScheme {
            description: None,
            open_id_connect_url: "https://auth.example.com/.well-known/openid-configuration"
                .to_string(),
        });
        let json = serde_json::to_value(&scheme).unwrap();
        assert_eq!(json["type"], "openIdConnect");
        assert!(
            json["openIdConnectUrl"]
                .as_str()
                .unwrap()
                .contains("openid-configuration")
        );
        let deser: SecurityScheme = serde_json::from_value(json).unwrap();
        assert!(matches!(deser, SecurityScheme::OpenIdConnect(_)));
    }

    #[test]
    fn test_mutual_tls_roundtrip() {
        let scheme = SecurityScheme::MutualTls(MutualTlsSecurityScheme { description: None });
        let json = serde_json::to_value(&scheme).unwrap();
        assert_eq!(json["type"], "mutualTLS");
        let deser: SecurityScheme = serde_json::from_value(json).unwrap();
        assert!(matches!(deser, SecurityScheme::MutualTls(_)));
    }

    #[test]
    fn test_security_requirement_with_scopes() {
        use std::collections::HashMap;
        let mut schemes = HashMap::new();
        schemes.insert(
            "oauth2".to_string(),
            vec!["read".to_string(), "write".to_string()],
        );
        let req = SecurityRequirement { schemes };
        let json = serde_json::to_value(&req).unwrap();
        let scopes = json["oauth2"].as_array().unwrap();
        assert_eq!(scopes.len(), 2);
        let deser: SecurityRequirement = serde_json::from_value(json).unwrap();
        assert_eq!(deser.schemes["oauth2"], vec!["read", "write"]);
    }
}
