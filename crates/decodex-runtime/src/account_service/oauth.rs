//! OAuth wire protocol and refresh response interpretation.
//!
//! Protocol reference: openai/codex c2f7fe89d87ce853900d0b5cb1f5dc4863e44d73,
//! login/src/auth/manager.rs and login/tests/suite/auth_refresh.rs. Account locks,
//! durable effects and native process ownership remain in AccountService.
#[cfg(all(feature = "process-acceptance-fixture", debug_assertions))]
use super::process_acceptance_fixture_endpoint;
use super::{CredentialRefreshError, CredentialRefreshPort, CredentialRefreshResult};
use crate::{account_import, host_credentials::CredentialSecretBundle};
use reqwest::blocking::Response;
use serde::Deserialize;
use serde_json::Value;
use std::{
	io::Read as _,
	mem,
	time::{SystemTime, UNIX_EPOCH},
};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};
#[cfg(not(all(feature = "process-acceptance-fixture", debug_assertions)))]
pub(super) const REFRESH_ENDPOINT: &str = "https://auth.openai.com/oauth/token";
pub(super) const MAX_REFRESH_ERROR_BODY_BYTES: u64 = 4_096;
/// Exact OpenAI OAuth refresh adapter used by the Mac daemon.
pub(crate) struct OpenAiCredentialRefresher {
	transport: decodex_account_login::RefreshTransport,
}
impl OpenAiCredentialRefresher {
	/// Construct a bounded client without ambient credential configuration.
	pub(crate) fn new() -> Result<Self, CredentialRefreshError> {
		let transport =
			decodex_account_login::RefreshTransport::new().map_err(map_transport_error)?;
		Ok(Self { transport })
	}
}

impl CredentialRefreshPort for OpenAiCredentialRefresher {
	fn refresh(
		&self,
		current: &CredentialSecretBundle,
	) -> Result<CredentialRefreshResult, CredentialRefreshError> {
		self.exchange(current, &refresh_endpoint()?)
	}
}

impl OpenAiCredentialRefresher {
	fn exchange(
		&self,
		current: &CredentialSecretBundle,
		endpoint: &str,
	) -> Result<CredentialRefreshResult, CredentialRefreshError> {
		let response = self
			.transport
			.refresh(endpoint, current.refresh_token().ok_or(CredentialRefreshError::Rejected)?)
			.map_err(map_transport_error)?;
		let status = response.status();

		if !status.is_success() {
			return Err(classify_refresh_http_response(response));
		}

		let refreshed: RefreshResponse =
			response.json().map_err(|_| CredentialRefreshError::Ambiguous)?;
		let observed_at = SystemTime::now()
			.duration_since(UNIX_EPOCH)
			.map_err(|_| CredentialRefreshError::Ambiguous)?;
		let observed_at_micros = i64::try_from(observed_at.as_micros())
			.map_err(|_| CredentialRefreshError::Ambiguous)?;

		credential_refresh_result(current, refreshed, observed_at_micros)
	}
}
fn map_transport_error(
	error: decodex_account_login::RefreshTransportError,
) -> CredentialRefreshError {
	match error {
		decodex_account_login::RefreshTransportError::Unavailable =>
			CredentialRefreshError::Unavailable,
		decodex_account_login::RefreshTransportError::Ambiguous =>
			CredentialRefreshError::Ambiguous,
	}
}

#[derive(Deserialize, Zeroize, ZeroizeOnDrop)]
pub(super) struct RefreshResponse {
	pub(super) id_token: Option<String>,
	pub(super) access_token: Option<String>,
	pub(super) refresh_token: Option<String>,
	pub(super) token_type: Option<String>,
}

pub(super) fn classify_refresh_http_response(response: Response) -> CredentialRefreshError {
	let status = response.status();
	let mut body = Zeroizing::new(Vec::new());
	let _ = response.take(MAX_REFRESH_ERROR_BODY_BYTES + 1).read_to_end(&mut body);

	classify_refresh_http_failure(status, &body)
}

