//! Minimal, non-persistent Responses request using the existing account credential owner.
//! Wire reference: openai/codex abbdde95b593594c4daa2677392dbfd1dd4ccb8b,
//! codex-api/src/common.rs, endpoint/responses.rs and tests/sse_end_to_end.rs.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use decodex_core::{AccountId, AccountQuotaWindow};

use super::{AccountApiInventory, AccountApiObservation, AccountApiRuntime, BACKEND_API_BASE};

const ACTIVATION_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_STREAM_BYTES: usize = 256 * 1024;
const ACTIVATION_MODEL: &str = "gpt-5.6-sol";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ActivationOutcome {
	Completed,
	Rejected,
	Unknown,
}

impl AccountApiRuntime {
	pub(crate) async fn observe_and_activate(
		&self,
		account_id: &AccountId,
	) -> AccountApiObservation {
		let observation = self.observe_account(account_id).await;
		let Some(weekly) = observation.inventory.as_ref().ok().and_then(|inventory| {
			inventory
				.quota_windows
				.iter()
				.find(|window| window.duration_minutes == AccountQuotaWindow::SEVEN_DAYS_MINUTES)
				.and_then(|window| window.result.ok().flatten())
		}) else {
			return observation;
		};
		if !self
			.store
			.read_desktop_settings()
			.await
			.is_ok_and(|settings| settings.auto_activate_quota)
		{
			return observation;
		}
		// Reuse the credential lock and refresh owner. No auth file or child process is created.
		let Ok(credential) =
			self.accounts.api_credential_for_observation(account_id, ACTIVATION_TIMEOUT).await
		else {
			return observation;
		};
		if credential.account_revision != observation.account_revision {
			return observation;
		}
		let Some(now) = SystemTime::now()
			.duration_since(UNIX_EPOCH)
			.ok()
			.and_then(|time| i64::try_from(time.as_micros()).ok())
		else {
			return observation;
		};
		let can_send =
			observation.inventory.as_ref().is_ok_and(|inventory| can_activate(inventory, now));
		if !self
			.store
			.claim_quota_activation(account_id, credential.account_revision, weekly, can_send, now)
			.await
			.unwrap_or(false)
		{
			return observation;
		}
		let request = self
			.client
			.post(format!("{BACKEND_API_BASE}/codex/responses"))
			.timeout(ACTIVATION_TIMEOUT)
			.bearer_auth(credential.stored.bundle().access_token())
			.header("ChatGPT-Account-Id", credential.binding.provider.account_id())
			.header("Accept", "text/event-stream")
			.json(&activation_request());
		let outcome = send_activation(request).await;
		// Do not retain the credential lock while querying again.
		drop(credential);
		match outcome {
			ActivationOutcome::Completed | ActivationOutcome::Rejected => {
				let _ = self
					.store
					.finish_quota_activation(
						account_id,
						now,
						outcome == ActivationOutcome::Completed,
					)
					.await;
			},
			ActivationOutcome::Unknown => {},
		}
		// Read the provider's reset time even on ambiguous transport termination; never resend
		// just because a completed minimal request still rounds to 0 percent used.
		self.observe_account(account_id).await
	}
}

fn can_activate(inventory: &AccountApiInventory, now: i64) -> bool {
	if inventory.conditions.ordinary_requests_allowed(inventory.ordinary_usage_allowed)
		== Some(false)
	{
		return false;
	}
	// This probe activates an included weekly window. Existing paid credits alone
	// do not override an explicit refusal of included usage for this synthetic request.
	// The identity-checked provider decision is independent of displayed utilization.
	// Older backends omit it; retain their existing window-based activation behavior.
	inventory.ordinary_usage_allowed.unwrap_or_else(|| {
		inventory.quota_windows.iter().all(|window| match window.result {
			Ok(Some(fact)) => fact.resets_at_unix_micros <= now || fact.used_percent < 100,
			Ok(None) => window.duration_minutes == AccountQuotaWindow::FIVE_HOURS_MINUTES,
			Err(_) => false,
		})
	})
}

