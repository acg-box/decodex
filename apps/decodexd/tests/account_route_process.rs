//! Process-level account Route acceptance over the production daemon composition root.

#![cfg(all(target_os = "macos", feature = "process-acceptance-fixture", debug_assertions))]
#![allow(unused_crate_dependencies)]

use std::{
	collections::VecDeque,
	fs,
	io::{BufRead as _, BufReader, Read as _, Write as _},
	net::{TcpListener, TcpStream},
	os::unix::fs::{MetadataExt as _, PermissionsExt as _},
	path::{Path, PathBuf},
	process::{Child, Command, Stdio},
	sync::{
		Arc, Mutex,
		atomic::{AtomicBool, Ordering},
		mpsc,
	},
	thread::{self, JoinHandle},
	time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use decodex_core::{AccountId, AccountSelectionMode, DecodexRoot};
use decodex_database::SqliteStore;
use decodex_protocol::{
	AccountClient, AccountCommandResponse, AccountRouteWaitReasonDto, AccountsResult,
	ClientProfile, CodexAuthProjectionResult, CommandPayload, EntityId, EntityRevision,
	IdempotencyKey, ResultPayload, WireText,
};
use decodex_runtime::{HostCredentialStore as _, SqliteCredentialStore};
use serde_json::{Value, json};
use tempfile::{TempDir, tempdir_in};
use tokio::time;

const READY_LINE: &str = "decodexd serving WebSocket /v1/ws over same-UID local transport";
const ACCOUNT_A: &str = "21000000-0000-4000-8000-0000000000a1";
const ACCOUNT_B: &str = "21000000-0000-4000-8000-0000000000b1";
const PROVIDER_A: &str = "process-fixture-provider-a";
const PROVIDER_B: &str = "process-fixture-provider-b";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_daemon_routes_a_b_a_restarts_and_routes_again_with_exact_readback() {
	let fixture = Fixture::new();
	let initial_a = fixture.tokens(PROVIDER_A, "a-initial");
	let initial_b = fixture.tokens(PROVIDER_B, "b-initial");
	let routed_b = fixture.tokens(PROVIDER_B, "b-routed");
	let routed_a = fixture.tokens(PROVIDER_A, "a-routed");
	let restarted_b = fixture.tokens(PROVIDER_B, "b-restarted");
	fixture.write_shared_auth(&initial_a);
	let import_a = fixture.write_import("account-a.json", &initial_a);
	let import_b = fixture.write_import("account-b.json", &initial_b);
	let refresh = RefreshServer::start(vec![
		(initial_b.refresh_token.clone(), routed_b.clone()),
		(initial_a.refresh_token.clone(), routed_a.clone()),
		(routed_b.refresh_token.clone(), restarted_b.clone()),
	]);
	let mut first = RunningDaemon::start(fixture.home(), refresh.endpoint());
	let client = fixture.client();

	import_account(
		&client,
		ACCOUNT_A,
		"22000000-0000-4000-8000-0000000000a1",
		"process-import-a",
		&import_a,
	)
	.await;
	import_account(
		&client,
		ACCOUNT_B,
		"22000000-0000-4000-8000-0000000000b1",
		"process-import-b",
		&import_b,
	)
	.await;
	wait_for_projection(&client, ACCOUNT_A).await;
	let routed = route_account(
		&client,
		ACCOUNT_A,
		"22000000-0000-4000-8000-0000000000a0",
		"process-route-initial-a",
	)
	.await;
	fixture.assert_exact_readback(ACCOUNT_A, PROVIDER_A, &initial_a, routed).await;

	let routed = route_account(
		&client,
		ACCOUNT_B,
		"22000000-0000-4000-8000-0000000000b2",
		"process-route-b",
	)
	.await;
	fixture.assert_exact_readback(ACCOUNT_B, PROVIDER_B, &routed_b, routed).await;

	let routed = route_account(
		&client,
		ACCOUNT_A,
		"22000000-0000-4000-8000-0000000000a2",
		"process-route-a",
	)
	.await;
	fixture.assert_exact_readback(ACCOUNT_A, PROVIDER_A, &routed_a, routed).await;
	let first_output = first.stop();

	let mut restarted = RunningDaemon::start(fixture.home(), refresh.endpoint());
	let restarted_client = fixture.client();
	let routed = route_account(
		&restarted_client,
		ACCOUNT_B,
		"22000000-0000-4000-8000-0000000000b3",
		"process-route-b-after-restart",
	)
	.await;
	fixture.assert_exact_readback(ACCOUNT_B, PROVIDER_B, &restarted_b, routed).await;
	let restarted_output = restarted.stop();

	refresh.finish();
	let secrets = [initial_a, initial_b, routed_b, routed_a, restarted_b]
		.into_iter()
		.flat_map(TokenFixture::secret_values)
		.collect::<Vec<_>>();
	assert_no_credentials(&first_output, &secrets);
	assert_no_credentials(&restarted_output, &secrets);
}

struct Fixture {
	_home: TempDir,
	home: PathBuf,
	root: DecodexRoot,
	expires_at_seconds: i64,
}

impl Fixture {
	fn new() -> Self {
		let parent = PathBuf::from(std::env::var_os("HOME").expect("read process fixture parent"));
		let temporary = tempdir_in(parent).expect("create owner-private process fixture");
		let home = temporary.path().canonicalize().expect("canonicalize process fixture");
		let root = DecodexRoot::from_home(&home).expect("type process fixture root");
		let paths = root.paths();
		fs::create_dir(paths.root().as_path()).expect("create process fixture root");
		fs::create_dir(paths.server_dir()).expect("create process fixture server directory");
		fs::create_dir(home.join(".codex")).expect("create process fixture Codex directory");
		for directory in
			[paths.root().as_path(), paths.server_dir().as_path(), &home.join(".codex")]
		{
			fs::set_permissions(directory, fs::Permissions::from_mode(0o700))
				.expect("scope process fixture directory");
		}
		fs::write(
			paths.config_file(),
			format!(
				r#"version = 1
active_profile = "local"

[profiles.local]
kind = "local"
policy = "same_uid"
service_owner_uid = {}

[cache]
max_entries = 16
max_bytes = 1048576
max_entry_bytes = 65536
"#,
				fs::metadata(paths.root().as_path()).expect("read fixture root metadata").uid(),
			),
		)
		.expect("write process fixture config");
		fs::set_permissions(paths.config_file(), fs::Permissions::from_mode(0o600))
			.expect("scope process fixture config");
		let expires_at_seconds = SystemTime::now()
			.duration_since(UNIX_EPOCH)
			.expect("fixture clock after epoch")
			.as_secs()
			.saturating_add(86_400)
			.try_into()
			.expect("fixture expiry fits i64");
		Self { _home: temporary, home, root, expires_at_seconds }
	}

	fn home(&self) -> &Path {
		&self.home
	}

	fn client(&self) -> AccountClient {
		let profile = ClientProfile::load(self.root.as_path(), None)
			.expect("load isolated daemon client profile");
		AccountClient::new(profile)
	}

	fn tokens(&self, provider: &str, suffix: &str) -> TokenFixture {
		TokenFixture {
			provider: provider.to_owned(),
			email: format!("{suffix}@example.test"),
			expires_at_micros: self
				.expires_at_seconds
				.checked_mul(1_000_000)
				.expect("fixture expiry micros fit i64"),
			access_token: jwt(json!({"exp": self.expires_at_seconds, "fixture": suffix})),
			refresh_token: format!("process-fixture-refresh-{suffix}"),
			id_token: jwt(json!({
				"email": format!("{suffix}@example.test"),
				"https://api.openai.com/auth": {
					"chatgpt_account_id": provider,
					"chatgpt_plan_type": "pro"
				}
			})),
		}
	}

	fn write_shared_auth(&self, tokens: &TokenFixture) {
		let path = self.home.join(".codex/auth.json");
		write_private_json(&path, &tokens.shared_auth());
	}

	fn write_import(&self, name: &str, tokens: &TokenFixture) -> PathBuf {
		let path = self.home.join(name);
		write_private_json(&path, &tokens.versioned_import());
		path
	}

	async fn assert_exact_readback(
		&self,
		account: &str,
		provider: &str,
		tokens: &TokenFixture,
		expected_account_revision: EntityRevision,
	) {
		let store = SqliteStore::open(&self.root.paths()).expect("open exact readback store");
		let account_id = AccountId::new(account).expect("type fixture account ID");
		let rows = store
			.read_account_registry(Some(&account_id), 1)
			.await
			.expect("read exact account row");
		assert!(rows.len() == 1, "exact account row must exist");
		let row = &rows[0];
		assert!(
			row.revision == i64::try_from(expected_account_revision.0).expect("revision fits i64"),
			"SQLite account revision must match the terminal Route result"
		);
		let binding = row.credential.as_ref().expect("routed account must retain a binding");
		assert!(
			binding.provider.account_id() == provider,
			"SQLite provider identity must be exact"
		);
		let stored = SqliteCredentialStore::new(store.clone())
			.read_exact(&account_id, binding)
			.expect("read exact SQLite credential binding");
		assert!(
			stored.bundle().access_token() == tokens.access_token,
			"SQLite access credential must match the scripted successor"
		);
		assert!(
			stored.bundle().refresh_token() == tokens.refresh_token,
			"SQLite refresh credential must match the scripted successor"
		);
		assert!(
			stored.bundle().id_token() == Some(tokens.id_token.as_str()),
			"SQLite identity credential must match the scripted successor"
		);
		let routing =
			store.read_account_routing_control().await.expect("read exact SQLite routing control");
		assert!(
			matches!(routing.mode, AccountSelectionMode::Fixed(ref selected) if selected == &account_id),
			"SQLite routing control must select the exact account"
		);
		let auth: Value = serde_json::from_slice(
			&fs::read(self.home.join(".codex/auth.json")).expect("read isolated shared auth"),
		)
		.expect("decode isolated shared auth");
		assert!(
			auth.pointer("/tokens/account_id").and_then(Value::as_str) == Some(provider),
			"shared-auth provider identity must be exact"
		);
		assert!(
			auth.pointer("/tokens/access_token").and_then(Value::as_str)
				== Some(tokens.access_token.as_str()),
			"shared-auth access credential must match SQLite"
		);
		assert!(
			auth.pointer("/tokens/refresh_token").and_then(Value::as_str)
				== Some(tokens.refresh_token.as_str()),
			"shared-auth refresh credential must match SQLite"
		);
		assert!(
			auth.pointer("/tokens/id_token").and_then(Value::as_str)
				== Some(tokens.id_token.as_str()),
			"shared-auth identity credential must match SQLite"
		);
	}
}

#[derive(Clone)]
struct TokenFixture {
	provider: String,
	email: String,
	expires_at_micros: i64,
	access_token: String,
	refresh_token: String,
	id_token: String,
}

impl TokenFixture {
	fn shared_auth(&self) -> Value {
		json!({
			"auth_mode": "chatgpt",
			"OPENAI_API_KEY": null,
			"tokens": {
				"id_token": self.id_token,
				"access_token": self.access_token,
				"refresh_token": self.refresh_token,
				"account_id": self.provider,
			},
			"last_refresh": "2026-01-01T00:00:00Z",
		})
	}

	fn versioned_import(&self) -> Value {
		json!({
			"schema": "decodex/account-credential-import/1",
			"provider": "chatgpt",
			"provider_account_id": self.provider,
			"provider_email": self.email,
			"access_token": self.access_token,
			"refresh_token": self.refresh_token,
			"id_token": self.id_token,
			"plan_type": "pro",
			"token_type": "bearer",
			"access_token_expires_at_unix_micros": self.expires_at_micros,
		})
	}

	fn refresh_response(&self) -> Value {
		json!({
			"id_token": self.id_token,
			"access_token": self.access_token,
			"refresh_token": self.refresh_token,
			"token_type": "bearer",
			"expires_in": 86400,
		})
	}

	fn secret_values(self) -> [String; 3] {
		[self.access_token, self.refresh_token, self.id_token]
	}
}

async fn import_account(
	client: &AccountClient,
	account: &str,
	operation: &str,
	key: &str,
	source: &Path,
) {
	let response = client
		.execute(
			CommandPayload::ImportAccountCredentialFile {
				operation_id: entity(operation),
				account_id: entity(account),
				enabled: true,
				source_descriptor: WireText::new(source.to_string_lossy().into_owned())
					.expect("bound fixture source path"),
			},
			None,
			idempotency(key),
		)
		.await
		.expect("dispatch account import");
	assert!(
		matches!(response, AccountCommandResponse::Applied { result, .. } if matches!(*result, ResultPayload::AccountChanged { .. })),
		"account import must commit"
	);
}

async fn route_account(
	client: &AccountClient,
	account: &str,
	operation: &str,
	key: &str,
) -> EntityRevision {
	let (account_revision, routing_revision) = account_revisions(client, account).await;
	let deadline = time::Instant::now() + Duration::from_secs(15);
	let mut wait_reason = "none";
	loop {
		let response = client
			.route_account(
				entity(operation),
				entity(account),
				account_revision,
				routing_revision,
				idempotency(key),
			)
			.await
			.expect("dispatch RouteAccount");
		match response {
			AccountCommandResponse::Applied { result, .. } => match *result {
				ResultPayload::AccountRouted { account: routed, routing, .. } => {
					assert!(
						routed.account_id.as_str() == account,
						"Route result account must be exact"
					);
					assert!(
						matches!(routing.mode, decodex_protocol::AccountSelectionModeDto::Fixed(ref selected) if selected.as_str() == account),
						"Route result routing mode must be exact"
					);
					return routed.account_revision;
				},
				ResultPayload::AccountRoutePending { pending } => {
					wait_reason = match pending.wait_reason {
						AccountRouteWaitReasonDto::ExternalCodex { .. } => "external_codex",
						AccountRouteWaitReasonDto::CodexObservationUnavailable => {
							"codex_observation_unavailable"
						},
						AccountRouteWaitReasonDto::AccountReadiness { .. } => "account_readiness",
						AccountRouteWaitReasonDto::SharedAuthStabilizing => {
							"shared_auth_stabilizing"
						},
						AccountRouteWaitReasonDto::SharedAuthUnavailable => {
							"shared_auth_unavailable"
						},
						AccountRouteWaitReasonDto::ProjectionReadback => "projection_readback",
					};
				},
				_ => panic!("RouteAccount returned an unrelated credential-negative result"),
			},
			AccountCommandResponse::Rejected { .. } => {
				panic!("RouteAccount was rejected without credential output")
			},
			AccountCommandResponse::PotentiallyDispatched { .. } => {},
		}
		assert!(
			time::Instant::now() < deadline,
			"RouteAccount {key} did not settle before deadline: {wait_reason}"
		);
		time::sleep(Duration::from_millis(100)).await;
	}
}

async fn account_revisions(
	client: &AccountClient,
	account: &str,
) -> (EntityRevision, EntityRevision) {
	let AccountsResult::Available { accounts, routing: Some(routing), .. } =
		client.list().await.expect("list exact account state")
	else {
		panic!("account state must be available");
	};
	let account_revision = accounts
		.iter()
		.find(|candidate| candidate.account_id.as_str() == account)
		.map(|candidate| candidate.account_revision)
		.expect("target account must exist");
	(account_revision, routing.revision)
}

async fn wait_for_projection(client: &AccountClient, account: &str) {
	let deadline = time::Instant::now() + Duration::from_secs(5);
	loop {
		if matches!(
			client.codex_auth_projection().await.expect("read shared-auth projection"),
			CodexAuthProjectionResult::Current { account_id, .. } if account_id.as_str() == account
		) {
			return;
		}
		assert!(time::Instant::now() < deadline, "shared auth did not become stable");
		time::sleep(Duration::from_millis(100)).await;
	}
}

struct RefreshServer {
	endpoint: String,
	stop: Arc<AtomicBool>,
	thread: JoinHandle<()>,
}

impl RefreshServer {
	fn start(script: Vec<(String, TokenFixture)>) -> Self {
		let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback refresh fixture");
		listener.set_nonblocking(true).expect("make refresh fixture nonblocking");
		let endpoint = format!(
			"http://127.0.0.1:{}/oauth/token",
			listener.local_addr().expect("read refresh fixture address").port()
		);
		let stop = Arc::new(AtomicBool::new(false));
		let thread_stop = Arc::clone(&stop);
		let thread = thread::spawn(move || {
			let mut script = VecDeque::from(script);
			while !thread_stop.load(Ordering::Acquire) || !script.is_empty() {
				match listener.accept() {
					Ok((stream, _)) => serve_refresh(stream, &mut script),
					Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
						thread::sleep(Duration::from_millis(10));
					},
					Err(_) => panic!("loopback refresh fixture failed"),
				}
			}
			assert!(script.is_empty(), "all scripted refreshes must be consumed");
		});
		Self { endpoint, stop, thread }
	}

	fn endpoint(&self) -> &str {
		&self.endpoint
	}

	fn finish(self) {
		self.stop.store(true, Ordering::Release);
		self.thread.join().expect("join loopback refresh fixture");
	}
}

