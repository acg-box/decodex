use std::str;

use serde_json::Value;

use crate::mcp;

pub(in crate::mcp::tests) struct ParsedHttpResponse {
	pub(in crate::mcp::tests) status: String,
	pub(in crate::mcp::tests) headers: Vec<(String, String)>,
	pub(in crate::mcp::tests) body: Vec<u8>,
}
impl ParsedHttpResponse {
	pub(in crate::mcp::tests) fn parse(response: &[u8]) -> Self {
		let header_end = mcp::http_header_end(response).expect("response should include headers");
		let headers = str::from_utf8(&response[..header_end]).expect("headers should be utf-8");
		let mut lines = headers.split("\r\n");
		let status = lines.next().expect("status line should exist").to_owned();
		let headers = lines
			.filter_map(|line| {
				let (name, value) = line.split_once(':')?;

				Some((name.trim().to_ascii_lowercase(), value.trim().to_owned()))
			})
			.collect();

		Self { status, headers, body: response[(header_end + 4)..].to_vec() }
	}

	pub(in crate::mcp::tests) fn header(&self, name: &str) -> Option<&str> {
		self.headers
			.iter()
			.find(|(header, _)| header == &name.to_ascii_lowercase())
			.map(|(_, value)| value.as_str())
	}

	pub(in crate::mcp::tests) fn json_body(&self) -> Value {
		serde_json::from_slice(&self.body).expect("HTTP body should be JSON")
	}

	pub(in crate::mcp::tests) fn body_text(&self) -> &str {
		str::from_utf8(&self.body).expect("HTTP body should be utf-8")
	}
}
