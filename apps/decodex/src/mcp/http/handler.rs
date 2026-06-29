use std::{
	collections::BTreeSet,
	io::{ErrorKind, Read as _, Write as _},
	net::{TcpListener, TcpStream},
	str,
};

use serde_json::{self, Value};

use crate::prelude::{Result, eyre};

use super::{
	super::{
		MCP_HTTP_ENDPOINT_PATH, MCP_SESSION_HEADER, McpCapabilityProfile, McpContext, McpServer,
		McpTransport, json_rpc_error,
	},
	MCP_HTTP_MAX_REQUEST_BYTES, MCP_HTTP_READ_TIMEOUT,
	auth::McpHttpAuthorization,
	message::{
		McpHttpRequest, McpHttpResponse, http_content_length, http_header_end,
		initialize_response_succeeded, json_rpc_method_name,
		mcp_http_response_for_server_responses,
	},
	security::mcp_http_origin_is_allowed,
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

	fn handle_request(&mut self, request: McpHttpRequest) -> Result<McpHttpResponse> {
		let cors_origin = match self.allowed_cors_origin(&request) {
			Ok(origin) => origin,
			Err(()) => {
				return Ok(McpHttpResponse::json_error(
					"403 Forbidden",
					json_rpc_error(Value::Null, -32_000, "Forbidden origin"),
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

	fn handle_post(&mut self, request: McpHttpRequest) -> Result<McpHttpResponse> {
		if !request.content_type_is_json() {
			return Ok(McpHttpResponse::json_error(
				"415 Unsupported Media Type",
				json_rpc_error(Value::Null, -32_600, "Invalid Request"),
			));
		}

		let body = match str::from_utf8(&request.body) {
			Ok(body) => body,
			Err(_) => {
				return Ok(McpHttpResponse::json_error(
					"400 Bad Request",
					json_rpc_error(Value::Null, -32_700, "Parse error"),
				));
			},
		};
		let method = json_rpc_method_name(body);
		let is_initialize = method.as_deref() == Some("initialize");
		let session_id = request.header(MCP_SESSION_HEADER).map(str::to_owned);

		if method.is_none() && serde_json::from_str::<Value>(body).is_err() {
			return McpHttpResponse::json(
				self.server
					.handle_line(body, false)
					.into_iter()
					.next()
					.unwrap_or_else(|| json_rpc_error(Value::Null, -32_700, "Parse error")),
				None,
			);
		}

		let wants_sse = request.accepts_sse();

		if is_initialize {
			let responses = self.server.handle_line(body, wants_sse);
			let response_session_id =
				initialize_response_succeeded(&responses).then(|| self.sessions.create());

			return mcp_http_response_for_server_responses(
				responses,
				wants_sse,
				response_session_id,
			);
		}

		let Some(session_id) = session_id.as_deref() else {
			return Ok(McpHttpResponse::json_error(
				"428 Precondition Required",
				json_rpc_error(Value::Null, -32_000, "Missing MCP session"),
			));
		};

		if !self.sessions.contains(session_id) {
			return Ok(McpHttpResponse::json_error(
				"404 Not Found",
				json_rpc_error(Value::Null, -32_001, "Unknown MCP session"),
			));
		}

		let responses = self.server.handle_line(body, wants_sse);

		mcp_http_response_for_server_responses(responses, wants_sse, Some(session_id.to_owned()))
	}

	fn handle_delete(&mut self, request: &McpHttpRequest) -> McpHttpResponse {
		let Some(session_id) = request.header(MCP_SESSION_HEADER) else {
			return McpHttpResponse::json_error(
				"428 Precondition Required",
				json_rpc_error(Value::Null, -32_000, "Missing MCP session"),
			);
		};

		if !self.sessions.remove(session_id) {
			return McpHttpResponse::json_error(
				"404 Not Found",
				json_rpc_error(Value::Null, -32_001, "Unknown MCP session"),
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

		if mcp_http_origin_is_allowed(
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

#[derive(Default)]
pub(in crate::mcp) struct McpHttpSessions {
	active: BTreeSet<String>,
	next_id: u64,
}

impl McpHttpSessions {
	fn create(&mut self) -> String {
		self.next_id = self.next_id.saturating_add(1);

		let session_id = format!("decodex-mcp-session-{:016x}", self.next_id);

		self.active.insert(session_id.clone());

		session_id
	}

	fn contains(&self, session_id: &str) -> bool {
		self.active.contains(session_id)
	}

	fn remove(&mut self, session_id: &str) -> bool {
		self.active.remove(session_id)
	}
}

pub(in crate::mcp) fn serve_streamable_http_with_profile(
	listener: TcpListener,
	context: McpContext,
	capability_profile: McpCapabilityProfile,
	allowed_origins: Vec<String>,
	authorization: McpHttpAuthorization,
) -> Result<()> {
	let mut handler = McpHttpHandler {
		server: McpServer { context, capability_profile, transport: McpTransport::StreamableHttp },
		sessions: McpHttpSessions::default(),
		allowed_origins,
		listen_address: listener.local_addr().map(|address| address.to_string()).ok(),
		authorization,
	};

	for stream in listener.incoming() {
		match stream {
			Ok(mut stream) => {
				if let Err(error) = handle_mcp_http_stream(&mut stream, &mut handler) {
					tracing::warn!(?error, "Decodex MCP Streamable HTTP request failed.");
				}
			},
			Err(error) if error.kind() == ErrorKind::Interrupted => continue,
			Err(error) => return Err(error.into()),
		}
	}

	Ok(())
}

fn handle_mcp_http_stream(stream: &mut TcpStream, handler: &mut McpHttpHandler) -> Result<()> {
	stream.set_read_timeout(Some(MCP_HTTP_READ_TIMEOUT))?;

	let request = read_mcp_http_request(stream)?;
	let response = handler.handle_request_bytes(&request)?;

	stream.write_all(&response)?;
	stream.flush()?;

	Ok(())
}

fn read_mcp_http_request(stream: &mut TcpStream) -> Result<Vec<u8>> {
	let mut buffer = Vec::new();
	let mut scratch = [0_u8; 1_024];
	let mut expected_len = None;

	loop {
		let read = stream.read(&mut scratch)?;

		if read == 0 {
			break;
		}

		buffer.extend_from_slice(&scratch[..read]);

		if buffer.len() > MCP_HTTP_MAX_REQUEST_BYTES {
			eyre::bail!("MCP HTTP request exceeded {MCP_HTTP_MAX_REQUEST_BYTES} bytes.");
		}
		if expected_len.is_none()
			&& let Some(header_end) = http_header_end(&buffer)
		{
			let content_length = http_content_length(&buffer[..header_end])?;

			expected_len = Some(header_end + 4 + content_length);
		}
		if expected_len.is_some_and(|length| buffer.len() >= length) {
			break;
		}
	}

	Ok(buffer)
}
