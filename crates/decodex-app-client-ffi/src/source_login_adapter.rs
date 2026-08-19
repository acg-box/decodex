// Source-derived account login adapter.
//
// Upstream provenance: OpenAI Codex, Apache-2.0, peeled commit
// 9392c3fa5bcda342b5b96a1a04d67b2f781617c2 (tag rust-v0.148.0-alpha.9).
// Reviewed source functions:
// - login/src/pkce.rs: generate_pkce
// - login/src/server.rs: build_authorize_url, exchange_code_for_tokens,
//   persist_tokens_async
// - login/src/device_code_auth.rs: request_device_code,
//   complete_device_code_login
// - login/src/auth/storage.rs: FileAuthStorage::save
//
// Decodex modifications: one closed error surface; strict request/response
// bounds; loopback-only callback binding; typed cancellation; no logging;
// terminal device-poll classification; no browser launcher, terminal,
// executable, or child process; and an exact four-field auth document accepted
// by the daemon-owned import authority.

use std::{
	collections::HashMap,
	fs::{self, OpenOptions},
	future::Future,
	io::{ErrorKind, Read as _, Write as _},
	net::{Shutdown, TcpListener, TcpStream},
	os::unix::fs::{MetadataExt as _, OpenOptionsExt as _, PermissionsExt as _},
	path::Path,
	sync::{
		Arc,
		atomic::{AtomicBool, Ordering},
	},
	thread,
	time::{Duration, Instant},
};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use reqwest::{Client, Response as HttpResponse, redirect::Policy};
use serde::{Deserialize, Deserializer, Serialize, de};
use sha2::{Digest as _, Sha256};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use tokio::{runtime::Handle, sync::Notify};
use url::Url;
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

use crate::account_reauthentication::LoginMethod;

const DEFAULT_ISSUER: &str = "https://auth.openai.com";
const OAUTH_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const OAUTH_ORIGINATOR: &str = "codex_cli_rs";
const OAUTH_SCOPE: &str =
	"openid profile email offline_access api.connectors.read api.connectors.invoke";
const CALLBACK_PORTS: [u16; 2] = [1_455, 1_457];
const LOGIN_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const HTTP_TIMEOUT: Duration = Duration::from_secs(20);
const CALLBACK_POLL_INTERVAL: Duration = Duration::from_millis(50);
const MAX_CALLBACK_REQUEST_BYTES: usize = 16 * 1_024;
const MAX_CALLBACK_TARGET_BYTES: usize = 8 * 1_024;
const MAX_CALLBACK_HEADERS: usize = 64;
const MAX_CALLBACK_HEADER_BYTES: usize = 8 * 1_024;
const MAX_RESPONSE_BYTES: usize = 256 * 1_024;
const MAX_TOKEN_BYTES: usize = 128 * 1_024;
const MAX_DEVICE_VALUE_BYTES: usize = 1_024;
const MAX_DEVICE_POLL_INTERVAL_SECONDS: u64 = 60;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Error {
	Cancelled,
	TimedOut,
	Unavailable,
	Rejected,
	DeviceAuthorizationRejected,
	InvalidResponse,
	Persistence,
}

pub(crate) enum LoginEvent {
	BrowserAuthorization { authorization_url: String },
	DeviceAuthorization { verification_url: String, user_code: String },
}

#[derive(Clone, Default)]
pub(crate) struct Cancellation {
	cancelled: Arc<AtomicBool>,
	notify: Arc<Notify>,
}

impl Cancellation {
	pub(crate) fn cancel(&self) {
		self.cancelled.store(true, Ordering::Release);
		self.notify.notify_one();
	}

	pub(crate) fn is_cancelled(&self) -> bool {
		self.cancelled.load(Ordering::Acquire)
	}

	async fn cancelled(&self) {
		loop {
			if self.is_cancelled() {
				return;
			}
			self.notify.notified().await;
		}
	}
}

#[derive(Clone)]
pub(crate) struct Config {
	issuer: Url,
	client_id: String,
	callback_ports: Vec<u16>,
	login_timeout: Duration,
	http_timeout: Duration,
}

impl Config {
	pub(crate) fn production() -> Result<Self, Error> {
		let issuer = Url::parse(DEFAULT_ISSUER).map_err(|_| Error::Unavailable)?;
		let config = Self {
			issuer,
			client_id: OAUTH_CLIENT_ID.to_owned(),
			callback_ports: CALLBACK_PORTS.to_vec(),
			login_timeout: LOGIN_TIMEOUT,
			http_timeout: HTTP_TIMEOUT,
		};
		config.validate(false)?;
		Ok(config)
	}

	fn validate(&self, allow_insecure_loopback: bool) -> Result<(), Error> {
		let scheme_allowed = self.issuer.scheme() == "https"
			|| (allow_insecure_loopback
				&& self.issuer.scheme() == "http"
				&& self.issuer.host_str().is_some_and(is_loopback_host));
		if !scheme_allowed
			|| self.issuer.cannot_be_a_base()
			|| !self.issuer.username().is_empty()
			|| self.issuer.password().is_some()
			|| self.issuer.query().is_some()
			|| self.issuer.fragment().is_some()
			|| self.client_id.is_empty()
			|| self.client_id.len() > MAX_DEVICE_VALUE_BYTES
			|| self.callback_ports.is_empty()
			|| self.login_timeout.is_zero()
			|| self.http_timeout.is_zero()
		{
			return Err(Error::Unavailable);
		}
		Ok(())
	}

	#[cfg(test)]
	fn test(issuer: Url, callback_port: u16, login_timeout: Duration) -> Result<Self, Error> {
		let config = Self {
			issuer,
			client_id: OAUTH_CLIENT_ID.to_owned(),
			callback_ports: vec![callback_port],
			login_timeout,
			http_timeout: Duration::from_secs(2),
		};
		config.validate(true)?;
		Ok(config)
	}
}

pub(crate) fn run(
	config: &Config,
	method: LoginMethod,
	login_home: &Path,
	runtime: &Handle,
	cancellation: &Cancellation,
	publish: impl Fn(LoginEvent),
) -> Result<(), Error> {
	let client = Client::builder()
		.redirect(Policy::none())
		.connect_timeout(config.http_timeout)
		.read_timeout(config.http_timeout)
		.timeout(config.http_timeout)
		.user_agent(format!(
			"decodex/{} codex-login-source/rust-v0.148.0-alpha.9",
			env!("CARGO_PKG_VERSION")
		))
		.build()
		.map_err(|_| Error::Unavailable)?;
	let deadline = Instant::now() + config.login_timeout;
	match method {
		LoginMethod::BrowserRedirect => {
			run_browser(config, &client, login_home, runtime, cancellation, deadline, publish)
		},
		LoginMethod::DeviceCode => runtime.block_on(run_device(
			config,
			&client,
			login_home,
			cancellation,
			deadline,
			publish,
		)),
	}
}

