use std::time::Duration;

mod auth;
mod handler;
mod message;
mod security;

const MCP_HTTP_READ_TIMEOUT: Duration = Duration::from_secs(2);
const MCP_HTTP_MAX_REQUEST_BYTES: usize = 1_024 * 1_024;
const MCP_CORS_ALLOW_METHODS: &str = "POST, DELETE, OPTIONS";
const MCP_CORS_ALLOW_HEADERS: &str = "Content-Type, Accept, Mcp-Session-Id, Authorization";
const MCP_AUTHORIZATION_HEADER: &str = "Authorization";
const MCP_WWW_AUTHENTICATE_HEADER: &str = "Bearer realm=\"decodex-mcp\"";

pub(super) use auth::McpHttpAuthorization;
pub(super) use handler::serve_streamable_http_with_profile;
#[cfg(test)]
pub(super) use handler::{McpHttpHandler, McpHttpSessions};
#[cfg(test)]
pub(super) use message::http_header_end;
pub(super) use security::{validate_mcp_http_capability_profile, validate_mcp_http_listen_address};
