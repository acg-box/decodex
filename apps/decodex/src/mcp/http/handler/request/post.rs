use std::str;

use serde_json::{self, Value};

use crate::{
	mcp::{
		self, MCP_SESSION_HEADER,
		http::{
			handler::request::McpHttpHandler,
			message::{self, McpHttpRequest, McpHttpResponse},
		},
	},
	prelude::Result,
};

impl McpHttpHandler {
	pub(super) fn handle_post(&mut self, request: McpHttpRequest) -> Result<McpHttpResponse> {
		if !request.content_type_is_json() {
			return Ok(McpHttpResponse::json_error(
				"415 Unsupported Media Type",
				mcp::json_rpc_error(Value::Null, -32_600, "Invalid Request"),
			));
		}

		let body = match str::from_utf8(&request.body) {
			Ok(body) => body,
			Err(_) => {
				return Ok(McpHttpResponse::json_error(
					"400 Bad Request",
					mcp::json_rpc_error(Value::Null, -32_700, "Parse error"),
				));
			},
		};
		let method = message::json_rpc_method_name(body);
		let is_initialize = method.as_deref() == Some("initialize");
		let session_id = request.header(MCP_SESSION_HEADER).map(str::to_owned);

		if method.is_none() && serde_json::from_str::<Value>(body).is_err() {
			return McpHttpResponse::json(
				self.server
					.handle_line(body, false)
					.into_iter()
					.next()
					.unwrap_or_else(|| mcp::json_rpc_error(Value::Null, -32_700, "Parse error")),
				None,
			);
		}

		let wants_sse = request.accepts_sse();

		if is_initialize {
			let responses = self.server.handle_line(body, wants_sse);
			let response_session_id =
				message::initialize_response_succeeded(&responses).then(|| self.sessions.create());

			return message::mcp_http_response_for_server_responses(
				responses,
				wants_sse,
				response_session_id,
			);
		}

		let Some(session_id) = session_id.as_deref() else {
			return Ok(McpHttpResponse::json_error(
				"428 Precondition Required",
				mcp::json_rpc_error(Value::Null, -32_000, "Missing MCP session"),
			));
		};

		if !self.sessions.contains(session_id) {
			return Ok(McpHttpResponse::json_error(
				"404 Not Found",
				mcp::json_rpc_error(Value::Null, -32_001, "Unknown MCP session"),
			));
		}

		let responses = self.server.handle_line(body, wants_sse);

		message::mcp_http_response_for_server_responses(
			responses,
			wants_sse,
			Some(session_id.to_owned()),
		)
	}
}
