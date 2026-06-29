use std::{
	collections::BTreeSet,
	env,
	io::{ErrorKind, Read as _, Write as _},
	net::{IpAddr, TcpListener, TcpStream},
	str,
	time::Duration,
};

use reqwest::Url;
use serde_json::Value;

use crate::prelude::{Result, eyre};

use super::{
	MCP_HTTP_ENDPOINT_PATH, MCP_PROTOCOL_VERSION, MCP_SESSION_HEADER, McpCapabilityProfile,
	McpContext, McpServer, McpTransport, json_rpc_error,
};

const MCP_HTTP_READ_TIMEOUT: Duration = Duration::from_secs(2);
const MCP_HTTP_MAX_REQUEST_BYTES: usize = 1_024 * 1_024;
const MCP_CORS_ALLOW_METHODS: &str = "POST, DELETE, OPTIONS";
const MCP_CORS_ALLOW_HEADERS: &str = "Content-Type, Accept, Mcp-Session-Id, Authorization";
const MCP_AUTHORIZATION_HEADER: &str = "Authorization";
const MCP_WWW_AUTHENTICATE_HEADER: &str = "Bearer realm=\"decodex-mcp\"";

#[derive(Clone, Default)]
pub(super) struct McpHttpAuthorization {
	token: Option<String>,
}

impl McpHttpAuthorization {
	pub(super) fn disabled() -> Self {
		Self { token: None }
	}

	pub(super) fn from_env_var_name(env_var: Option<&str>) -> Result<Self> {
		let Some(env_var) = env_var else {
			return Ok(Self::disabled());
		};

		validate_mcp_bearer_token_env_var_name(env_var)?;

		let token = env::var(env_var).map_err(|_| {
			eyre::eyre!(
				"Streamable HTTP bearer token env var `{env_var}` is not set; set it or remove --bearer-token-env."
			)
		})?;

		validate_mcp_bearer_token(&token, env_var)?;

		Ok(Self { token: Some(token) })
	}

	fn is_required(&self) -> bool {
		self.token.is_some()
	}

	fn request_is_authorized(&self, request: &McpHttpRequest) -> bool {
		let Some(expected) = self.token.as_deref() else {
			return true;
		};
		let Some(header) = request.header(MCP_AUTHORIZATION_HEADER) else {
			return false;
		};
		let Some((scheme, supplied)) = header.trim().split_once(' ') else {
			return false;
		};

		scheme.eq_ignore_ascii_case("Bearer") && supplied == expected
	}

	fn unauthorized_response() -> McpHttpResponse {
		let mut response = McpHttpResponse::json_error(
			"401 Unauthorized",
			json_rpc_error(Value::Null, -32_000, "Unauthorized"),
		);

		response.headers.push(("WWW-Authenticate", String::from(MCP_WWW_AUTHENTICATE_HEADER)));

		response
	}

	#[cfg(test)]
	pub(super) fn from_token_for_test(token: &str) -> Self {
		Self { token: Some(token.to_owned()) }
	}
}

pub(super) struct McpHttpHandler {
	pub(super) server: McpServer,
	pub(super) sessions: McpHttpSessions,
	pub(super) allowed_origins: Vec<String>,
	pub(super) listen_address: Option<String>,
	pub(super) authorization: McpHttpAuthorization,
}

