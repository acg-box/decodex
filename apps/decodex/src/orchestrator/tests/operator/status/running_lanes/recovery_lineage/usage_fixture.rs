use crate::orchestrator::tests::operator::status::running_lanes::{
	HashMap, Read as _, Shutdown, TcpListener, Write as _, thread,
};

pub(super) fn start_codex_usage_fixture_server(
	responses: Vec<(&'static str, &'static str)>,
) -> String {
	let listener = TcpListener::bind("127.0.0.1:0").expect("usage fixture server should bind");
	let address = listener.local_addr().expect("usage fixture address should resolve");

	thread::spawn(move || {
		let responses_by_account = responses.into_iter().collect::<HashMap<_, _>>();
		let request_count = responses_by_account.len();

		for _ in 0..request_count {
			let (mut stream, _peer) =
				listener.accept().expect("usage fixture request should arrive");
			let mut request = [0_u8; 4_096];
			let bytes_read = stream.read(&mut request).expect("usage request should read");
			let request = String::from_utf8_lossy(&request[..bytes_read]);
			let account_id = usage_fixture_account_id(&request);
			let (status, body) = match account_id
				.and_then(|account_id| responses_by_account.get(account_id).copied())
			{
				Some(body) => ("200 OK", body),
				None => ("404 Not Found", r#"{"error":"unknown account"}"#),
			};
			let response = format!(
				"HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
				body.len(),
				body
			);

			stream.write_all(response.as_bytes()).expect("usage fixture response should write");

			let _ = stream.shutdown(Shutdown::Both);
		}
	});

	format!("http://{address}/wham/usage")
}

fn usage_fixture_account_id(request: &str) -> Option<&str> {
	request.lines().find_map(|line| {
		let (name, value) = line.split_once(':')?;

		name.eq_ignore_ascii_case("ChatGPT-Account-Id").then_some(value.trim())
	})
}