pub(super) fn classify_refresh_http_failure(
	status: reqwest::StatusCode,
	body: &[u8],
) -> CredentialRefreshError {
	if status == reqwest::StatusCode::UNAUTHORIZED {
		return CredentialRefreshError::Rejected;
	}
	if body.len() <= usize::try_from(MAX_REFRESH_ERROR_BODY_BYTES).unwrap_or(usize::MAX)
		&& let Ok(value) = serde_json::from_slice::<Value>(body)
	{
		// Match native OAuth code precedence, never diagnostic prose. Keep the error
		// body private and preserve the existing uncertain-response boundary.
		let text = |value: &Value| {
			value.as_str().filter(|text| !text.trim().is_empty()).map(str::to_owned)
		};
		let code = text(&value["error"])
			.or_else(|| text(&value["error"]["code"]))
			.or_else(|| text(&value["code"]));

		if code.as_deref().is_some_and(|code| {
			(status == reqwest::StatusCode::BAD_REQUEST
				&& code.eq_ignore_ascii_case("invalid_grant"))
				|| ["refresh_token_expired", "refresh_token_reused", "refresh_token_invalidated"]
					.iter()
					.any(|terminal| code.eq_ignore_ascii_case(terminal))
		}) {
			return CredentialRefreshError::Rejected;
		}
	}
	if status.is_client_error() || status.is_server_error() {
		CredentialRefreshError::Unavailable
	} else {
		CredentialRefreshError::Ambiguous
	}
}

pub(super) fn refresh_endpoint() -> Result<String, CredentialRefreshError> {
	#[cfg(all(feature = "process-acceptance-fixture", debug_assertions))]
	{
		process_acceptance_fixture_endpoint().ok_or(CredentialRefreshError::Unavailable)
	}
	#[cfg(not(all(feature = "process-acceptance-fixture", debug_assertions)))]
	{
		Ok(REFRESH_ENDPOINT.to_owned())
	}
}

#[cfg(all(feature = "process-acceptance-fixture", debug_assertions))]
pub(super) fn process_test_refresh_endpoint_is_safe(value: &str) -> bool {
	let Ok(endpoint) = reqwest::Url::parse(value) else {
		return false;
	};

	endpoint.scheme() == "http"
		&& endpoint.host_str() == Some("127.0.0.1")
		&& endpoint.port().is_some()
		&& endpoint.path() == "/oauth/token"
		&& endpoint.query().is_none()
		&& endpoint.fragment().is_none()
		&& endpoint.username().is_empty()
		&& endpoint.password().is_none()
}

pub(super) fn credential_refresh_result(
	current: &CredentialSecretBundle,
	mut refreshed: RefreshResponse,
	observed_at_micros: i64,
) -> Result<CredentialRefreshResult, CredentialRefreshError> {
	let mut access_token = refreshed
		.access_token
		.take()
		.or_else(|| Some(current.access_token().to_owned()))
		.filter(|value| !value.is_empty())
		.map(Zeroizing::new)
		.ok_or(CredentialRefreshError::Ambiguous)?;
	let mut refresh_token = refreshed
		.refresh_token
		.take()
		.or_else(|| current.refresh_token().map(str::to_owned))
		.map(Zeroizing::new)
		.ok_or(CredentialRefreshError::Rejected)?;
	let mut id_token = refreshed
		.id_token
		.take()
		.or_else(|| current.id_token().map(str::to_owned))
		.filter(|value| !value.is_empty())
		.map(Zeroizing::new)
		.ok_or(CredentialRefreshError::Ambiguous)?;
	let identity = account_import::decode_chatgpt_identity(&id_token)
		.map_err(|_| CredentialRefreshError::Ambiguous)?;
	// Omission preserves previously validated metadata; an explicitly malformed
	// replacement is still rejected. JWT expiry is the service's validity authority.
	let token_type = refreshed.token_type.take().unwrap_or_else(|| current.token_type().to_owned());

	let expires_at_micros = account_import::decode_expiry_micros(&access_token)
		.map_err(|_| CredentialRefreshError::Ambiguous)?;

	if expires_at_micros <= observed_at_micros {
		return Err(CredentialRefreshError::Ambiguous);
	}

	let bundle = CredentialSecretBundle::chatgpt(
		mem::take(&mut *access_token),
		mem::take(&mut *refresh_token),
		Some(mem::take(&mut *id_token)),
		identity.plan_type,
		identity.provider_email,
		token_type,
		expires_at_micros,
	)
	.map_err(|_| CredentialRefreshError::Ambiguous)?;

	Ok(CredentialRefreshResult { returned_provider: identity.provider, bundle })
}

