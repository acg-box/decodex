use serde_json::Value;

use crate::{
	mcp::{
		self, MCP_HTTP_ENDPOINT_PATH,
		http::{
			auth::McpHttpAuthorization,
			handler::request::McpHttpHandler,
			message::{McpHttpRequest, McpHttpResponse},
		},
	},
	prelude::Result,
};

impl McpHttpHandler {
	pub(super) fn handle_request(&mut self, request: McpHttpRequest) -> Result<McpHttpResponse> {
		let cors_origin = match self.allowed_cors_origin(&request) {
			Ok(origin) => origin,
			Err(()) => {
				return Ok(McpHttpResponse::json_error(
					"403 Forbidden",
					mcp::json_rpc_error(Value::Null, -32_000, "Forbidden origin"),
				));
			},
		};
		let mut response = if request.path != MCP_HTTP_ENDPOINT_PATH {
			McpHttpResponse::empty("404 Not Found")
		} else if request.method != "OPTIONS" && !self.authorization.request_is_authorized(&request)
		{
			McpHttpAuthorization::unauthorized_response()
		} else {
			match request.method.as_str() {
				"OPTIONS" => self.handle_options(&request),
				"POST" => self.handle_post(request)?,
				"DELETE" => self.handle_delete(&request),
				_ => McpHttpResponse::empty("405 Method Not Allowed"),
			}
		};

		response.add_cors_headers(cors_origin);

		Ok(response)
	}
}