fn run_browser(
	config: &Config,
	client: &Client,
	login_home: &Path,
	runtime: &Handle,
	cancellation: &Cancellation,
	deadline: Instant,
	publish: impl Fn(LoginEvent),
) -> Result<(), Error> {
	let server = bind_callback_server(&config.callback_ports)?;
	let actual_port = server.local_addr().map_err(|_| Error::Unavailable)?.port();
	let redirect_uri = format!("http://localhost:{actual_port}/auth/callback");
	let pkce = generate_pkce()?;
	let state = generate_state()?;
	let authorization_url = build_authorize_url(config, &redirect_uri, &pkce, &state)?.into();
	publish(LoginEvent::BrowserAuthorization { authorization_url });

	loop {
		check_cancel_or_timeout(cancellation, deadline)?;
		let wait = remaining(deadline)?.min(CALLBACK_POLL_INTERVAL);
		let (stream, peer) = match server.accept() {
			Ok(accepted) => accepted,
			Err(error) if error.kind() == ErrorKind::WouldBlock => {
				thread::sleep(wait);
				continue;
			},
			Err(_) => return Err(Error::Unavailable),
		};
		if !peer.ip().is_loopback() {
			continue;
		}
		let Some(request) = read_callback_request(stream, cancellation, deadline)? else {
			continue;
		};
		match handle_callback_request(
			request,
			config,
			client,
			login_home,
			runtime,
			cancellation,
			deadline,
			&redirect_uri,
			&pkce,
			&state,
		)? {
			CallbackOutcome::Continue => {},
			CallbackOutcome::Completed => return Ok(()),
		}
	}
}

async fn run_device(
	config: &Config,
	client: &Client,
	login_home: &Path,
	cancellation: &Cancellation,
	deadline: Instant,
	publish: impl Fn(LoginEvent),
) -> Result<(), Error> {
	let user_code_url = endpoint(config, "api/accounts/deviceauth/usercode")?;
	let request = DeviceCodeRequest { client_id: &config.client_id };
	let response =
		cancellable(cancellation, deadline, client.post(user_code_url).json(&request).send())
			.await?;
	if !response.status().is_success() {
		return Err(Error::Rejected);
	}
	let response: DeviceCodeResponse = read_bounded_json(response, cancellation, deadline).await?;
	let grant = DeviceGrant::new(response)?;
	let verification_url = endpoint(config, "codex/device")?.to_string();
	publish(LoginEvent::DeviceAuthorization {
		verification_url,
		user_code: grant.user_code.clone(),
	});

	let poll_url = endpoint(config, "api/accounts/deviceauth/token")?;
	loop {
		check_cancel_or_timeout(cancellation, deadline)?;
		let request = DevicePollRequest {
			device_auth_id: &grant.device_auth_id,
			user_code: &grant.user_code,
		};
		let response = cancellable(
			cancellation,
			deadline,
			client.post(poll_url.clone()).json(&request).send(),
		)
		.await?;
		match response.status().as_u16() {
			200..=299 => {
				let code: DeviceCodeSuccess =
					read_bounded_json(response, cancellation, deadline).await?;
				code.validate()?;
				let redirect_uri = endpoint(config, "deviceauth/callback")?.to_string();
				let pkce = PkceCodes {
					code_verifier: code.code_verifier.clone(),
					code_challenge: code.code_challenge.clone(),
				};
				return ExchangeContext { config, client, login_home, cancellation, deadline }
					.run(&redirect_uri, &pkce, &code.authorization_code)
					.await;
			},
			403 | 404 => {
				let failure: DevicePollFailure =
					read_bounded_json(response, cancellation, deadline).await?;
				if !failure.is_pending() {
					return Err(Error::DeviceAuthorizationRejected);
				}
				sleep_cancellable(cancellation, deadline, grant.interval).await?;
			},
			_ => return Err(Error::Rejected),
		}
	}
}

enum CallbackOutcome {
	Continue,
	Completed,
}

#[allow(clippy::too_many_arguments)]
fn handle_callback_request(
	request: CallbackRequest,
	config: &Config,
	client: &Client,
	login_home: &Path,
	runtime: &Handle,
	cancellation: &Cancellation,
	deadline: Instant,
	redirect_uri: &str,
	pkce: &PkceCodes,
	expected_state: &str,
) -> Result<CallbackOutcome, Error> {
	let CallbackRequest { target, mut stream } = request;
	let parsed = Url::parse(&format!("http://localhost{target}"));
	let Ok(parsed) = parsed else {
		respond_text(&mut stream, 400, "Invalid sign-in callback")?;
		return Ok(CallbackOutcome::Continue);
	};
	if parsed.path() != "/auth/callback" {
		respond_text(&mut stream, 404, "Not found")?;
		return Ok(CallbackOutcome::Continue);
	}
	let parameters = unique_query(&parsed)?;
	if parameters.get("state").map(String::as_str) != Some(expected_state) {
		respond_text(&mut stream, 400, "Sign-in state mismatch")?;
		return Ok(CallbackOutcome::Continue);
	}
	if parameters.contains_key("error") {
		respond_text(&mut stream, 400, "Sign-in was not completed")?;
		return Err(Error::Rejected);
	}
	let Some(code) = parameters.get("code") else {
		respond_text(&mut stream, 400, "Sign-in code is missing")?;
		return Err(Error::InvalidResponse);
	};
	if !valid_secret_scalar(code) {
		respond_text(&mut stream, 400, "Sign-in code is invalid")?;
		return Err(Error::InvalidResponse);
	}
	let result = runtime.block_on(
		ExchangeContext { config, client, login_home, cancellation, deadline }.run(
			redirect_uri,
			pkce,
			code,
		),
	);
	match result {
		Ok(()) => {
			respond_text(
				&mut stream,
				200,
				"Browser sign-in completed. Return to Decodex to finish.",
			)?;
			Ok(CallbackOutcome::Completed)
		},
		Err(error) => {
			respond_text(&mut stream, 400, "Sign-in could not be completed")?;
			Err(error)
		},
	}
}

struct ExchangeContext<'a> {
	config: &'a Config,
	client: &'a Client,
	login_home: &'a Path,
	cancellation: &'a Cancellation,
	deadline: Instant,
}

