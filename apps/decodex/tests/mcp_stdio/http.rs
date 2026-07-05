use std::{
	io::{Read as _, Result, Write},
	net::{Shutdown, TcpListener, TcpStream},
	str, thread,
	time::{Duration, Instant},
};

use serde_json::Value;

use crate::mcp_stdio::support::ChildGuard;

#[derive(Debug)]
pub(crate) struct ParsedHttpResponse {
	status: String,
	headers: Vec<(String, String)>,
	body: Vec<u8>,
}
impl ParsedHttpResponse {
	fn parse(bytes: Vec<u8>) -> Self {
		let header_end = bytes
			.windows(4)
			.position(|window| window == b"\r\n\r\n")
			.expect("HTTP response should have headers");
		let header_text = str::from_utf8(&bytes[..header_end]).expect("headers should be utf-8");
		let mut lines = header_text.split("\r\n");
		let status = lines.next().expect("status line should exist").to_owned();
		let headers = lines
			.filter_map(|line| {
				let (name, value) = line.split_once(':')?;

				Some((name.trim().to_ascii_lowercase(), value.trim().to_owned()))
			})
			.collect();

		Self { status, headers, body: bytes[header_end + 4..].to_vec() }
	}

	pub(crate) fn status(&self) -> &str {
		&self.status
	}

	pub(crate) fn header(&self, name: &str) -> Option<&str> {
		let lower_name = name.to_ascii_lowercase();

		self.headers
			.iter()
			.find(|(header, _)| header == &lower_name)
			.map(|(_, value)| value.as_str())
	}

	pub(crate) fn body_text(&self) -> String {
		String::from_utf8(self.body.clone()).expect("body should be utf-8")
	}

	pub(crate) fn json_body(&self) -> Value {
		serde_json::from_slice(&self.body).expect("body should be JSON")
	}
}

pub(crate) fn free_loopback_address() -> String {
	let listener = TcpListener::bind("127.0.0.1:0").expect("free loopback port should bind");
	let address = listener.local_addr().expect("loopback address should exist");

	address.to_string()
}

pub(crate) fn wait_for_streamable_http(addr: &str, child: &mut ChildGuard) {
	let deadline = Instant::now() + Duration::from_secs(10);

	loop {
		if let Some(status) = child.try_wait() {
			panic!("HTTP MCP process exited before accepting requests: {status:?}");
		}

		match http_options(addr) {
			Ok(response) if response.status() == "HTTP/1.1 204 No Content" => return,
			Ok(response) => panic!("HTTP MCP readiness probe returned {}", response.status()),
			Err(_) if Instant::now() < deadline => thread::sleep(Duration::from_millis(50)),
			Err(error) => panic!("HTTP MCP process did not listen at {addr}: {error}"),
		}
	}
}

pub(crate) fn http_post(addr: &str, headers: &[(&str, &str)], body: &str) -> ParsedHttpResponse {
	let mut request = format!(
		"POST /mcp HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",
		body.len()
	);

	for (name, value) in headers {
		request.push_str(name);
		request.push_str(": ");
		request.push_str(value);
		request.push_str("\r\n");
	}

	request.push_str("\r\n");
	request.push_str(body);

	http_raw(addr, &request)
}

fn http_options(addr: &str) -> Result<ParsedHttpResponse> {
	let request = format!(
		"OPTIONS /mcp HTTP/1.1\r\nHost: {addr}\r\nAccess-Control-Request-Method: POST\r\nContent-Length: 0\r\n\r\n"
	);
	let mut stream = TcpStream::connect(addr)?;
	let mut response = Vec::new();

	stream.write_all(request.as_bytes())?;
	stream.shutdown(Shutdown::Write)?;
	stream.read_to_end(&mut response)?;

	Ok(ParsedHttpResponse::parse(response))
}

fn http_raw(addr: &str, request: &str) -> ParsedHttpResponse {
	let mut stream = TcpStream::connect(addr).expect("HTTP server should accept TCP");
	let mut response = Vec::new();

	stream.write_all(request.as_bytes()).expect("HTTP request should write");
	stream.shutdown(Shutdown::Write).expect("HTTP request should finish");
	stream.read_to_end(&mut response).expect("HTTP response should read");

	ParsedHttpResponse::parse(response)
}
