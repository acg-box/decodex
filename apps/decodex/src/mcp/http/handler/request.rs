mod cors;
mod lifecycle;
mod post;
mod routing;

use crate::{
	mcp::{
		McpServer,
		http::{
			auth::McpHttpAuthorization, handler::sessions::McpHttpSessions, message::McpHttpRequest,
		},
	},
	prelude::Result,
};

pub(in crate::mcp) struct McpHttpHandler {
	pub(in crate::mcp) server: McpServer,
	pub(in crate::mcp) sessions: McpHttpSessions,
	pub(in crate::mcp) allowed_origins: Vec<String>,
	pub(in crate::mcp) listen_address: Option<String>,
	pub(in crate::mcp) authorization: McpHttpAuthorization,
}
impl McpHttpHandler {
	pub(in crate::mcp) fn handle_request_bytes(&mut self, request: &[u8]) -> Result<Vec<u8>> {
		let request = match McpHttpRequest::parse(request) {
			Ok(request) => request,
			Err(response) => return response.into_bytes(),
		};
		let response = self.handle_request(request)?;

		response.into_bytes()
	}
}
