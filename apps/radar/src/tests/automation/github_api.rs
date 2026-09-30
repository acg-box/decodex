use std::{
	collections::VecDeque,
	io::{Read as _, Write as _},
	os::unix::net::UnixListener,
	path::PathBuf,
	sync::{
		Arc, Mutex,
		atomic::{AtomicBool, Ordering},
	},
	thread,
	time::Duration,
};

use crate::GitHubApi;

#[test]
fn retries_truncated_success_body_for_idempotent_get() {
	let valid_body = r#"{"ok":true}"#;
	let server = spawn_server(vec![
		"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 32\r\nConnection: close\r\n\r\n{}".to_owned(),
		response("200 OK", &[], valid_body),
	]);
	let api = server.api(None);
	let response = api.get(server.url()).expect("truncated body should be retried");

	assert_eq!(response.payload["ok"], true);
	server.finish();
}

#[test]
fn retries_invalid_json_for_idempotent_get() {
	let server = spawn_server(vec![
		response("200 OK", &[], r#"{"incomplete":"#),
		response("200 OK", &[], r#"{"ok":true}"#),
	]);
	let api = server.api(None);
	let response = api.get(server.url()).expect("invalid JSON should be retried");

	assert_eq!(response.payload["ok"], true);
	server.finish();
}

#[test]
fn reports_structured_rate_limit_without_retrying() {
	let body = r#"{"message":"API rate limit exceeded for test address."}"#;
	let server = spawn_server(vec![response(
		"403 Forbidden",
		&[
			("X-RateLimit-Remaining", "0"),
			("X-RateLimit-Reset", "1785132000"),
			("Retry-After", "120"),
		],
		body,
	)]);
	let api = server.api(None);
	let error = api.get(server.url()).expect_err("rate limit must fail without retry");
	let message = error.to_string();

	assert!(message.contains("GitHub API rate limit exceeded"));
	assert!(message.contains("reason_code=github_rate_limited"));
	assert!(message.contains("status=403"));
	assert!(message.contains("remaining=0"));
	assert!(message.contains("reset_epoch=1785132000"));
	assert!(message.contains("retry_after=120"));
	server.finish();
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
	let adversary_url = "http://adversary.test/capture";
	let server = spawn_server(vec![response(
		"200 OK",
		&[("Link", &format!("<{adversary_url}>; rel=\"next\""))],
		"[]",
	)]);
	let api = server.api(Some("test-secret".into()));
	let error = api
		.get_paginated_for_test(server.url(), None, 10, 100)
		.expect_err("cross-origin pagination must fail");

	assert!(error.to_string().contains("pinned origin"));
	let requests = server.finish_with_requests();

	assert_eq!(requests.len(), 1, "cross-origin pagination must not send a second request");
	let request = requests[0].to_ascii_lowercase();

	assert!(request.contains("host: github.test"));
	assert!(request.contains("authorization: bearer test-secret"));
	assert!(!request.contains("adversary.test"));
}

#[test]
fn pagination_detects_cycles_without_repeating_a_request() {
	let server = spawn_server_with(1, |url, _| {
		response("200 OK", &[("Link", &format!("<{url}>; rel=\"next\""))], "[]")
	});
	let api = server.api(None);
	let error = api
		.get_paginated_for_test(server.url(), None, 10, 100)
		.expect_err("cyclic pagination must fail");

	assert!(error.to_string().contains("cycle detected"));
	server.finish();
}

#[test]
fn pagination_enforces_page_and_item_limits() {
	let page_server = spawn_server_with(2, |url, index| {
		let next = if index == 0 {
			url.replace("/test", "/page-2")
		} else {
			url.replace("/test", "/page-3")
		};

		response("200 OK", &[("Link", &format!("<{next}>; rel=\"next\""))], "[]")
	});
	let page_api = page_server.api(None);
	let page_error = page_api
		.get_paginated_for_test(page_server.url(), None, 2, 100)
		.expect_err("third page must exceed the bound");

	assert!(page_error.to_string().contains("2-page limit"));
	page_server.finish();

	let item_server = spawn_server(vec![response("200 OK", &[], "[1,2,3]")]);
	let item_api = item_server.api(None);
	let item_error = item_api
		.get_paginated_for_test(item_server.url(), None, 2, 2)
		.expect_err("oversized item page must exceed the bound");

	assert!(item_error.to_string().contains("2-item limit"));
	item_server.finish();
}

#[test]
fn object_pagination_collects_all_commits_and_preserves_first_page_metadata() {
	let server = spawn_server_with(3, |url, index| {
		let start = index * 100;
		let end = (start + 100).min(251);
		let commits = (start..end)
			.map(|i| {
				serde_json::json!({
					"sha": format!("{i:040x}"), "commit": {"message": format!("Change (#{})", i + 1)}
				})
			})
			.collect::<Vec<_>>();
		let body = if index == 0 {
			serde_json::json!({"status": "ahead", "total_commits": 251, "commits": commits})
		} else {
			serde_json::json!({"commits": commits})
		}
		.to_string();
		if index < 2 {
			response(
				"200 OK",
				&[("Link", &format!("<{url}?page={}>; rel=\"next\"", index + 2))],
				&body,
			)
		} else {
			response("200 OK", &[], &body)
		}
	});
	let payload = server.api(None).get_paginated_field(server.url(), "commits").unwrap();
	assert_eq!(payload["status"], "ahead");
	assert_eq!(payload["total_commits"], 251);
	assert_eq!(payload["commits"].as_array().unwrap().len(), 251);
	assert_eq!(payload["commits"][250]["commit"]["message"], "Change (#251)");
	assert_eq!(server.finish_with_requests().len(), 3);
}

#[test]
fn object_pagination_keeps_bounds_and_rejects_bad_pages() {
	let cases = [
		(
			vec![response(
				"200 OK",
				&[("Link", "<http://adversary.test/next>; rel=\"next\"")],
				r#"{"commits":[]}"#,
			)],
			10,
			100,
			"pinned origin",
		),
		(
			vec![response(
				"200 OK",
				&[("Link", "<http://github.test/test>; rel=\"next\"")],
				r#"{"commits":[]}"#,
			)],
			10,
			100,
			"cycle detected",
		),
		(
			vec![response(
				"200 OK",
				&[("Link", "<http://github.test/page-2>; rel=\"next\"")],
				r#"{"commits":[]}"#,
			)],
			1,
			100,
			"1-page limit",
		),
		(vec![response("200 OK", &[], r#"{"commits":[1,2,3]}"#)], 10, 2, "2-item limit"),
		(
			vec![
				response(
					"200 OK",
					&[("Link", "<http://github.test/page-2>; rel=\"next\"")],
					r#"{"commits":[1,2]}"#,
				),
				response("200 OK", &[], r#"{"commits":[3]}"#),
			],
			10,
			2,
			"2-item limit",
		),
		(
			vec![
				response(
					"200 OK",
					&[("Link", "<http://github.test/page-2>; rel=\"next\"")],
					r#"{"commits":[]}"#,
				),
				response("200 OK", &[], r#"{"commits":null}"#),
			],
			10,
			100,
			"Expected list field commits",
		),
	];
	for (responses, max_pages, max_items, expected) in cases {
		let expected_requests = responses.len();
		let server = spawn_server(responses);
		let error = server
			.api(Some("test-secret".into()))
			.get_paginated_for_test(server.url(), Some("commits"), max_pages, max_items)
			.expect_err("invalid object pagination must fail");
		assert!(error.to_string().contains(expected), "{error}");
		assert_eq!(server.finish_with_requests().len(), expected_requests);
	}
}

pub(crate) struct TestServer {
	_directory: crate::private_fs::PrivateTestDirectory,
	socket: PathBuf,
	thread: thread::JoinHandle<()>,
	requests: Arc<Mutex<Vec<String>>>,
	stop: Arc<AtomicBool>,
	url: String,
}
impl TestServer {
	pub(crate) fn api(&self, token: Option<String>) -> GitHubApi {
		GitHubApi::new_for_test(token, &self.url, &self.socket)
			.expect("GitHub API client should build")
	}

	fn finish(self) {
		drop(self.finish_with_requests());
	}

	pub(crate) fn finish_with_requests(self) -> Vec<String> {
		self.stop.store(true, Ordering::Release);
		self.thread.join().expect("test server should finish");
		self.requests.lock().expect("request log should not be poisoned").clone()
	}

	pub(crate) fn url(&self) -> &str {
		&self.url
	}
}

fn spawn_server(responses: Vec<String>) -> TestServer {
	spawn_server_responses(responses)
}

fn spawn_server_responses(responses: Vec<String>) -> TestServer {
	let directory = crate::test_support::private_tempdir();
	let socket = directory.path().join("g.sock");
	let listener = UnixListener::bind(&socket).expect("test listener should bind");
	listener.set_nonblocking(true).expect("test listener should become nonblocking");
	let url = "http://github.test/test".to_owned();
	let requests = Arc::new(Mutex::new(Vec::new()));
	let server_requests = Arc::clone(&requests);
	let stop = Arc::new(AtomicBool::new(false));
	let server_stop = Arc::clone(&stop);
	let server = thread::spawn(move || {
		let mut responses = VecDeque::from(responses);

		loop {
			match listener.accept() {
				Ok((mut stream, _)) => {
					stream
						.set_nonblocking(false)
						.expect("accepted test stream should become blocking");
					let mut request = [0_u8; 4096];
					let read = stream.read(&mut request).expect("test request should be readable");

					server_requests
						.lock()
						.expect("request log should not be poisoned")
						.push(String::from_utf8_lossy(&request[..read]).into_owned());

					let response = responses.pop_front().unwrap_or_else(|| {
						response("500 Internal Server Error", &[], r#"{"unexpected":true}"#)
					});

					stream.write_all(response.as_bytes()).expect("test response should write");
					stream.flush().expect("test response should flush");
				},
				Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
					if server_stop.load(Ordering::Acquire) {
						break;
					}
					thread::sleep(Duration::from_millis(1));
				},
				Err(error) => panic!("test request should connect: {error}"),
			}
		}

		assert!(responses.is_empty(), "test server did not receive all expected requests");
	});

	TestServer { _directory: directory, socket, thread: server, requests, stop, url }
}

pub(crate) fn spawn_server_with(
	response_count: usize,
	builder: impl Fn(&str, usize) -> String,
) -> TestServer {
	let url = "http://github.test/test";
	let responses = (0..response_count).map(|index| builder(url, index)).collect::<Vec<_>>();

	spawn_server_responses(responses)
}

pub(crate) fn response(status: &str, headers: &[(&str, &str)], body: &str) -> String {
	let extra_headers =
		headers.iter().map(|(name, value)| format!("{name}: {value}\r\n")).collect::<String>();

	format!(
		"HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n{extra_headers}\r\n{body}",
		body.len()
	)
}
