//! # MCP Authentication
//!
//! **OAuth 2.1 Bearer token authentication** for MCP protocol
//! - Standard Authorization header handling
//! - Custom authentication strategies support
//! - External MCP server authentication

use crate::{BearerToken, McpError};
use protocol_transport_core::{ProtocolError, UniversalRequest};

/// **Authentication Handler Trait**
pub trait AuthHandler: Send + Sync {
    /// Validate request authentication
    fn validate_request(&self, request: &UniversalRequest) -> Result<(), ProtocolError>;

    /// Add authentication to outgoing request
    fn add_auth_headers(&self, request: &mut UniversalRequest) -> Result<(), ProtocolError>;
}

/// **Bearer Token Authentication Handler**
pub struct BearerAuthHandler {
    /// Required bearer token (for server mode)
    required_token: Option<String>,
    /// Bearer token for client requests (for client mode)
    client_token: Option<BearerToken>,
}

impl BearerAuthHandler {
    /// Create new bearer auth handler
    pub fn new() -> Self {
        Self {
            required_token: None,
            client_token: None,
        }
    }

    /// Configure required token for server mode
    pub fn with_required_token(mut self, token: &str) -> Self {
        self.required_token = Some(token.to_string());
        self
    }

    /// Configure client token for outgoing requests
    pub fn with_client_token(mut self, token: BearerToken) -> Self {
        self.client_token = Some(token);
        self
    }

    /// Extract bearer token from Authorization header
    fn extract_bearer_token(&self, request: &UniversalRequest) -> Option<String> {
        request
            .headers
            .get("authorization")
            .or_else(|| request.headers.get("Authorization"))
            .and_then(|auth_header| {
                if auth_header.starts_with("Bearer ") {
                    Some(auth_header[7..].to_string())
                } else {
                    None
                }
            })
    }
}

impl AuthHandler for BearerAuthHandler {
    fn validate_request(&self, request: &UniversalRequest) -> Result<(), ProtocolError> {
        // If no token is required, allow all requests
        let required_token = match &self.required_token {
            Some(token) => token,
            None => return Ok(()),
        };

        // Extract token from request
        let provided_token = self.extract_bearer_token(request).ok_or_else(|| {
            ProtocolError::Internal(
                McpError::Authentication("Missing or invalid Authorization header".to_string())
                    .to_string(),
            )
        })?;

        // Validate token
        if provided_token != *required_token {
            return Err(ProtocolError::Internal(
                McpError::Authentication("Invalid bearer token".to_string()).to_string(),
            ));
        }

        Ok(())
    }

    fn add_auth_headers(&self, request: &mut UniversalRequest) -> Result<(), ProtocolError> {
        if let Some(client_token) = &self.client_token {
            request.headers.insert(
                "Authorization".to_string(),
                client_token.to_authorization_header(),
            );
        }
        Ok(())
    }
}

impl Default for BearerAuthHandler {
    fn default() -> Self {
        Self::new()
    }
}

/// **No Authentication Handler** - Allows all requests
pub struct NoAuthHandler;

impl AuthHandler for NoAuthHandler {
    fn validate_request(&self, _request: &UniversalRequest) -> Result<(), ProtocolError> {
        Ok(())
    }

    fn add_auth_headers(&self, _request: &mut UniversalRequest) -> Result<(), ProtocolError> {
        Ok(())
    }
}

/// **Custom Authentication Handler** - User-defined validation
pub struct CustomAuthHandler<F, G>
where
    F: Fn(&UniversalRequest) -> Result<(), ProtocolError> + Send + Sync,
    G: Fn(&mut UniversalRequest) -> Result<(), ProtocolError> + Send + Sync,
{
    validate_fn: F,
    add_auth_fn: G,
}

impl<F, G> CustomAuthHandler<F, G>
where
    F: Fn(&UniversalRequest) -> Result<(), ProtocolError> + Send + Sync,
    G: Fn(&mut UniversalRequest) -> Result<(), ProtocolError> + Send + Sync,
{
    /// Create custom auth handler with validation and auth addition functions
    pub fn new(validate_fn: F, add_auth_fn: G) -> Self {
        Self {
            validate_fn,
            add_auth_fn,
        }
    }
}

impl<F, G> AuthHandler for CustomAuthHandler<F, G>
where
    F: Fn(&UniversalRequest) -> Result<(), ProtocolError> + Send + Sync,
    G: Fn(&mut UniversalRequest) -> Result<(), ProtocolError> + Send + Sync,
{
    fn validate_request(&self, request: &UniversalRequest) -> Result<(), ProtocolError> {
        (self.validate_fn)(request)
    }

    fn add_auth_headers(&self, request: &mut UniversalRequest) -> Result<(), ProtocolError> {
        (self.add_auth_fn)(request)
    }
}

/// **Authentication Builder** - Convenient auth handler creation
pub struct AuthBuilder;

impl AuthBuilder {
    /// Create no authentication handler
    pub fn none() -> NoAuthHandler {
        NoAuthHandler
    }

    /// Create bearer token auth for server
    pub fn bearer_server(required_token: &str) -> BearerAuthHandler {
        BearerAuthHandler::new().with_required_token(required_token)
    }

    /// Create bearer token auth for client
    pub fn bearer_client(token: &str) -> BearerAuthHandler {
        BearerAuthHandler::new().with_client_token(BearerToken::new(token))
    }

    /// Create bearer token auth for both server and client
    pub fn bearer_both(required_token: &str, client_token: &str) -> BearerAuthHandler {
        BearerAuthHandler::new()
            .with_required_token(required_token)
            .with_client_token(BearerToken::new(client_token))
    }
}