fn serve_refresh(mut stream: TcpStream, script: &mut VecDeque<(String, TokenFixture)>) {
	let clone = stream.try_clone().expect("clone refresh fixture stream");
	let mut reader = BufReader::new(clone);
	let mut content_length = None;
	let mut first = true;
	loop {
		let mut line = String::new();
		reader.read_line(&mut line).expect("read refresh request header");
		assert!(!line.is_empty(), "refresh request headers must be complete");
		if first {
			assert!(line.starts_with("POST /oauth/token "), "refresh request path must be exact");
			first = false;
		}
		if line == "\r\n" {
			break;
		}
		if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
			content_length = value.trim().parse::<usize>().ok();
		}
	}
	let mut body = vec![0_u8; content_length.expect("refresh request content length")];
	reader.read_exact(&mut body).expect("read refresh request body");
	let request: Value = serde_json::from_slice(&body).expect("decode refresh request");
	let (expected_refresh, response) = script.pop_front().expect("unexpected refresh request");
	assert!(
		request.get("refresh_token").and_then(Value::as_str) == Some(expected_refresh.as_str()),
		"refresh request must use the exact expected credential"
	);
	let body = serde_json::to_vec(&response.refresh_response()).expect("encode refresh response");
	write!(
		stream,
		"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
		body.len()
	)
	.expect("write refresh response headers");
	stream.write_all(&body).expect("write refresh response body");
}

