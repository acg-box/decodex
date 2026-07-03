mod records;
mod refresh;
mod selection;
mod sorting;
mod usage;

use std::{
	io::{Read, Write},
	net::TcpListener,
	thread,
};

use crate::agent::codex_accounts::{CodexAccountActivitySummary, CodexAccountLogin};

fn codex_account_login_for_sort(
	account_id: &str,
	primary_remaining_percent: Option<i64>,
	secondary_remaining_percent: Option<i64>,
	last_selected_at_unix_epoch: Option<i64>,
) -> CodexAccountLogin {
	CodexAccountLogin {
		access_token: String::from("access"),
		account_id: account_id.to_owned(),
		plan_type: Some(String::from("pro")),
		last_selected_at_unix_epoch,
		summary: CodexAccountActivitySummary {
			account_fingerprint: format!("...{account_id}"),
			primary_remaining_percent,
			secondary_remaining_percent,
			..CodexAccountActivitySummary::default()
		},
		account_summaries: Vec::new(),
	}
}

fn start_codex_usage_fixture_server(responses: Vec<&'static str>) -> String {
	let listener = TcpListener::bind("127.0.0.1:0").expect("usage fixture server should bind");
	let address = listener.local_addr().expect("usage fixture address should resolve");

	thread::spawn(move || {
		for body in responses {
			let (mut stream, _peer) =
				listener.accept().expect("usage fixture should accept request");
			let mut buffer = [0_u8; 4_096];
			let _bytes_read = stream.read(&mut buffer).expect("request should read");
			let response = format!(
				"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
				body.len(),
				body
			);

			stream.write_all(response.as_bytes()).expect("usage response should write");
		}
	});

	format!("http://{address}/usage")
}

fn start_codex_refresh_fixture_server(responses: Vec<&'static str>) -> String {
	let listener = TcpListener::bind("127.0.0.1:0").expect("refresh fixture should bind");
	let address = listener.local_addr().expect("refresh fixture address should resolve");

	thread::spawn(move || {
		for body in responses {
			let (mut stream, _peer) =
				listener.accept().expect("refresh fixture should accept request");
			let mut buffer = [0_u8; 4_096];
			let _bytes_read = stream.read(&mut buffer).expect("refresh request should read");
			let response = format!(
				"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
				body.len(),
				body
			);

			stream.write_all(response.as_bytes()).expect("refresh response should write");
		}
	});

	format!("http://{address}/oauth/token")
}

fn start_codex_refresh_status_fixture_server(
	responses: Vec<(u16, &'static str, &'static str)>,
) -> String {
	let listener = TcpListener::bind("127.0.0.1:0").expect("refresh fixture should bind");
	let address = listener.local_addr().expect("refresh fixture address should resolve");

	thread::spawn(move || {
		for (status, reason, body) in responses {
			let (mut stream, _peer) =
				listener.accept().expect("refresh fixture should accept request");
			let mut buffer = [0_u8; 4_096];
			let _bytes_read = stream.read(&mut buffer).expect("refresh request should read");
			let response = format!(
				"HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
				body.len(),
				body
			);

			stream.write_all(response.as_bytes()).expect("refresh response should write");
		}
	});

	format!("http://{address}/oauth/token")
}
