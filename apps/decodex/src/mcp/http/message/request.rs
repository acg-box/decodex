use std::str;

use crate::{mcp::http::message::response::McpHttpResponse, prelude};

pub(in crate::mcp) struct McpHttpRequest {
	pub(in crate::mcp::http) method: String,
	pub(in crate::mcp::http) path: String,
	pub(in crate::mcp::http) headers: Vec<(String, String)>,
	pub(in crate::mcp::http) body: Vec<u8>,
}
impl McpHttpRequest {
	pub(in crate::mcp::http) fn parse(
		request: &[u8],
	) -> std::result::Result<Self, McpHttpResponse> {
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

	pub(in crate::mcp::http) fn header(&self, name: &str) -> Option<&str> {
		self.headers
			.iter()
			.find(|(header, _)| header.eq_ignore_ascii_case(name))
			.map(|(_, value)| value.as_str())
	}

	pub(in crate::mcp::http) fn accepts_sse(&self) -> bool {
		header_contains(self.header("Accept"), "text/event-stream")
	}

	pub(in crate::mcp::http) fn content_type_is_json(&self) -> bool {
		header_contains(self.header("Content-Type"), "application/json")
	}
}

pub(in crate::mcp) fn http_header_end(bytes: &[u8]) -> Option<usize> {
	bytes.windows(4).position(|window| window == b"\r\n\r\n")
}

pub(in crate::mcp) fn http_content_length(header_bytes: &[u8]) -> prelude::Result<usize> {
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
