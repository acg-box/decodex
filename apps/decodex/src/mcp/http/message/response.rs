use serde_json::{self, Value};

use crate::{
	mcp::{
		MCP_SESSION_HEADER,
		http::{MCP_CORS_ALLOW_HEADERS, MCP_CORS_ALLOW_METHODS},
	},
	prelude::Result,
};

pub(in crate::mcp) struct McpHttpResponse {
	pub(in crate::mcp::http) status: &'static str,
	pub(in crate::mcp::http) content_type: Option<&'static str>,
	pub(in crate::mcp::http) headers: Vec<(&'static str, String)>,
	pub(in crate::mcp::http) body: Vec<u8>,
}
impl McpHttpResponse {
	pub(in crate::mcp::http) fn empty(status: &'static str) -> Self {
		Self { status, content_type: None, headers: Vec::new(), body: Vec::new() }
	}

	pub(in crate::mcp::http) fn empty_with_session(
		status: &'static str,
		session_id: Option<String>,
	) -> Self {
		let mut response = Self::empty(status);

		response.add_session_header(session_id);

		response
	}

	pub(in crate::mcp::http) fn json(value: Value, session_id: Option<String>) -> Result<Self> {
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

	pub(in crate::mcp::http) fn json_error(status: &'static str, value: Value) -> Self {
		let body = serde_json::to_vec(&value).unwrap_or_else(|_| {
			br#"{"jsonrpc":"2.0","id":null,"error":{"code":-32603,"message":"Internal error"}}"#
				.to_vec()
		});

		Self {
			status,
			content_type: Some("application/json"),
			headers: vec![("Cache-Control", String::from("no-store"))],
			body,
		}
	}

	pub(in crate::mcp::http) fn sse(
		responses: Vec<Value>,
		session_id: Option<String>,
	) -> Result<Self> {
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

	pub(in crate::mcp::http) fn add_cors_headers(&mut self, origin: Option<String>) {
		let Some(origin) = origin else {
			return;
		};

		self.headers.push(("Access-Control-Allow-Origin", origin));
		self.headers.push(("Vary", String::from("Origin")));
		self.headers.push(("Access-Control-Allow-Methods", String::from(MCP_CORS_ALLOW_METHODS)));
		self.headers.push(("Access-Control-Allow-Headers", String::from(MCP_CORS_ALLOW_HEADERS)));
		self.headers.push(("Access-Control-Expose-Headers", String::from(MCP_SESSION_HEADER)));
	}

	pub(in crate::mcp::http) fn into_bytes(self) -> Result<Vec<u8>> {
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

pub(in crate::mcp) fn mcp_http_response_for_server_responses(
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