impl ExchangeContext<'_> {
	async fn run(
		self,
		redirect_uri: &str,
		pkce: &PkceCodes,
		authorization_code: &str,
	) -> Result<(), Error> {
		if !valid_secret_scalar(authorization_code) || !pkce.valid() {
			return Err(Error::InvalidResponse);
		}
		let token_url = endpoint(self.config, "oauth/token")?;
		let form = [
			("grant_type", "authorization_code"),
			("code", authorization_code),
			("redirect_uri", redirect_uri),
			("client_id", self.config.client_id.as_str()),
			("code_verifier", pkce.code_verifier.as_str()),
		];
		let mut serializer = url::form_urlencoded::Serializer::new(String::new());
		for (key, value) in form {
			serializer.append_pair(key, value);
		}
		let body = Zeroizing::new(serializer.finish());
		let response = cancellable(
			self.cancellation,
			self.deadline,
			self.client
				.post(token_url)
				.header("Content-Type", "application/x-www-form-urlencoded")
				.body(body.as_str().to_owned())
				.send(),
		)
		.await?;
		if !response.status().is_success() {
			return Err(Error::Rejected);
		}
		let tokens: ExchangedTokens =
			read_bounded_json(response, self.cancellation, self.deadline).await?;
		tokens.validate()?;
		persist_auth(self.login_home, &tokens)
	}
}

async fn cancellable<T>(
	cancellation: &Cancellation,
	deadline: Instant,
	future: impl Future<Output = Result<T, reqwest::Error>>,
) -> Result<T, Error> {
	let wait = remaining(deadline)?;
	tokio::select! {
		biased;
		_ = cancellation.cancelled() => Err(Error::Cancelled),
		result = tokio::time::timeout(wait, future) => match result {
			Ok(Ok(value)) => Ok(value),
			Ok(Err(_)) => Err(Error::Unavailable),
			Err(_) => Err(Error::TimedOut),
		},
	}
}

async fn read_bounded_json<T: for<'de> Deserialize<'de>>(
	mut response: HttpResponse,
	cancellation: &Cancellation,
	deadline: Instant,
) -> Result<T, Error> {
	if response.content_length().is_some_and(|length| length > MAX_RESPONSE_BYTES as u64) {
		return Err(Error::InvalidResponse);
	}
	let mut body = Zeroizing::new(Vec::new());
	loop {
		let chunk = cancellable(cancellation, deadline, response.chunk()).await?;
		let Some(chunk) = chunk else {
			break;
		};
		if body.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
			return Err(Error::InvalidResponse);
		}
		body.extend_from_slice(&chunk);
	}
	serde_json::from_slice(&body).map_err(|_| Error::InvalidResponse)
}

async fn sleep_cancellable(
	cancellation: &Cancellation,
	deadline: Instant,
	duration: Duration,
) -> Result<(), Error> {
	let wait = duration.min(remaining(deadline)?);
	tokio::select! {
		biased;
		_ = cancellation.cancelled() => Err(Error::Cancelled),
		_ = tokio::time::sleep(wait) => check_cancel_or_timeout(cancellation, deadline),
	}
}

fn endpoint(config: &Config, path: &str) -> Result<Url, Error> {
	config.issuer.join(path).map_err(|_| Error::Unavailable)
}

fn build_authorize_url(
	config: &Config,
	redirect_uri: &str,
	pkce: &PkceCodes,
	state: &str,
) -> Result<Url, Error> {
	if !pkce.valid() || !valid_secret_scalar(state) {
		return Err(Error::Unavailable);
	}
	let mut url = endpoint(config, "oauth/authorize")?;
	url.query_pairs_mut()
		.append_pair("response_type", "code")
		.append_pair("client_id", &config.client_id)
		.append_pair("redirect_uri", redirect_uri)
		.append_pair("scope", OAUTH_SCOPE)
		.append_pair("code_challenge", &pkce.code_challenge)
		.append_pair("code_challenge_method", "S256")
		.append_pair("id_token_add_organizations", "true")
		.append_pair("codex_cli_simplified_flow", "true")
		.append_pair("state", state)
		.append_pair("originator", OAUTH_ORIGINATOR);
	Ok(url)
}

#[derive(Clone, Zeroize, ZeroizeOnDrop)]
struct PkceCodes {
	code_verifier: String,
	code_challenge: String,
}

impl PkceCodes {
	fn valid(&self) -> bool {
		if self.code_verifier.len() < 43
			|| self.code_verifier.len() > 128
			|| self.code_challenge.len() != 43
			|| !self.code_verifier.bytes().all(|byte| {
				byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~')
			}) {
			return false;
		}
		let digest = Sha256::digest(self.code_verifier.as_bytes());
		URL_SAFE_NO_PAD.encode(digest) == self.code_challenge
	}
}

fn generate_pkce() -> Result<PkceCodes, Error> {
	let mut bytes = Zeroizing::new([0_u8; 64]);
	getrandom::fill(bytes.as_mut()).map_err(|_| Error::Unavailable)?;
	Ok(pkce_from_bytes(bytes.as_ref()))
}

fn pkce_from_bytes(bytes: &[u8]) -> PkceCodes {
	let code_verifier = URL_SAFE_NO_PAD.encode(bytes);
	let digest = Sha256::digest(code_verifier.as_bytes());
	let code_challenge = URL_SAFE_NO_PAD.encode(digest);
	PkceCodes { code_verifier, code_challenge }
}

fn generate_state() -> Result<String, Error> {
	let mut bytes = Zeroizing::new([0_u8; 32]);
	getrandom::fill(bytes.as_mut()).map_err(|_| Error::Unavailable)?;
	Ok(URL_SAFE_NO_PAD.encode(bytes.as_ref()))
}

fn bind_callback_server(ports: &[u16]) -> Result<TcpListener, Error> {
	for port in ports {
		if let Ok(server) = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, *port)) {
			server.set_nonblocking(true).map_err(|_| Error::Unavailable)?;
			return Ok(server);
		}
	}
	Err(Error::Unavailable)
}

struct CallbackRequest {
	target: String,
	stream: TcpStream,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CallbackParseFailure {
	Incomplete,
	TooLarge,
	TooManyHeaders,
	Invalid,
}

struct ParsedCallbackHead<'a> {
	target: &'a str,
}