fn activation_request() -> serde_json::Value {
	serde_json::json!({
		"model": ACTIVATION_MODEL,
		"instructions": "Reply exactly OK. Do not use tools.",
		"input": [{"type":"message","role":"user","content":[{"type":"input_text","text":"Reply exactly OK."}]}],
		"tools": [], "tool_choice": "none", "parallel_tool_calls": false,
		"reasoning": {"effort":"low"}, "store": false, "stream": true, "include": []
	})
}

async fn send_activation(request: reqwest::RequestBuilder) -> ActivationOutcome {
	let mut response = match request.send().await {
		Ok(response) => response,
		Err(error) if error.is_connect() || error.is_builder() =>
			return ActivationOutcome::Rejected,
		Err(_) => return ActivationOutcome::Unknown,
	};
	// 5xx can occur after acceptance. Only explicit client rejection permits a later retry.
	if response.status().is_client_error() {
		return ActivationOutcome::Rejected;
	}
	if !response.status().is_success() {
		return ActivationOutcome::Unknown;
	}
	let mut parser = CompletionStream::default();
	loop {
		match response.chunk().await {
			Ok(Some(chunk)) => match parser.push(&chunk) {
				Ok(Some(outcome)) => return outcome,
				Ok(None) => {},
				Err(()) => return ActivationOutcome::Unknown,
			},
			Ok(None) | Err(_) => return ActivationOutcome::Unknown,
		}
	}
}

#[derive(Default)]
struct CompletionStream {
	bytes: Vec<u8>,
	data: Vec<u8>,
	total: usize,
}

impl CompletionStream {
	fn push(&mut self, chunk: &[u8]) -> Result<Option<ActivationOutcome>, ()> {
		self.total = self.total.checked_add(chunk.len()).ok_or(())?;
		if self.total > MAX_STREAM_BYTES {
			return Err(());
		}
		self.bytes.extend_from_slice(chunk);
		while let Some(end) = self.bytes.iter().position(|byte| *byte == b'\n') {
			let mut line: Vec<_> = self.bytes.drain(..=end).collect();
			line.pop();
			if line.last() == Some(&b'\r') {
				line.pop();
			}
			if line.is_empty() {
				if self.data.is_empty() {
					continue;
				}
				let event =
					serde_json::from_slice::<serde_json::Value>(&self.data).map_err(|_| ())?;
				self.data.clear();
				match event["type"].as_str() {
					Some("response.completed")
						if event["response"]["id"].as_str().is_some_and(|id| !id.is_empty())
							&& event["response"]["status"]
								.as_str()
								.is_none_or(|status| status == "completed") =>
						return Ok(Some(ActivationOutcome::Completed)),
					Some("response.failed" | "response.incomplete" | "error") =>
						return Ok(Some(ActivationOutcome::Unknown)),
					_ => {},
				}
			} else if let Some(data) = line.strip_prefix(b"data:") {
				if !self.data.is_empty() {
					self.data.push(b'\n');
				}
				self.data.extend_from_slice(data.strip_prefix(b" ").unwrap_or(data));
			}
		}
		Ok(None)
	}
}

#[cfg(test)]
mod tests {
	use super::{
		ActivationOutcome, CompletionStream, MAX_STREAM_BYTES, activation_request, send_activation,
	};
	use std::time::Duration;