struct RunningDaemon {
	child: Child,
	stdout: Arc<Mutex<Vec<u8>>>,
	stderr: Arc<Mutex<Vec<u8>>>,
	stdout_reader: JoinHandle<()>,
	stderr_reader: JoinHandle<()>,
}

impl RunningDaemon {
	fn start(home: &Path, refresh_endpoint: &str) -> Self {
		let mut child = Command::new(env!("CARGO_BIN_EXE_decodexd"))
			.env("HOME", home)
			.env("DECODEX_PROCESS_TEST_REFRESH_ENDPOINT", refresh_endpoint)
			.stdout(Stdio::piped())
			.stderr(Stdio::piped())
			.spawn()
			.expect("start actual daemon process");
		let stdout = Arc::new(Mutex::new(Vec::new()));
		let stderr = Arc::new(Mutex::new(Vec::new()));
		let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
		let stdout_pipe = child.stdout.take().expect("capture daemon stdout");
		let stdout_bytes = Arc::clone(&stdout);
		let stdout_reader = thread::spawn(move || {
			for line in BufReader::new(stdout_pipe).lines() {
				let line = line.expect("read daemon stdout");
				stdout_bytes.lock().expect("lock daemon stdout").extend_from_slice(line.as_bytes());
				stdout_bytes.lock().expect("lock daemon stdout").push(b'\n');
				if line == READY_LINE {
					let _ = ready_sender.send(());
				}
			}
		});
		let stderr_pipe = child.stderr.take().expect("capture daemon stderr");
		let stderr_bytes = Arc::clone(&stderr);
		let stderr_reader = thread::spawn(move || {
			let mut reader = BufReader::new(stderr_pipe);
			reader
				.read_to_end(&mut stderr_bytes.lock().expect("lock daemon stderr"))
				.expect("read daemon stderr");
		});
		if ready_receiver.recv_timeout(Duration::from_secs(30)).is_err() {
			let _ = child.kill();
			let _ = child.wait();
			panic!("actual daemon did not become ready");
		}
		Self { child, stdout, stderr, stdout_reader, stderr_reader }
	}