fn parse_callback_head(bytes: &[u8]) -> Result<ParsedCallbackHead<'_>, CallbackParseFailure> {
	if bytes.len() > MAX_CALLBACK_REQUEST_BYTES {
		return Err(CallbackParseFailure::TooLarge);
	}
	let header_complete = bytes.windows(4).any(|window| window == b"\r\n\r\n");
	if !header_complete {
		return Err(if bytes.len() == MAX_CALLBACK_REQUEST_BYTES {
			CallbackParseFailure::TooLarge
		} else {
			CallbackParseFailure::Incomplete
		});
	}
	let mut headers = [httparse::EMPTY_HEADER; MAX_CALLBACK_HEADERS];
	let mut request = httparse::Request::new(&mut headers);
	let consumed = match request.parse(bytes) {
		Ok(httparse::Status::Complete(consumed)) => consumed,
		Ok(httparse::Status::Partial) => return Err(CallbackParseFailure::Incomplete),
		Err(httparse::Error::TooManyHeaders) => return Err(CallbackParseFailure::TooManyHeaders),
		Err(_) => return Err(CallbackParseFailure::Invalid),
	};
	if consumed != bytes.len()
		|| request.method != Some("GET")
		|| !matches!(request.version, Some(0 | 1))
	{
		return Err(CallbackParseFailure::Invalid);
	}
	let target = request.path.ok_or(CallbackParseFailure::Invalid)?;
	if target.len() > MAX_CALLBACK_TARGET_BYTES {
		return Err(CallbackParseFailure::TooLarge);
	}
	let mut header_bytes = 0_usize;
	for header in request.headers.iter() {
		header_bytes =
			header_bytes.saturating_add(header.name.len()).saturating_add(header.value.len());
		if header_bytes > MAX_CALLBACK_HEADER_BYTES
			|| header.name.eq_ignore_ascii_case("transfer-encoding")
		{
			return Err(CallbackParseFailure::TooLarge);
		}
		if header.name.eq_ignore_ascii_case("content-length") {
			let value =
				std::str::from_utf8(header.value).map_err(|_| CallbackParseFailure::Invalid)?;
			if value.trim() != "0" {
				return Err(CallbackParseFailure::Invalid);
			}
		}
	}
	Ok(ParsedCallbackHead { target })
}

fn read_callback_request(
	mut stream: TcpStream,
	cancellation: &Cancellation,
	deadline: Instant,
) -> Result<Option<CallbackRequest>, Error> {
	stream.set_nonblocking(false).map_err(|_| Error::Unavailable)?;
	stream.set_read_timeout(Some(CALLBACK_POLL_INTERVAL)).map_err(|_| Error::Unavailable)?;
	stream.set_write_timeout(Some(CALLBACK_POLL_INTERVAL)).map_err(|_| Error::Unavailable)?;
	let mut bytes = [0_u8; MAX_CALLBACK_REQUEST_BYTES];
	let mut length = 0_usize;
	loop {
		check_cancel_or_timeout(cancellation, deadline)?;
		if length == bytes.len() {
			let _ = respond_text(&mut stream, 400, "Invalid sign-in callback");
			return Ok(None);
		}
		match stream.read(&mut bytes[length..]) {
			Ok(0) => return Ok(None),
			Ok(read) => length += read,
			Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
				continue;
			},
			Err(_) => return Ok(None),
		}
		match parse_callback_head(&bytes[..length]) {
			Ok(head) => {
				return Ok(Some(CallbackRequest { target: head.target.to_owned(), stream }));
			},
			Err(CallbackParseFailure::Incomplete) => {},
			Err(
				CallbackParseFailure::TooLarge
				| CallbackParseFailure::TooManyHeaders
				| CallbackParseFailure::Invalid,
			) => {
				let _ = respond_text(&mut stream, 400, "Invalid sign-in callback");
				return Ok(None);
			},
		}
	}
}

fn unique_query(url: &Url) -> Result<HashMap<String, String>, Error> {
	let mut parameters = HashMap::new();
	for (key, value) in url.query_pairs() {
		if key.len() > MAX_DEVICE_VALUE_BYTES
			|| value.len() > MAX_DEVICE_VALUE_BYTES
			|| parameters.insert(key.into_owned(), value.into_owned()).is_some()
			|| parameters.len() > 8
		{
			return Err(Error::InvalidResponse);
		}
	}
	Ok(parameters)
}

fn respond_text(stream: &mut TcpStream, status: u16, message: &'static str) -> Result<(), Error> {
	let reason = match status {
		200 => "OK",
		400 => "Bad Request",
		404 => "Not Found",
		_ => return Err(Error::Unavailable),
	};
	let response = format!(
		"HTTP/1.1 {status} {reason}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{message}",
		message.len(),
	);
	stream.write_all(response.as_bytes()).map_err(|_| Error::Unavailable)?;
	stream.flush().map_err(|_| Error::Unavailable)?;
	stream.shutdown(Shutdown::Write).map_err(|_| Error::Unavailable)
}

fn check_cancel_or_timeout(cancellation: &Cancellation, deadline: Instant) -> Result<(), Error> {
	if cancellation.is_cancelled() {
		return Err(Error::Cancelled);
	}
	if Instant::now() >= deadline {
		return Err(Error::TimedOut);
	}
	Ok(())
}

fn remaining(deadline: Instant) -> Result<Duration, Error> {
	deadline
		.checked_duration_since(Instant::now())
		.filter(|value| !value.is_zero())
		.ok_or(Error::TimedOut)
}

fn is_loopback_host(host: &str) -> bool {
	host.eq_ignore_ascii_case("localhost")
		|| host.parse::<std::net::IpAddr>().is_ok_and(|address| address.is_loopback())
}

fn valid_secret_scalar(value: &str) -> bool {
	!value.is_empty()
		&& value.len() <= MAX_TOKEN_BYTES
		&& value.trim() == value
		&& !value.chars().any(char::is_control)
}

#[derive(Serialize)]
struct DeviceCodeRequest<'a> {
	client_id: &'a str,
}

#[derive(Deserialize, Zeroize, ZeroizeOnDrop)]
struct DeviceCodeResponse {
	device_auth_id: String,
	#[serde(alias = "user_code", alias = "usercode")]
	user_code: String,
	#[serde(default, deserialize_with = "deserialize_interval")]
	interval: u64,
}

fn deserialize_interval<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
	D: Deserializer<'de>,
{
	let interval = String::deserialize(deserializer)?;
	interval.trim().parse::<u64>().map_err(de::Error::custom)
}

#[derive(Zeroize, ZeroizeOnDrop)]
struct DeviceGrant {
	device_auth_id: String,
	user_code: String,
	#[zeroize(skip)]
	interval: Duration,
}

impl DeviceGrant {
	fn new(response: DeviceCodeResponse) -> Result<Self, Error> {
		if !valid_device_value(&response.device_auth_id)
			|| !valid_user_code(&response.user_code)
			|| response.interval == 0
			|| response.interval > MAX_DEVICE_POLL_INTERVAL_SECONDS
		{
			return Err(Error::InvalidResponse);
		}
		Ok(Self {
			device_auth_id: response.device_auth_id.clone(),
			user_code: response.user_code.clone(),
			interval: Duration::from_secs(response.interval),
		})
	}
}

fn valid_device_value(value: &str) -> bool {
	!value.is_empty()
		&& value.len() <= MAX_DEVICE_VALUE_BYTES
		&& value.trim() == value
		&& !value.chars().any(char::is_control)
}

fn valid_user_code(value: &str) -> bool {
	let bytes = value.as_bytes();
	(9..=10).contains(&bytes.len())
		&& bytes.get(4) == Some(&b'-')
		&& bytes
			.iter()
			.enumerate()
			.all(|(index, byte)| index == 4 || byte.is_ascii_uppercase() || byte.is_ascii_digit())
}