	#[test]
	fn ordinary_denial_blocks_activation_even_after_the_displayed_reset() {
		let usage = decodex_codex::decode_account_api_usage(br#"{"rate_limit":{"primary_window":{"used_percent":0,"limit_window_seconds":604800,"reset_at":1800000000}}}"#).unwrap();
		let mut inventory = super::AccountApiInventory {
			account_revision: 1,
			ordinary_usage_allowed: Some(false),
			conditions: Default::default(),
			quota_windows: usage.quota_windows,
			reported_available_count: None,
			details_complete: false,
			credits: Vec::new(),
		};
		let after_reset = 1_800_000_001_000_000;
		assert!(!super::can_activate(&inventory, after_reset));
		inventory.conditions.has_credits = Some(true);
		assert!(
			!super::can_activate(&inventory, after_reset),
			"paid credits do not authorize the synthetic included-window probe"
		);
		inventory.ordinary_usage_allowed = None;
		assert!(super::can_activate(&inventory, after_reset));
		inventory.ordinary_usage_allowed = Some(true);
		assert!(super::can_activate(&inventory, after_reset));
		inventory.conditions.spend_control_reached = Some(true);
		assert!(!super::can_activate(&inventory, after_reset));
	}

	#[tokio::test]
	async fn failure_to_connect_can_retry_without_replaying_an_accepted_request() {
		let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("listener");
		let address = listener.local_addr().expect("address");
		drop(listener);
		let request = reqwest::Client::new()
			.post(format!("http://{address}/responses"))
			.timeout(Duration::from_secs(2))
			.json(&activation_request());
		assert_eq!(send_activation(request).await, ActivationOutcome::Rejected);
	}

	#[test]
	fn completion_handles_every_chunk_boundary_and_never_trusts_output_text() {
		let bytes = b"event: response.completed\r\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"r1\",\"status\":\"completed\"}}\r\n\r\n";
		for split in 0..bytes.len() {
			let mut parser = CompletionStream::default();
			assert_eq!(parser.push(&bytes[..split]), Ok(None));
			assert_eq!(parser.push(&bytes[split..]), Ok(Some(ActivationOutcome::Completed)));
		}
		let mut parser = CompletionStream::default();
		assert_eq!(
			parser.push(b"data: {\"type\":\"response.output_text.delta\",\"delta\":\"OK\"}\n\n"),
			Ok(None)
		);
		assert_eq!(
			parser.push(b"data: {\"type\":\"response.failed\"}\n\n"),
			Ok(Some(ActivationOutcome::Unknown))
		);
	}

	#[test]
	fn stream_is_bounded_and_malformed_completion_does_not_succeed() {
		assert_eq!(CompletionStream::default().push(&vec![b'x'; MAX_STREAM_BYTES + 1]), Err(()));
		assert_eq!(CompletionStream::default().push(b"data: invalid\n\n"), Err(()));
		assert_eq!(
			CompletionStream::default().push(b"data: {\"type\":\"response.completed\"}\n\n"),
			Ok(None)
		);
	}

	#[tokio::test]
	async fn http_request_has_no_storage_or_tools_and_requires_positive_completion() {
		use std::io::{Read as _, Write as _};
		for (status, body, expected) in [
			(
				"200 OK",
				"data: {\"type\":\"response.completed\",\"response\":{\"id\":\"r1\"}}\n\n",
				ActivationOutcome::Completed,
			),
			(
				"200 OK",
				"data: {\"type\":\"response.output_text.delta\",\"delta\":\"OK\"}\n\n",
				ActivationOutcome::Unknown,
			),
			("429 Too Many Requests", "", ActivationOutcome::Rejected),
			("503 Service Unavailable", "", ActivationOutcome::Unknown),
		] {
			let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("valid test fixture");
			let address = listener.local_addr().expect("valid test fixture");
			let server = std::thread::spawn(move || {
				let (mut socket, _) = listener.accept().expect("valid test fixture");
				socket.set_read_timeout(Some(Duration::from_secs(5))).expect("valid test fixture");
				let mut request = Vec::new();
				loop {
					let mut chunk = [0; 4096];
					let count = socket.read(&mut chunk).expect("valid test fixture");
					assert!(count > 0);
					request.extend_from_slice(&chunk[..count]);
					if let Some(end) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
						let headers = String::from_utf8_lossy(&request[..end]).to_lowercase();
						let length: usize = headers
							.lines()
							.find_map(|line| line.strip_prefix("content-length: "))
							.expect("valid test fixture")
							.parse()
							.expect("valid test fixture");
						if request.len() < end + 4 + length {
							continue;
						}
						let json: serde_json::Value = serde_json::from_slice(&request[end + 4..])
							.expect("valid test fixture");
						assert_eq!(json["store"], false);
						assert_eq!(json["tools"], serde_json::json!([]));
						assert_eq!(json["input"].as_array().expect("valid test fixture").len(), 1);
						assert!(json.get("previous_response_id").is_none());
						break;
					}
				}
				write!(socket,"HTTP/1.1 {status}\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).expect("valid test fixture");
			});
			let request = reqwest::Client::new()
				.post(format!("http://{address}/responses"))
				.timeout(Duration::from_secs(5))
				.json(&activation_request());
			assert_eq!(send_activation(request).await, expected);
			server.join().expect("valid test fixture");
		}
	}
}
