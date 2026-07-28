use std::{
	io::{Read as _, Write as _},
	net::TcpListener,
	thread,
};

use crate::GitHubApi;

#[test]
fn retries_truncated_success_body_for_idempotent_get() {
	let valid_body = r#"{"ok":true}"#;
	let (url, server) = spawn_server(vec![
		format!(
			"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 32\r\nConnection: close\r\n\r\n{{}}"
		),
		response("200 OK", &[], valid_body),
	]);
	let api = GitHubApi::new_for_test(None, &url).expect("GitHub API client should build");
	let response = api.get(&url).expect("truncated body should be retried");

	assert_eq!(response.payload["ok"], true);
	server.join().expect("test server should finish");
}

#[test]
fn retries_invalid_json_for_idempotent_get() {
	let (url, server) = spawn_server(vec![
		response("200 OK", &[], r#"{"incomplete":"#),
		response("200 OK", &[], r#"{"ok":true}"#),
	]);
	let api = GitHubApi::new_for_test(None, &url).expect("GitHub API client should build");
	let response = api.get(&url).expect("invalid JSON should be retried");

	assert_eq!(response.payload["ok"], true);
	server.join().expect("test server should finish");
}

#[test]
fn reports_structured_rate_limit_without_retrying() {
	let body = r#"{"message":"API rate limit exceeded for test address."}"#;
	let (url, server) = spawn_server(vec![response(
		"403 Forbidden",
		&[
			("X-RateLimit-Remaining", "0"),
			("X-RateLimit-Reset", "1785132000"),
			("Retry-After", "120"),
		],
		body,
	)]);
	let api = GitHubApi::new_for_test(None, &url).expect("GitHub API client should build");
	let error = api.get(&url).expect_err("rate limit must fail without retry");
	let message = error.to_string();

	assert!(message.contains("GitHub API rate limit exceeded"));
	assert!(message.contains("reason_code=github_rate_limited"));
	assert!(message.contains("status=403"));
	assert!(message.contains("remaining=0"));
	assert!(message.contains("reset_epoch=1785132000"));
	assert!(message.contains("retry_after=120"));
	server.join().expect("single-response server proves there was no retry");
}

#[test]
fn production_client_requires_the_exact_https_github_api_origin() {
	let api = GitHubApi::new(Some("test-secret".into())).expect("GitHub API client should build");
	let error = api
		.get("http://api.github.com/repos/openai/codex")
		.expect_err("HTTP must fail before any credential-bearing request");

	assert!(error.to_string().contains("pinned origin https://api.github.com"));
}

#[test]
fn pagination_rejects_cross_origin_links_before_forwarding_credentials() {
	let adversary = TcpListener::bind("127.0.0.1:0").expect("adversary listener should bind");
	let adversary_url =
		format!("http://{}/capture", adversary.local_addr().expect("address should exist"));
	let (url, server) = spawn_server(vec![response(
		"200 OK",
		&[("Link", &format!("<{adversary_url}>; rel=\"next\""))],
		"[]",
	)]);
	let api = GitHubApi::new_for_test(Some("test-secret".into()), &url)
		.expect("GitHub API client should build");
	let error =
		api.get_paginated_for_test(&url, 10, 100).expect_err("cross-origin pagination must fail");

	assert!(error.to_string().contains("pinned origin"));
	server.join().expect("trusted server should finish");
	adversary.set_nonblocking(true).expect("adversary listener should become nonblocking");
	assert!(
		adversary.accept().is_err_and(|error| error.kind() == std::io::ErrorKind::WouldBlock),
		"cross-origin target must receive no request"
	);
}

#[test]
fn pagination_detects_cycles_without_repeating_a_request() {
	let (url, server) = spawn_server_with(1, |url, _| {
		response("200 OK", &[("Link", &format!("<{url}>; rel=\"next\""))], "[]")
	});
	let api = GitHubApi::new_for_test(None, &url).expect("GitHub API client should build");
	let error = api.get_paginated_for_test(&url, 10, 100).expect_err("cyclic pagination must fail");

	assert!(error.to_string().contains("cycle detected"));
	server.join().expect("one request proves the cycle was not followed");
}

#[test]
fn pagination_enforces_page_and_item_limits() {
	let (page_url, page_server) = spawn_server_with(2, |url, index| {
		let next = if index == 0 {
			url.replace("/test", "/page-2")
		} else {
			url.replace("/test", "/page-3")
		};

		response("200 OK", &[("Link", &format!("<{next}>; rel=\"next\""))], "[]")
	});
	let page_api =
		GitHubApi::new_for_test(None, &page_url).expect("GitHub API client should build");
	let page_error = page_api
		.get_paginated_for_test(&page_url, 2, 100)
		.expect_err("third page must exceed the bound");

	assert!(page_error.to_string().contains("2-page limit"));
	page_server.join().expect("bounded page server should finish");

	let (item_url, item_server) = spawn_server(vec![response("200 OK", &[], "[1,2,3]")]);
	let item_api =
		GitHubApi::new_for_test(None, &item_url).expect("GitHub API client should build");
	let item_error = item_api
		.get_paginated_for_test(&item_url, 2, 2)
		.expect_err("oversized item page must exceed the bound");

	assert!(item_error.to_string().contains("2-item limit"));
	item_server.join().expect("bounded item server should finish");
}

fn spawn_server(responses: Vec<String>) -> (String, thread::JoinHandle<()>) {
	let listener = TcpListener::bind("127.0.0.1:0").expect("test listener should bind");
	let address = listener.local_addr().expect("test listener should have an address");
	let server = thread::spawn(move || {
		for response in responses {
			let (mut stream, _) = listener.accept().expect("test request should connect");
			let mut request = [0_u8; 4096];
			let _ = stream.read(&mut request);

			stream.write_all(response.as_bytes()).expect("test response should write");
			stream.flush().expect("test response should flush");
		}
	});

	(format!("http://{address}/test"), server)
}

fn spawn_server_with(
	response_count: usize,
	builder: impl Fn(&str, usize) -> String,
) -> (String, thread::JoinHandle<()>) {
	let listener = TcpListener::bind("127.0.0.1:0").expect("test listener should bind");
	let address = listener.local_addr().expect("test listener should have an address");
	let url = format!("http://{address}/test");
	let responses = (0..response_count).map(|index| builder(&url, index)).collect::<Vec<_>>();
	let server = thread::spawn(move || {
		for response in responses {
			let (mut stream, _) = listener.accept().expect("test request should connect");
			let mut request = [0_u8; 4096];
			let _ = stream.read(&mut request);

			stream.write_all(response.as_bytes()).expect("test response should write");
			stream.flush().expect("test response should flush");
		}
	});

	(url, server)
}

fn response(status: &str, headers: &[(&str, &str)], body: &str) -> String {
	let extra_headers =
		headers.iter().map(|(name, value)| format!("{name}: {value}\r\n")).collect::<String>();

	format!(
		"HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n{extra_headers}\r\n{body}",
		body.len()
	)
}