#[derive(Serialize)]
struct DevicePollRequest<'a> {
	device_auth_id: &'a str,
	user_code: &'a str,
}

#[derive(Deserialize, Zeroize, ZeroizeOnDrop)]
struct DevicePollFailure {
	error: DevicePollError,
}

impl DevicePollFailure {
	fn is_pending(&self) -> bool {
		matches!(
			self.error.code.as_str(),
			"authorization_pending"
				| "deviceauth_authorization_pending"
				| "deviceauth_authorization_unknown"
		)
	}
}

#[derive(Deserialize, Zeroize, ZeroizeOnDrop)]
struct DevicePollError {
	code: String,
}

#[derive(Deserialize, Zeroize, ZeroizeOnDrop)]
struct DeviceCodeSuccess {
	authorization_code: String,
	code_challenge: String,
	code_verifier: String,
}

impl DeviceCodeSuccess {
	fn validate(&self) -> Result<(), Error> {
		if !valid_secret_scalar(&self.authorization_code)
			|| !(PkceCodes {
				code_verifier: self.code_verifier.clone(),
				code_challenge: self.code_challenge.clone(),
			})
			.valid()
		{
			return Err(Error::InvalidResponse);
		}
		Ok(())
	}
}

#[derive(Deserialize, Zeroize, ZeroizeOnDrop)]
struct ExchangedTokens {
	id_token: String,
	access_token: String,
	refresh_token: String,
}

impl ExchangedTokens {
	fn validate(&self) -> Result<(), Error> {
		if [self.id_token.as_str(), self.access_token.as_str(), self.refresh_token.as_str()]
			.into_iter()
			.all(valid_secret_scalar)
		{
			Ok(())
		} else {
			Err(Error::InvalidResponse)
		}
	}
}

#[derive(Deserialize)]
struct IdTokenClaims {
	#[serde(rename = "https://api.openai.com/auth")]
	authority: Option<IdTokenAuthority>,
}

#[derive(Deserialize)]
struct IdTokenAuthority {
	chatgpt_account_id: Option<String>,
}

fn provider_account_id(id_token: &str) -> Result<Zeroizing<String>, Error> {
	let mut parts = id_token.split('.');
	let (Some(header), Some(payload), Some(signature), None) =
		(parts.next(), parts.next(), parts.next(), parts.next())
	else {
		return Err(Error::InvalidResponse);
	};
	if header.is_empty() || payload.is_empty() || signature.is_empty() {
		return Err(Error::InvalidResponse);
	}
	let decoded =
		Zeroizing::new(URL_SAFE_NO_PAD.decode(payload).map_err(|_| Error::InvalidResponse)?);
	if decoded.len() > MAX_RESPONSE_BYTES {
		return Err(Error::InvalidResponse);
	}
	let claims: IdTokenClaims =
		serde_json::from_slice(&decoded).map_err(|_| Error::InvalidResponse)?;
	let account_id = claims
		.authority
		.and_then(|authority| authority.chatgpt_account_id)
		.filter(|value| valid_device_value(value))
		.ok_or(Error::InvalidResponse)?;
	Ok(Zeroizing::new(account_id))
}

#[derive(Serialize)]
struct AuthDocument<'a> {
	auth_mode: &'static str,
	#[serde(rename = "OPENAI_API_KEY")]
	openai_api_key: Option<&'static str>,
	tokens: AuthTokens<'a>,
	last_refresh: String,
}

#[derive(Serialize)]
struct AuthTokens<'a> {
	id_token: &'a str,
	access_token: &'a str,
	refresh_token: &'a str,
	account_id: &'a str,
}

fn persist_auth(login_home: &Path, tokens: &ExchangedTokens) -> Result<(), Error> {
	let account_id = provider_account_id(&tokens.id_token)?;
	let last_refresh =
		OffsetDateTime::now_utc().format(&Rfc3339).map_err(|_| Error::Persistence)?;
	let document = AuthDocument {
		auth_mode: "chatgpt",
		openai_api_key: None,
		tokens: AuthTokens {
			id_token: &tokens.id_token,
			access_token: &tokens.access_token,
			refresh_token: &tokens.refresh_token,
			account_id: &account_id,
		},
		last_refresh,
	};
	let bytes =
		Zeroizing::new(serde_json::to_vec_pretty(&document).map_err(|_| Error::Persistence)?);
	let auth_path = login_home.join("auth.json");
	let temporary_path = login_home.join(".decodex-auth.json.tmp");
	if fs::symlink_metadata(&auth_path).is_ok() || fs::symlink_metadata(&temporary_path).is_ok() {
		return Err(Error::Persistence);
	}
	let result = (|| {
		let mut file = OpenOptions::new()
			.create_new(true)
			.write(true)
			.mode(0o600)
			.open(&temporary_path)
			.map_err(|_| Error::Persistence)?;
		file.write_all(&bytes).map_err(|_| Error::Persistence)?;
		file.sync_all().map_err(|_| Error::Persistence)?;
		drop(file);
		verify_private_regular_file(&temporary_path)?;
		fs::rename(&temporary_path, &auth_path).map_err(|_| Error::Persistence)?;
		verify_private_regular_file(&auth_path)
	})();
	if result.is_err() {
		let _ = fs::remove_file(&temporary_path);
	}
	result
}