impl McpHttpHandler {
	pub(super) fn handle_request_bytes(&mut self, request: &[u8]) -> Result<Vec<u8>> {
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
pub(super) struct McpHttpSessions {
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

struct McpHttpRequest {
	method: String,
	path: String,
	headers: Vec<(String, String)>,
	body: Vec<u8>,
}

impl McpHttpRequest {
	fn parse(request: &[u8]) -> std::result::Result<Self, McpHttpResponse> {
		let Some(header_end) = http_header_end(request) else {
			return Err(McpHttpResponse::empty("400 Bad Request"));
		};
		let header_text = str::from_utf8(&request[..header_end])
			.map_err(|_| McpHttpResponse::empty("400 Bad Request"))?;
		let mut lines = header_text.split("\r\n");
		let request_line = lines.next().ok_or_else(|| McpHttpResponse::empty("400 Bad Request"))?;
		let mut request_parts = request_line.split_whitespace();
		let method = request_parts
			.next()
			.ok_or_else(|| McpHttpResponse::empty("400 Bad Request"))?
			.to_owned();
		let path = request_parts
			.next()
			.ok_or_else(|| McpHttpResponse::empty("400 Bad Request"))?
			.to_owned();
		let version =
			request_parts.next().ok_or_else(|| McpHttpResponse::empty("400 Bad Request"))?;

		if !version.starts_with("HTTP/1.") {
			return Err(McpHttpResponse::empty("505 HTTP Version Not Supported"));
		}

		let mut headers = Vec::new();

		for line in lines {
			if line.is_empty() {
				continue;
			}

			let Some((name, value)) = line.split_once(':') else {
				return Err(McpHttpResponse::empty("400 Bad Request"));
			};

			headers.push((name.trim().to_owned(), value.trim().to_owned()));
		}

		let content_length = http_content_length(&request[..header_end])
			.map_err(|_| McpHttpResponse::empty("400 Bad Request"))?;
		let body_start = header_end + 4;
		let body_end = body_start.saturating_add(content_length);

		if request.len() < body_end {
			return Err(McpHttpResponse::empty("400 Bad Request"));
		}

		Ok(Self { method, path, headers, body: request[body_start..body_end].to_vec() })
	}

	fn header(&self, name: &str) -> Option<&str> {
		self.headers
			.iter()
			.find(|(header, _)| header.eq_ignore_ascii_case(name))
			.map(|(_, value)| value.as_str())
	}

	fn accepts_sse(&self) -> bool {
		header_contains(self.header("Accept"), "text/event-stream")
	}

	fn content_type_is_json(&self) -> bool {
		header_contains(self.header("Content-Type"), "application/json")
	}
}

struct McpHttpResponse {
	status: &'static str,
	content_type: Option<&'static str>,
	headers: Vec<(&'static str, String)>,
	body: Vec<u8>,
}

impl McpHttpResponse {
	fn empty(status: &'static str) -> Self {
		Self { status, content_type: None, headers: Vec::new(), body: Vec::new() }
	}

	fn empty_with_session(status: &'static str, session_id: Option<String>) -> Self {
		let mut response = Self::empty(status);

		response.add_session_header(session_id);

		response
	}

	fn json(value: Value, session_id: Option<String>) -> Result<Self> {
		let body = serde_json::to_vec(&value)?;
		let mut response = Self {
			status: "200 OK",
			content_type: Some("application/json"),
			headers: vec![("Cache-Control", String::from("no-store"))],
			body,
		};

		response.add_session_header(session_id);

		Ok(response)
	}

	fn json_error(status: &'static str, value: Value) -> Self {
		let body =
			serde_json::to_vec(&value)
				.unwrap_or_else(|_| {
					br#"{"jsonrpc":"2.0","id":null,"error":{"code":-32603,"message":"Internal error"}}"#.to_vec()
				});

		Self {
			status,
			content_type: Some("application/json"),
			headers: vec![("Cache-Control", String::from("no-store"))],
			body,
		}
	}

	fn sse(responses: Vec<Value>, session_id: Option<String>) -> Result<Self> {
		let mut body = Vec::new();

		for response in responses {
			let line = serde_json::to_string(&response)?;

			body.extend_from_slice(b"event: message\n");
			body.extend_from_slice(b"data: ");
			body.extend_from_slice(line.as_bytes());
			body.extend_from_slice(b"\n\n");
		}

		let mut response = Self {
			status: "200 OK",
			content_type: Some("text/event-stream"),
			headers: vec![
				("Cache-Control", String::from("no-store")),
				("X-Accel-Buffering", String::from("no")),
			],
			body,
		};

		response.add_session_header(session_id);

		Ok(response)
	}

	fn add_session_header(&mut self, session_id: Option<String>) {
		if let Some(session_id) = session_id {
			self.headers.push((MCP_SESSION_HEADER, session_id));
		}
	}

	fn add_cors_headers(&mut self, origin: Option<String>) {
		let Some(origin) = origin else {
			return;
		};

		self.headers.push(("Access-Control-Allow-Origin", origin));
		self.headers.push(("Vary", String::from("Origin")));
		self.headers.push(("Access-Control-Allow-Methods", String::from(MCP_CORS_ALLOW_METHODS)));
		self.headers.push(("Access-Control-Allow-Headers", String::from(MCP_CORS_ALLOW_HEADERS)));
		self.headers.push(("Access-Control-Expose-Headers", String::from(MCP_SESSION_HEADER)));
	}

	fn into_bytes(self) -> Result<Vec<u8>> {
		let mut response = Vec::new();

		response.extend_from_slice(format!("HTTP/1.1 {}\r\n", self.status).as_bytes());
		response.extend_from_slice(b"Connection: close\r\n");
		response.extend_from_slice(format!("Content-Length: {}\r\n", self.body.len()).as_bytes());

		if let Some(content_type) = self.content_type {
			response.extend_from_slice(format!("Content-Type: {content_type}\r\n").as_bytes());
		}

		for (name, value) in self.headers {
			response.extend_from_slice(format!("{name}: {value}\r\n").as_bytes());
		}

		response.extend_from_slice(b"\r\n");
		response.extend_from_slice(&self.body);

		Ok(response)
	}
}

pub(super) fn serve_streamable_http_with_profile(
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

pub(super) fn validate_mcp_http_listen_address(
	address: &str,
	allowed_origins: &[String],
	authorization: &McpHttpAuthorization,
) -> Result<()> {
	if listen_address_host_is_loopback(address) {
		return Ok(());
	}
	if allowed_origins.is_empty() {
		eyre::bail!(
			"Refusing to bind Decodex MCP Streamable HTTP to `{address}` without --allow-origin; use the loopback default or set explicit trusted origins."
		)
	}
	if !authorization.is_required() {
		eyre::bail!(
			"Refusing to bind Decodex MCP Streamable HTTP to `{address}` without --bearer-token-env; direct non-loopback listeners require bearer authorization."
		)
	}

	Ok(())
}

pub(super) fn validate_mcp_http_capability_profile(
	capability_profile: McpCapabilityProfile,
	authorization: &McpHttpAuthorization,
) -> Result<()> {
	if capability_profile == McpCapabilityProfile::Observe || authorization.is_required() {
		return Ok(());
	}

	eyre::bail!(
		"Refusing to expose Decodex MCP Streamable HTTP profile `{}` without --bearer-token-env; elevated HTTP profiles require bearer authorization.",
		capability_profile.as_str()
	)
}

fn validate_mcp_bearer_token_env_var_name(env_var: &str) -> Result<()> {
	if env_var.is_empty() || env_var.trim() != env_var {
		eyre::bail!("--bearer-token-env must name a non-empty environment variable.");
	}

	let mut chars = env_var.chars();
	let Some(first) = chars.next() else {
		eyre::bail!("--bearer-token-env must name a non-empty environment variable.");
	};

	if !(first.is_ascii_alphabetic() || first == '_')
		|| !chars.all(|char| char.is_ascii_alphanumeric() || char == '_')
	{
		eyre::bail!(
			"--bearer-token-env must start with an ASCII letter or underscore and contain only ASCII letters, digits, or underscores."
		);
	}

	Ok(())
}

fn validate_mcp_bearer_token(token: &str, env_var: &str) -> Result<()> {
	if token.is_empty() || token.trim().is_empty() {
		eyre::bail!("Streamable HTTP bearer token env var `{env_var}` is empty.");
	}
	if token.chars().any(char::is_whitespace) {
		eyre::bail!(
			"Streamable HTTP bearer token env var `{env_var}` must not contain whitespace."
		);
	}

	Ok(())
}

fn mcp_http_response_for_server_responses(
	responses: Vec<Value>,
	wants_sse: bool,
	session_id: Option<String>,
) -> Result<McpHttpResponse> {
	if responses.is_empty() {
		return Ok(McpHttpResponse::empty_with_session("202 Accepted", session_id));
	}
	if wants_sse {
		return McpHttpResponse::sse(responses, session_id);
	}

	McpHttpResponse::json(
		responses.into_iter().next().unwrap_or_else(|| serde_json::json!({})),
		session_id,
	)
}

fn listen_address_host_is_loopback(address: &str) -> bool {
	let host = listen_address_host(address);

	host.as_deref().is_some_and(host_is_loopback)
}

fn mcp_http_origin_is_allowed(
	origin: &str,
	listen_address: Option<&str>,
	allowed_origins: &[String],
) -> bool {
	if allowed_origins.iter().any(|allowed| allowed == origin) {
		return true;
	}

	let Ok(parsed) = Url::parse(origin) else {
		return false;
	};
	let Some(host) = parsed.host_str() else {
		return false;
	};

	if !matches!(parsed.scheme(), "http" | "https") || !host_is_loopback(host) {
		return false;
	}

	let Some(listen_port) = listen_address.and_then(listen_address_port) else {
		return true;
	};

	parsed.port_or_known_default() == Some(listen_port)
}

fn host_is_loopback(host: &str) -> bool {
	host.eq_ignore_ascii_case("localhost")
		|| host
			.trim_matches(['[', ']'])
			.parse::<IpAddr>()
			.is_ok_and(|address| address.is_loopback())
}

fn listen_address_host(address: &str) -> Option<String> {
	let (host, _) = address.rsplit_once(':')?;

	Some(host.trim_matches(['[', ']']).to_owned())
}

fn listen_address_port(address: &str) -> Option<u16> {
	let (_, port) = address.rsplit_once(':')?;

	port.parse().ok()
}

pub(super) fn http_header_end(bytes: &[u8]) -> Option<usize> {
	bytes.windows(4).position(|window| window == b"\r\n\r\n")
}

fn http_content_length(header_bytes: &[u8]) -> Result<usize> {
	let header_text = str::from_utf8(header_bytes)?;

	for line in header_text.split("\r\n").skip(1) {
		let Some((name, value)) = line.split_once(':') else {
			continue;
		};

		if name.trim().eq_ignore_ascii_case("Content-Length") {
			return Ok(value.trim().parse()?);
		}
	}

	Ok(0)
}

fn header_contains(header: Option<&str>, value: &str) -> bool {
	header
		.map(|header| {
			header.split(',').any(|item| {
				item.trim().split(';').next().is_some_and(|item| item.eq_ignore_ascii_case(value))
			})
		})
		.unwrap_or(false)
}

fn json_rpc_method_name(body: &str) -> Option<String> {
	serde_json::from_str::<Value>(body)
		.ok()
		.and_then(|value| value.get("method").and_then(Value::as_str).map(str::to_owned))
}

fn initialize_response_succeeded(responses: &[Value]) -> bool {
	responses.iter().any(|response| {
		response.get("error").is_none()
			&& response
				.get("result")
				.and_then(|result| result.get("protocolVersion"))
				.and_then(Value::as_str)
				== Some(MCP_PROTOCOL_VERSION)
	})
}
