use std::path::Path;

use serde_json::Value;

use crate::mcp::{
	DEFAULT_MCP_HTTP_LISTEN_ADDRESS, McpCapabilityProfile, McpContext, McpHttpAuthorization,
	McpHttpHandler, McpHttpSessions, McpServer, McpTransport,
	tests::support::parsed_http::ParsedHttpResponse,
};

pub(in crate::mcp::tests) fn http_handler(
	repo_root: &Path,
	capability_profile: McpCapabilityProfile,
) -> McpHttpHandler {
	http_handler_with_allowed_origins(repo_root, capability_profile, Vec::new())
}

pub(in crate::mcp::tests) fn http_handler_with_authorization(
	repo_root: &Path,
	capability_profile: McpCapabilityProfile,
	authorization: McpHttpAuthorization,
) -> McpHttpHandler {
	let context = McpContext {
		repo_root: repo_root.to_path_buf(),
		config_path: None,
		project_id: None,
		state_store: None,
	};

	http_handler_with_context_and_authorization(
		context,
		capability_profile,
		Vec::new(),
		authorization,
	)
}

pub(in crate::mcp::tests) fn http_handler_with_allowed_origins(
	repo_root: &Path,
	capability_profile: McpCapabilityProfile,
	allowed_origins: Vec<String>,
) -> McpHttpHandler {
	let context = McpContext {
		repo_root: repo_root.to_path_buf(),
		config_path: None,
		project_id: None,
		state_store: None,
	};

	http_handler_with_context(context, capability_profile, allowed_origins)
}

pub(in crate::mcp::tests) fn http_handler_with_context(
	context: McpContext,
	capability_profile: McpCapabilityProfile,
	allowed_origins: Vec<String>,
) -> McpHttpHandler {
	http_handler_with_context_and_authorization(
		context,
		capability_profile,
		allowed_origins,
		McpHttpAuthorization::disabled(),
	)
}

pub(in crate::mcp::tests) fn http_handler_with_context_and_authorization(
	context: McpContext,
	capability_profile: McpCapabilityProfile,
	allowed_origins: Vec<String>,
	authorization: McpHttpAuthorization,
) -> McpHttpHandler {
	McpHttpHandler {
		server: McpServer { context, capability_profile, transport: McpTransport::StreamableHttp },
		sessions: McpHttpSessions::default(),
		allowed_origins,
		listen_address: Some(String::from(DEFAULT_MCP_HTTP_LISTEN_ADDRESS)),
		authorization,
	}
}

pub(in crate::mcp::tests) fn run_http(
	handler: &mut McpHttpHandler,
	request: Vec<u8>,
) -> ParsedHttpResponse {
	let response =
		handler.handle_request_bytes(&request).expect("HTTP handler should return response");

	ParsedHttpResponse::parse(&response)
}

pub(in crate::mcp::tests) fn http_json_rpc(
	handler: &mut McpHttpHandler,
	session_id: &str,
	body: &str,
) -> Value {
	let response = run_http(
		handler,
		http_post(
			"/mcp",
			[("Origin", "http://127.0.0.1:8193"), ("Mcp-Session-Id", session_id)],
			body,
		),
	);

	assert_eq!(response.status, "HTTP/1.1 200 OK");
	assert_eq!(response.header("content-type"), Some("application/json"));
	assert_eq!(response.header("access-control-allow-origin"), Some("http://127.0.0.1:8193"));

	response.json_body()
}

pub(in crate::mcp::tests) fn http_resource_read_json(
	handler: &mut McpHttpHandler,
	session_id: &str,
	id: u64,
	uri: &str,
) -> Value {
	let request = serde_json::json!({
		"jsonrpc": "2.0",
		"id": id,
		"method": "resources/read",
		"params": {
			"uri": uri
		}
	})
	.to_string();
	let response = http_json_rpc(handler, session_id, &request);
	let contents = response["result"]["contents"].as_array().expect("resource contents array");
	let text = contents[0]["text"].as_str().expect("resource text should exist");

	serde_json::from_str(text).expect("resource text should be JSON")
}

pub(in crate::mcp::tests) fn http_post<'a>(
	path: &str,
	headers: impl IntoIterator<Item = (&'a str, &'a str)>,
	body: &str,
) -> Vec<u8> {
	let mut request = format!(
		"POST {path} HTTP/1.1\r\nHost: 127.0.0.1:8193\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",
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

	request.into_bytes()
}

pub(in crate::mcp::tests) fn http_delete<'a>(
	path: &str,
	headers: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Vec<u8> {
	let mut request =
		format!("DELETE {path} HTTP/1.1\r\nHost: 127.0.0.1:8193\r\nContent-Length: 0\r\n");

	for (name, value) in headers {
		request.push_str(name);
		request.push_str(": ");
		request.push_str(value);
		request.push_str("\r\n");
	}

	request.push_str("\r\n");

	request.into_bytes()
}

pub(in crate::mcp::tests) fn http_options<'a>(
	path: &str,
	headers: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Vec<u8> {
	let mut request =
		format!("OPTIONS {path} HTTP/1.1\r\nHost: 127.0.0.1:8193\r\nContent-Length: 0\r\n");

	for (name, value) in headers {
		request.push_str(name);
		request.push_str(": ");
		request.push_str(value);
		request.push_str("\r\n");
	}

	request.push_str("\r\n");

	request.into_bytes()
}