	fn stop(&mut self) -> Vec<u8> {
		assert!(
			unsafe { libc::kill(self.child.id() as libc::pid_t, libc::SIGTERM) } == 0,
			"signal actual daemon"
		);
		let deadline = Instant::now() + Duration::from_secs(20);
		let status = loop {
			if let Some(status) = self.child.try_wait().expect("poll daemon exit") {
				break status;
			}
			assert!(Instant::now() < deadline, "actual daemon did not stop before deadline");
			thread::sleep(Duration::from_millis(20));
		};
		assert!(status.success(), "actual daemon must stop cleanly");
		let stdout_reader = std::mem::replace(&mut self.stdout_reader, thread::spawn(|| {}));
		let stderr_reader = std::mem::replace(&mut self.stderr_reader, thread::spawn(|| {}));
		stdout_reader.join().expect("join daemon stdout reader");
		stderr_reader.join().expect("join daemon stderr reader");
		let mut output = self.stdout.lock().expect("lock final daemon stdout").clone();
		output.extend_from_slice(&self.stderr.lock().expect("lock final daemon stderr"));
		output
	}
}

impl Drop for RunningDaemon {
	fn drop(&mut self) {
		if self.child.try_wait().ok().flatten().is_none() {
			let _ = self.child.kill();
			let _ = self.child.wait();
		}
	}
}

fn assert_no_credentials(output: &[u8], secrets: &[String]) {
	assert!(
		secrets.iter().all(|secret| !contains_bytes(output, secret.as_bytes())),
		"daemon output must remain credential-negative"
	);
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
	!needle.is_empty() && haystack.windows(needle.len()).any(|window| window == needle)
}

fn write_private_json(path: &Path, value: &Value) {
	fs::write(path, serde_json::to_vec(value).expect("encode private fixture"))
		.expect("write private fixture");
	fs::set_permissions(path, fs::Permissions::from_mode(0o600)).expect("scope private fixture");
}

fn jwt(payload: Value) -> String {
	let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"none","typ":"JWT"}"#);
	let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload).expect("encode JWT fixture"));
	format!("{header}.{payload}.fixture")
}

fn entity(value: &str) -> EntityId {
	EntityId::new(value).expect("canonical fixture entity ID")
}

fn idempotency(value: &str) -> IdempotencyKey {
	IdempotencyKey::new(value).expect("bounded fixture idempotency key")
}
