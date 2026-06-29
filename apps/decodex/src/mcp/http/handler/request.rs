use std::str;

use serde_json::{self, Value};

use crate::{
	mcp::{
		self, MCP_HTTP_ENDPOINT_PATH, MCP_SESSION_HEADER, McpServer,
		http::{
			auth::McpHttpAuthorization,
			handler::sessions::McpHttpSessions,
			message::{self, McpHttpRequest, McpHttpResponse},
			security,
		},
	},
	prelude,
};

pub(in crate::mcp) struct McpHttpHandler {
	pub(in crate::mcp) server: McpServer,
	pub(in crate::mcp) sessions: McpHttpSessions,
	pub(in crate::mcp) allowed_origins: Vec<String>,
	pub(in crate::mcp) listen_address: Option<String>,
	pub(in crate::mcp) authorization: McpHttpAuthorization,
}
impl McpHttpHandler {
	pub(in crate::mcp) fn handle_request_bytes(
		&mut self,
		request: &[u8],
	) -> prelude::Result<Vec<u8>> {
		let request = match McpHttpRequest::parse(request) {
			Ok(request) => request,
			Err(response) => return response.into_bytes(),
		};
		let response = self.handle_request(request)?;

		response.into_bytes()
	}

	fn handle_request(&mut self, request: McpHttpRequest) -> prelude::Result<McpHttpResponse> {
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

	fn handle_options(&self, request: &McpHttpRequest) -> McpHttpResponse {
		let Some(method) = request.header("Access-Control-Request-Method") else {
			return McpHttpResponse::empty("204 No Content");
		};

		if matches!(method.to_ascii_uppercase().as_str(), "POST" | "DELETE") {
			McpHttpResponse::empty("204 No Content")
		} else {
			McpHttpResponse::empty("405 Method Not Allowed")
		}
	}

	fn handle_post(&mut self, request: McpHttpRequest) -> prelude::Result<McpHttpResponse> {
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

	fn handle_delete(&mut self, request: &McpHttpRequest) -> McpHttpResponse {
		let Some(session_id) = request.header(MCP_SESSION_HEADER) else {
			return McpHttpResponse::json_error(
				"428 Precondition Required",
				mcp::json_rpc_error(Value::Null, -32_000, "Missing MCP session"),
			);
		};

		if !self.sessions.remove(session_id) {
			return McpHttpResponse::json_error(
				"404 Not Found",
				mcp::json_rpc_error(Value::Null, -32_001, "Unknown MCP session"),
			);
		}

		McpHttpResponse::empty("202 Accepted")
	}

	fn allowed_cors_origin(
		&self,
		request: &McpHttpRequest,
	) -> std::result::Result<Option<String>, ()> {
		let Some(origin) = request.header("Origin") else {
			return Ok(None);
		};

		if security::mcp_http_origin_is_allowed(
			origin,
			self.listen_address.as_deref(),
			self.allowed_origins.as_slice(),
		) {
			Ok(Some(origin.to_owned()))
		} else {
			Err(())
		}
	}
}