#[cfg(test)]
mod tests {
	use super::{CredentialRefreshError, CredentialSecretBundle, OpenAiCredentialRefresher};
	use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
	use serde_json::Value;
	use std::{
		io::{BufRead as _, BufReader, Read as _, Write as _},
		net::TcpListener,
		thread,
		time::Duration,
	};

	fn jwt(claims: Value) -> String {
		format!("header.{}.signature", URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap()))
	}

	#[test]
	fn real_http_refresh_accepts_sparse_success_and_classifies_provider_failures() {
		let access = jwt(serde_json::json!({"exp":4_000_000_000_i64}));
		let identity = jwt(serde_json::json!({
			"email":"fixture@example.test",
			"https://api.openai.com/auth":{"chatgpt_account_id":"fixture-account","chatgpt_plan_type":"pro"}
		}));
		let current = CredentialSecretBundle::chatgpt(
			access.clone(),
			"fixture-old-refresh".into(),
			Some(identity),
			Some("pro".into()),
			Some("fixture@example.test".into()),
			"bearer".into(),
			4_000_000_000_000_000,
		)
		.unwrap();
		let listener = TcpListener::bind("127.0.0.1:0").unwrap();
		listener.set_nonblocking(true).unwrap();
		let endpoint = format!("http://{}/oauth/token", listener.local_addr().unwrap());
		let success =
			serde_json::json!({"access_token":access,"refresh_token":"fixture-new-refresh"})
				.to_string();
		let server = thread::spawn(move || {
			for (status, body) in [
				("200 OK", success),
				("503 Service Unavailable", "temporary failure".into()),
				("400 Bad Request", r#"{"error":"invalid_grant"}"#.into()),
			] {
				let deadline = std::time::Instant::now() + Duration::from_secs(10);
				let (stream, _) = loop {
					match listener.accept() {
						Ok(value) => break value,
						Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
							assert!(
								std::time::Instant::now() < deadline,
								"refresh request did not arrive"
							);
							thread::sleep(Duration::from_millis(5));
						},
						Err(error) => panic!("fixture accept: {error}"),
					}
				};
				stream.set_nonblocking(false).unwrap();
				stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
				let mut socket = BufReader::new(stream);
				let mut first = String::new();
				socket.read_line(&mut first).unwrap();
				assert_eq!(first, "POST /oauth/token HTTP/1.1\r\n");
				let mut length = 0;
				loop {
					let mut line = String::new();
					assert!(socket.read_line(&mut line).unwrap() > 0);
					if line == "\r\n" {
						break;
					}
					if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
						length = value.trim().parse::<usize>().unwrap();
					}
				}
				assert!(length < 4096);
				let mut body_bytes = vec![0; length];
				socket.read_exact(&mut body_bytes).unwrap();
				let grant: Value = serde_json::from_slice(&body_bytes).unwrap();
				assert_eq!(grant["grant_type"], "refresh_token");
				assert_eq!(grant["refresh_token"], "fixture-old-refresh");
				write!(
					socket.get_mut(),
					"HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
					body.len()
				)
				.unwrap();
			}
		});
		let adapter = OpenAiCredentialRefresher::new().unwrap();
		let refreshed = adapter.exchange(&current, &endpoint).unwrap();
		assert_eq!(refreshed.returned_provider.account_id(), "fixture-account");
		assert_eq!(refreshed.bundle.id_token(), current.id_token());
		assert_eq!(refreshed.bundle.refresh_token(), Some("fixture-new-refresh"));
		assert!(matches!(
			adapter.exchange(&current, &endpoint),
			Err(CredentialRefreshError::Unavailable)
		));
		assert!(matches!(
			adapter.exchange(&current, &endpoint),
			Err(CredentialRefreshError::Rejected)
		));
		server.join().unwrap();
	}
}