fn verify_private_regular_file(path: &Path) -> Result<(), Error> {
	let metadata = fs::symlink_metadata(path).map_err(|_| Error::Persistence)?;
	let canonical = fs::canonicalize(path).map_err(|_| Error::Persistence)?;
	if canonical != path
		|| !metadata.file_type().is_file()
		|| metadata.uid() != unsafe { libc::geteuid() }
		|| metadata.nlink() != 1
		|| metadata.permissions().mode() & 0o077 != 0
	{
		return Err(Error::Persistence);
	}
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::{
		sync::{Mutex, mpsc},
		thread,
	};

	struct MockHttpRequest {
		method: String,
		target: String,
		body: Vec<u8>,
	}

	struct MockIssuer {
		issuer: Url,
		stopped: Arc<AtomicBool>,
		worker: Option<thread::JoinHandle<()>>,
	}

	impl MockIssuer {
		fn start(
			handler: impl Fn(MockHttpRequest) -> (u16, String) + Send + Sync + 'static,
		) -> Self {
			let server =
				TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).expect("mock issuer");
			server.set_nonblocking(true).expect("nonblocking mock issuer");
			let port = server.local_addr().expect("mock issuer address").port();
			let issuer = Url::parse(&format!("http://127.0.0.1:{port}/")).expect("mock issuer URL");
			let stopped = Arc::new(AtomicBool::new(false));
			let worker_stopped = Arc::clone(&stopped);
			let handler = Arc::new(handler);
			let worker = thread::spawn(move || {
				while !worker_stopped.load(Ordering::Acquire) {
					match server.accept() {
						Ok((mut stream, peer)) => {
							assert!(peer.ip().is_loopback());
							let request = read_mock_request(&mut stream);
							let (status, body) = handler(request);
							write_mock_json(&mut stream, status, &body);
						},
						Err(error) if error.kind() == ErrorKind::WouldBlock => {
							thread::sleep(Duration::from_millis(10));
						},
						Err(error) => panic!("mock issuer accept failed: {error}"),
					}
				}
			});
			Self { issuer, stopped, worker: Some(worker) }
		}
	}

	impl Drop for MockIssuer {
		fn drop(&mut self) {
			self.stopped.store(true, Ordering::Release);
			if let Some(worker) = self.worker.take()
				&& let Err(payload) = worker.join()
				&& !thread::panicking()
			{
				std::panic::resume_unwind(payload);
			}
		}
	}

	fn read_mock_request(stream: &mut TcpStream) -> MockHttpRequest {
		stream.set_read_timeout(Some(Duration::from_secs(2))).expect("mock issuer read timeout");
		let mut bytes = [0_u8; MAX_CALLBACK_REQUEST_BYTES];
		let mut length = 0_usize;
		loop {
			assert!(length < bytes.len(), "mock request exceeded fixed buffer");
			let read = stream.read(&mut bytes[length..]).expect("mock issuer request");
			assert!(read > 0, "mock issuer request ended before completion");
			length += read;

			let parsed = {
				let mut headers = [httparse::EMPTY_HEADER; MAX_CALLBACK_HEADERS];
				let mut request = httparse::Request::new(&mut headers);
				let head_length =
					match request.parse(&bytes[..length]).expect("valid mock issuer request") {
						httparse::Status::Complete(length) => length,
						httparse::Status::Partial => continue,
					};
				let method = request.method.expect("mock request method").to_owned();
				let target = request.path.expect("mock request target").to_owned();
				let mut content_length = None;
				for header in request.headers.iter() {
					assert!(!header.name.eq_ignore_ascii_case("transfer-encoding"));
					if header.name.eq_ignore_ascii_case("content-length") {
						assert!(content_length.is_none(), "duplicate content length");
						let value = std::str::from_utf8(header.value)
							.expect("ASCII mock content length")
							.parse::<usize>()
							.expect("numeric mock content length");
						content_length = Some(value);
					}
				}
				(method, target, head_length, content_length.unwrap_or(0))
			};
			let (method, target, head_length, body_length) = parsed;
			let total_length =
				head_length.checked_add(body_length).expect("bounded mock request length");
			assert!(total_length <= bytes.len(), "mock request exceeded fixed buffer");
			if length < total_length {
				continue;
			}
			assert_eq!(length, total_length, "mock request contained trailing bytes");
			return MockHttpRequest {
				method,
				target,
				body: bytes[head_length..total_length].to_vec(),
			};
		}
	}

	fn write_mock_json(stream: &mut TcpStream, status: u16, body: &str) {
		let reason = match status {
			200 => "OK",
			400 => "Bad Request",
			403 => "Forbidden",
			404 => "Not Found",
			_ => panic!("unsupported mock status"),
		};
		let head = format!(
			"HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
			body.len(),
		);
		stream.write_all(head.as_bytes()).expect("mock response head");
		stream.write_all(body.as_bytes()).expect("mock response body");
		stream.flush().expect("mock response flush");
	}

	fn fixture_id_token() -> String {
		let payload = URL_SAFE_NO_PAD.encode(
			br#"{"email":"fixture@example.test","https://api.openai.com/auth":{"chatgpt_account_id":"fixture-account","chatgpt_plan_type":"plus"}}"#,
		);
		format!("header.{payload}.signature")
	}

	fn fixture_token_response() -> String {
		serde_json::json!({
			"id_token": fixture_id_token(),
			"access_token": "header.eyJleHAiOjQxMDI0NDQ4MDB9.signature",
			"refresh_token": "fixture-refresh",
		})
		.to_string()
	}

	fn fixture_device_success() -> String {
		let pkce = pkce_from_bytes(&[9_u8; 64]);
		serde_json::json!({
			"authorization_code": "fixture-authorization",
			"code_challenge": pkce.code_challenge,
			"code_verifier": pkce.code_verifier,
		})
		.to_string()
	}

	fn runtime() -> tokio::runtime::Runtime {
		tokio::runtime::Builder::new_multi_thread()
			.enable_all()
			.worker_threads(2)
			.build()
			.expect("test runtime")
	}

	fn canonical_temp_home() -> (tempfile::TempDir, std::path::PathBuf) {
		let home = tempfile::tempdir().expect("temporary login home");
		let path = fs::canonicalize(home.path()).expect("canonical temporary login home");
		(home, path)
	}

	fn token_only_issuer() -> MockIssuer {
		MockIssuer::start(|request| {
			assert_eq!(request.method, "POST");
			assert_eq!(request.target, "/oauth/token");
			let fields = url::form_urlencoded::parse(&request.body).collect::<HashMap<_, _>>();
			assert_eq!(fields.len(), 5);
			assert!(fields.contains_key("grant_type"));
			assert!(fields.contains_key("code"));
			assert!(fields.contains_key("redirect_uri"));
			assert!(fields.contains_key("client_id"));
			assert!(fields.contains_key("code_verifier"));
			(200, fixture_token_response())
		})
	}

	fn device_issuer(pending: bool) -> MockIssuer {
		MockIssuer::start(move |request| {
			assert_eq!(request.method, "POST");
			match request.target.as_str() {
				"/api/accounts/deviceauth/usercode" => {
					let value: serde_json::Value =
						serde_json::from_slice(&request.body).expect("device request JSON");
					assert!(value.get("client_id").and_then(serde_json::Value::as_str).is_some());
					(
						200,
						serde_json::json!({
							"device_auth_id": "fixture-device",
							"user_code": "FIXT-URE1",
							"interval": "1",
						})
						.to_string(),
					)
				},
				"/api/accounts/deviceauth/token" => {
					let value: serde_json::Value =
						serde_json::from_slice(&request.body).expect("device poll JSON");
					assert!(
						value.get("device_auth_id").and_then(serde_json::Value::as_str).is_some()
					);
					assert!(value.get("user_code").and_then(serde_json::Value::as_str).is_some());
					if pending {
						(
							403,
							serde_json::json!({
								"error": {
									"code": "deviceauth_authorization_pending",
									"message": "fixture pending",
									"param": null,
									"type": "authorization_error",
								},
							})
							.to_string(),
						)
					} else {
						(200, fixture_device_success())
					}
				},
				"/oauth/token" => {
					let fields =
						url::form_urlencoded::parse(&request.body).collect::<HashMap<_, _>>();
					assert_eq!(fields.len(), 5);
					(200, fixture_token_response())
				},
				_ => (404, "{}".to_owned()),
			}
		})
	}

	fn terminal_device_rejection_issuer() -> MockIssuer {
		MockIssuer::start(|request| {
			assert_eq!(request.method, "POST");
			match request.target.as_str() {
				"/api/accounts/deviceauth/usercode" => (
					200,
					serde_json::json!({
						"device_auth_id": "fixture-device",
						"user_code": "FIXT-URE1",
						"interval": "1",
					})
					.to_string(),
				),
				"/api/accounts/deviceauth/token" => (
					403,
					serde_json::json!({
						"error": {
							"code": "deviceauth_authorization_denied",
							"message": "fixture terminal denial",
							"param": null,
							"type": "authorization_error",
						},
					})
					.to_string(),
				),
				_ => (404, "{}".to_owned()),
			}
		})
	}

	#[test]
	fn authorize_url_matches_the_pinned_codex_contract() {
		let config = Config::production().expect("production login config");
		let pkce = pkce_from_bytes(&[7_u8; 64]);
		let state = URL_SAFE_NO_PAD.encode([8_u8; 32]);
		let url =
			build_authorize_url(&config, "http://localhost:1455/auth/callback", &pkce, &state)
				.expect("authorize URL");
		let query = url.query_pairs().into_owned().collect::<HashMap<_, _>>();

		assert_eq!(url.scheme(), "https");
		assert_eq!(url.host_str(), Some("auth.openai.com"));
		assert_eq!(url.path(), "/oauth/authorize");
		assert_eq!(query.get("response_type").map(String::as_str), Some("code"));
		assert_eq!(query.get("client_id").map(String::as_str), Some(OAUTH_CLIENT_ID));
		assert_eq!(query.get("scope").map(String::as_str), Some(OAUTH_SCOPE));
		assert_eq!(query.get("code_challenge_method").map(String::as_str), Some("S256"));
		assert_eq!(query.get("originator").map(String::as_str), Some(OAUTH_ORIGINATOR));
		assert_eq!(query.get("state").map(String::as_str), Some(state.as_str()));
		assert!(pkce.valid());
	}

	#[test]
	fn login_events_are_structured_and_credential_negative() {
		let browser = LoginEvent::BrowserAuthorization {
			authorization_url: "https://auth.openai.com/oauth/authorize?fixture=true".to_owned(),
		};
		let device = LoginEvent::DeviceAuthorization {
			verification_url: "https://auth.openai.com/codex/device".to_owned(),
			user_code: "FIXT-URE1".to_owned(),
		};

		assert!(matches!(browser, LoginEvent::BrowserAuthorization { .. }));
		assert!(matches!(device, LoginEvent::DeviceAuthorization { .. }));
	}

	#[test]
	fn private_auth_document_has_the_exact_daemon_schema_and_mode() {
		let home = tempfile::tempdir().expect("login home");
		let home_path = fs::canonicalize(home.path()).expect("canonical login home");
		let payload = URL_SAFE_NO_PAD
			.encode(br#"{"https://api.openai.com/auth":{"chatgpt_account_id":"fixture-account"}}"#);
		let tokens = ExchangedTokens {
			id_token: format!("header.{payload}.signature"),
			access_token: "header.eyJleHAiOjQxMDI0NDQ4MDB9.signature".to_owned(),
			refresh_token: "fixture-refresh".to_owned(),
		};

		persist_auth(&home_path, &tokens).expect("private auth persistence");

		let path = home_path.join("auth.json");
		let metadata = fs::symlink_metadata(&path).expect("auth metadata");
		let value: serde_json::Value =
			serde_json::from_slice(&fs::read(path).expect("auth bytes")).expect("auth JSON");
		let mut keys =
			value.as_object().expect("auth object").keys().map(String::as_str).collect::<Vec<_>>();
		keys.sort_unstable();
		assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
		assert_eq!(keys, ["OPENAI_API_KEY", "auth_mode", "last_refresh", "tokens"]);
		assert!(value["OPENAI_API_KEY"].is_null());
		assert_eq!(value["tokens"].as_object().expect("token object").len(), 4,);
	}

	#[test]
	fn cancellation_is_typed_and_sticky() {
		let cancellation = Cancellation::default();
		assert!(!cancellation.is_cancelled());
		cancellation.cancel();
		assert!(cancellation.is_cancelled());
	}

	#[test]
	fn browser_callback_completes_in_process_and_persists_the_private_auth_file() {
		let issuer = token_only_issuer();
		let config =
			Config::test(issuer.issuer.clone(), 0, Duration::from_secs(5)).expect("browser config");
		let (_home, home_path) = canonical_temp_home();
		let runtime = runtime();
		let handle = runtime.handle().clone();
		let cancellation = Cancellation::default();
		let worker_cancellation = cancellation.clone();
		let worker_config = config.clone();
		let worker_home = home_path.clone();
		let (event_tx, event_rx) = mpsc::channel();
		let worker = thread::spawn(move || {
			run(
				&worker_config,
				LoginMethod::BrowserRedirect,
				&worker_home,
				&handle,
				&worker_cancellation,
				|event| {
					let _ = event_tx.send(event);
				},
			)
		});
		let authorization_url = match event_rx
			.recv_timeout(Duration::from_secs(2))
			.expect("browser authorization event")
		{
			LoginEvent::BrowserAuthorization { authorization_url } => authorization_url,
			LoginEvent::DeviceAuthorization { .. } => panic!("unexpected device event"),
		};
		let authorization_url = Url::parse(&authorization_url).expect("authorization URL");
		let parameters = authorization_url.query_pairs().into_owned().collect::<HashMap<_, _>>();
		let redirect_uri = parameters.get("redirect_uri").expect("redirect URI");
		let state = parameters.get("state").expect("state");
		let mut callback = Url::parse(redirect_uri).expect("callback URL");
		callback
			.query_pairs_mut()
			.append_pair("code", "fixture-browser-authorization")
			.append_pair("state", state);
		let response = reqwest::blocking::Client::builder()
			.redirect(Policy::none())
			.build()
			.expect("callback client")
			.get(callback)
			.send()
			.expect("callback response");
		assert_eq!(response.status().as_u16(), 200);
		assert_eq!(
			response.text().expect("callback response body"),
			"Browser sign-in completed. Return to Decodex to finish."
		);
		let result = worker.join().expect("browser adapter worker");

		assert!(result.is_ok());
		assert!(home_path.join("auth.json").is_file());
		assert!(!home_path.join(".decodex-auth.json.tmp").exists());
	}

	#[test]
	fn device_code_completion_is_structured_and_uses_the_same_auth_handoff() {
		let issuer = device_issuer(false);
		let config =
			Config::test(issuer.issuer.clone(), 0, Duration::from_secs(5)).expect("device config");
		let (_home, home_path) = canonical_temp_home();
		let runtime = runtime();
		let events = Mutex::new(Vec::new());
		let result = run(
			&config,
			LoginMethod::DeviceCode,
			&home_path,
			runtime.handle(),
			&Cancellation::default(),
			|event| events.lock().expect("event lock").push(event),
		);

		assert!(result.is_ok());
		let events = events.into_inner().expect("events");
		assert_eq!(events.len(), 1);
		assert!(matches!(events.first(), Some(LoginEvent::DeviceAuthorization { .. })));
		assert!(home_path.join("auth.json").is_file());
	}

	#[test]
	fn terminal_device_poll_rejection_does_not_wait_for_the_login_timeout() {
		let issuer = terminal_device_rejection_issuer();
		let config = Config::test(issuer.issuer.clone(), 0, Duration::from_millis(80))
			.expect("device config");
		let (_home, home_path) = canonical_temp_home();
		let runtime = runtime();
		let result = run(
			&config,
			LoginMethod::DeviceCode,
			&home_path,
			runtime.handle(),
			&Cancellation::default(),
			|_| {},
		);

		assert!(matches!(result, Err(Error::DeviceAuthorizationRejected)));
		assert!(!home_path.join("auth.json").exists());
	}

	#[test]
	fn browser_state_mismatch_is_rejected_without_ending_the_session() {
		let config = Config::test(
			Url::parse("http://127.0.0.1:9/").expect("fixture issuer"),
			0,
			Duration::from_secs(5),
		)
		.expect("browser config");
		let (_home, home_path) = canonical_temp_home();
		let runtime = runtime();
		let handle = runtime.handle().clone();
		let cancellation = Cancellation::default();
		let worker_cancellation = cancellation.clone();
		let worker_config = config.clone();
		let worker_home = home_path.clone();
		let (event_tx, event_rx) = mpsc::channel();
		let worker = thread::spawn(move || {
			run(
				&worker_config,
				LoginMethod::BrowserRedirect,
				&worker_home,
				&handle,
				&worker_cancellation,
				|event| {
					let _ = event_tx.send(event);
				},
			)
		});
		let authorization_url = match event_rx
			.recv_timeout(Duration::from_secs(2))
			.expect("browser authorization event")
		{
			LoginEvent::BrowserAuthorization { authorization_url } => authorization_url,
			LoginEvent::DeviceAuthorization { .. } => panic!("unexpected device event"),
		};
		let authorization_url = Url::parse(&authorization_url).expect("authorization URL");
		let parameters = authorization_url.query_pairs().into_owned().collect::<HashMap<_, _>>();
		let mut callback =
			Url::parse(parameters.get("redirect_uri").expect("redirect URI")).expect("callback");
		callback
			.query_pairs_mut()
			.append_pair("code", "fixture-browser-authorization")
			.append_pair("state", "wrong-state");
		let response =
			reqwest::blocking::Client::new().get(callback).send().expect("mismatch response");
		assert_eq!(response.status().as_u16(), 400);
		cancellation.cancel();
		let result = worker.join().expect("browser adapter worker");

		assert!(matches!(result, Err(Error::Cancelled)));
		assert!(!home_path.join("auth.json").exists());
	}

	#[test]
	fn browser_timeout_and_device_cancellation_are_closed_and_leave_no_auth_file() {
		let runtime = runtime();
		let timeout_config = Config::test(
			Url::parse("http://127.0.0.1:9/").expect("fixture issuer"),
			0,
			Duration::from_millis(80),
		)
		.expect("timeout config");
		let (_timeout_home, timeout_home_path) = canonical_temp_home();
		let timeout = run(
			&timeout_config,
			LoginMethod::BrowserRedirect,
			&timeout_home_path,
			runtime.handle(),
			&Cancellation::default(),
			|_| {},
		);
		assert!(matches!(timeout, Err(Error::TimedOut)));
		assert!(!timeout_home_path.join("auth.json").exists());

		let issuer = device_issuer(true);
		let config =
			Config::test(issuer.issuer.clone(), 0, Duration::from_secs(5)).expect("device config");
		let (_cancel_home, cancel_home_path) = canonical_temp_home();
		let handle = runtime.handle().clone();
		let cancellation = Cancellation::default();
		let worker_cancellation = cancellation.clone();
		let worker_home = cancel_home_path.clone();
		let (event_tx, event_rx) = mpsc::channel();
		let worker = thread::spawn(move || {
			run(
				&config,
				LoginMethod::DeviceCode,
				&worker_home,
				&handle,
				&worker_cancellation,
				|event| {
					let _ = event_tx.send(event);
				},
			)
		});
		assert!(matches!(
			event_rx.recv_timeout(Duration::from_secs(2)),
			Ok(LoginEvent::DeviceAuthorization { .. })
		));
		cancellation.cancel();
		let cancelled = worker.join().expect("device adapter worker");
		assert!(matches!(cancelled, Err(Error::Cancelled)));
		assert!(!cancel_home_path.join("auth.json").exists());
	}

	#[test]
	fn oversized_provider_response_is_rejected_before_deserialization() {
		let issuer = MockIssuer::start(|request| {
			assert_eq!(request.target, "/api/accounts/deviceauth/usercode");
			(200, "x".repeat(MAX_RESPONSE_BYTES + 1))
		});
		let config =
			Config::test(issuer.issuer.clone(), 0, Duration::from_secs(2)).expect("device config");
		let (_home, home_path) = canonical_temp_home();
		let runtime = runtime();
		let result = run(
			&config,
			LoginMethod::DeviceCode,
			&home_path,
			runtime.handle(),
			&Cancellation::default(),
			|_| {},
		);

		assert!(matches!(result, Err(Error::InvalidResponse)));
		assert!(!home_path.join("auth.json").exists());
	}

	#[test]
	fn callback_parser_rejects_a_full_buffer_without_a_header_terminator() {
		let bytes = vec![b'A'; MAX_CALLBACK_REQUEST_BYTES];

		assert!(matches!(parse_callback_head(&bytes), Err(CallbackParseFailure::TooLarge)));
	}

	#[test]
	fn callback_parser_rejects_more_than_the_fixed_header_array() {
		let mut bytes = b"GET /auth/callback?code=x&state=y HTTP/1.1\r\n".to_vec();
		for _ in 0..=MAX_CALLBACK_HEADERS {
			bytes.extend_from_slice(b"X-Fixture: value\r\n");
		}
		bytes.extend_from_slice(b"\r\n");

		assert!(matches!(parse_callback_head(&bytes), Err(CallbackParseFailure::TooManyHeaders)));
	}
}
