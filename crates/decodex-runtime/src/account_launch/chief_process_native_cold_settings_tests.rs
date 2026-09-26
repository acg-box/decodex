//! Isolated account-bound runtime qualification; never use the real user's home.
use super::*;
#[path = "chief_process_native_recap_socket_tests.rs"] mod recap_socket;
#[path = "chief_process_native_runtime_submit_tests.rs"] mod submit;
use crate::{
	account_service::{
		AccountService, CredentialRefreshError, CredentialRefreshPort, CredentialRefreshResult,
	},
	conversation::ConversationRuntime,
	host_credentials::{CredentialSecretBundle, SqliteCredentialStore},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use decodex_core::{
	AccountId, AccountOperationId, BlobStore, DecodexRoot, ProcessExecutionAuthorization,
	ProcessExecutionEpochId,
};
use decodex_database::{
	AccountCommandKind, AccountCommandReceiptClaim, CommandIdentity, SqliteStore,
};

struct NoRefresh;
impl CredentialRefreshPort for NoRefresh {
	fn refresh(
		&self,
		_: &CredentialSecretBundle,
	) -> Result<CredentialRefreshResult, CredentialRefreshError> {
		panic!("fresh synthetic credentials must not refresh");
	}
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "run alone with isolated HOME, DECODEX_TEST_ACCOUNT_HOME and DECODEX_TEST_CODEX_BINARY"]
async fn installed_cold_runtime_settings_read_and_submit_without_replaying() {
	let home = std::path::PathBuf::from(
		std::env::var_os("DECODEX_TEST_ACCOUNT_HOME").expect("isolated home"),
	)
	.canonicalize()
	.expect("isolated fixture home");
	assert_eq!(
		std::path::PathBuf::from(std::env::var_os("HOME").expect("isolated fixture home")),
		home
	);
	assert_eq!(
		std::fs::read_to_string(home.join(".decodex-cold-settings-fixture"))
			.expect("isolated fixture home"),
		"isolated-native-settings\n"
	);
	assert!(!home.join(".codex").exists());
	tokio::time::timeout(Duration::from_secs(90), qualify(&home))
		.await
		.expect("bounded native cold runtime fixture");
}

async fn qualify(home: &std::path::Path) {
	let native_home = home.join(".codex");
	std::fs::create_dir(&native_home).expect("native cold-read qualification");
	let binary =
		std::env::var_os("DECODEX_TEST_CODEX_BINARY").expect("native cold-read qualification");
	let listener =
		tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("native cold-read qualification");
	let address = listener.local_addr().expect("native cold-read qualification");
	let requests = Arc::new(std::sync::atomic::AtomicUsize::new(0));
	let metadata_requests = Arc::new(std::sync::atomic::AtomicUsize::new(0));
	let backend = tokio::spawn(serve(listener, requests.clone(), metadata_requests.clone()));
	let catalog = native_home.join("models.json");
	std::fs::write(
		&catalog,
		serde_json::to_vec(
			&json!({"models":[effort::fixture_model("cold-native-model", "provider-effort")]}),
		)
		.expect("native cold-read qualification"),
	)
	.expect("native cold-read qualification");
	std::fs::write(native_home.join("config.toml"), format!("model=\"cold-native-model\"\nmodel_reasoning_effort=\"provider-effort\"\nmodel_catalog_json={}\nmodel_provider=\"fixture\"\nchatgpt_base_url=\"http://{address}/backend-api\"\ncli_auth_credentials_store=\"file\"\n[features]\nenable_request_compression=false\napps=false\nremote_plugins=false\n[analytics]\nenabled=false\n[model_providers.fixture]\nname=\"Isolated fixture\"\nbase_url=\"http://{address}\"\nwire_api=\"responses\"\nrequires_openai_auth=false\nsupports_websockets=false\n", json!(catalog))).expect("native cold-read qualification");
	let mut native = NativeSession::start(&binary, &native_home);
	let started = native
		.client
		.thread_start(json!({"cwd":home,"approvalPolicy":"never","sandbox":"read-only"}))
		.await
		.expect("native cold-read qualification");
	let thread =
		started["thread"]["id"].as_str().expect("native cold-read qualification").to_owned();
	let turn = native
		.client
		.turn_start(
			json!({"threadId":thread,"input":[{"type":"text","text":"Save fixture input"}]}),
		)
		.await
		.expect("native cold-read qualification");
	loop {
		if matches!(native.events.recv().await.expect("native cold-read qualification"), ServerEvent::Notification { method, params } if method == "turn/completed" && params["threadId"] == thread && params["turn"]["id"] == turn["turn"]["id"])
		{
			break;
		}
	}
	drop(native);
	// The saved fixture turn did not need authentication. The production reader
	// uses the enrolled ChatGPT account and must admit native token projection.
	let config = native_home.join("config.toml");
	let text = std::fs::read_to_string(&config).expect("native cold-read qualification");
	std::fs::write(config, text.replace("requires_openai_auth=false", "requires_openai_auth=true"))
		.expect("native cold-read qualification");
	assert_eq!(requests.load(Ordering::Acquire), 1);
	let root = DecodexRoot::new(home.join("product")).expect("native cold-read qualification");
	root.paths().ensure_layout().expect("native cold-read qualification");
	let store = SqliteStore::open(&root.paths()).expect("native cold-read qualification");
	let accounts = Arc::new(AccountService::new(
		store.clone(),
		Arc::new(SqliteCredentialStore::new(store.clone())),
		Arc::new(NoRefresh),
	));
	let account = enroll(&store, &accounts, home).await;
	let directory = home.to_owned();
	let profile = tokio::task::spawn_blocking(move || {
		crate::account_launch::process::AttestedAppServerProfile::attest(
			directory,
			Duration::from_secs(15),
		)
	})
	.await
	.expect("native cold-read qualification")
	.expect("production native profile attestation");
	accounts
		.attest_callback_capability(profile.account_callback_attestation())
		.await
		.expect("native cold-read qualification");
	seed(&root, &account, &thread, home);
	let runtime = runtime(&root, &store, accounts.clone(), profile.clone()).await;
	let id = decodex_core::ConversationId::new("44000000-0000-4000-8000-000000000001")
		.expect("native cold-read qualification");
	let before = store
		.read_ordinary_runtime_session_for_resume(&id)
		.await
		.expect("native cold-read qualification");
	let routing =
		store.read_account_routing_control().await.expect("native cold-read qualification");
	let result = runtime.model_settings("cold-model-query", id.as_str()).await;
	assert!(
		matches!(result, decodex_protocol::ConversationModelSettingsResult::Available { model: Some(ref model), reasoning_effort: Some(ref effort), requested_service_tier: None, .. } if model.as_str() == "cold-native-model" && effort.as_str() == "provider-effort"),
		"cold owned settings read: {result:?}"
	);
	assert_eq!(
		before,
		store
			.read_ordinary_runtime_session_for_resume(&id)
			.await
			.expect("native cold-read qualification")
	);
	assert_eq!(
		routing,
		store.read_account_routing_control().await.expect("native cold-read qualification")
	);
	assert_eq!(requests.load(Ordering::Acquire), 1, "metadata read cannot infer or replay");
	submit::qualify(&runtime, &store, home).await;
	assert_eq!(
		requests.load(Ordering::Acquire),
		3,
		"one original fixture and two explicit runtime turns"
	);
	runtime.begin_shutdown();
	runtime.wait_for_shutdown().await;
	drop(runtime);
	drop(accounts);
	drop(store);
	restart_and_submit(&root, profile, home).await;
	assert_eq!(requests.load(Ordering::Acquire), 4, "restart sends exactly one new turn");
	assert!(!backend.is_finished());
	eprintln!(
		"native fixture: {} metadata requests, {} inference requests",
		metadata_requests.load(Ordering::Acquire),
		requests.load(Ordering::Acquire)
	);
	backend.abort();
}

async fn restart_and_submit(
	root: &DecodexRoot,
	profile: crate::account_launch::AttestedAppServerProfile,
	home: &std::path::Path,
) {
	let store = SqliteStore::open(&root.paths()).expect("reopen product store");
	let accounts = Arc::new(AccountService::new(
		store.clone(),
		Arc::new(SqliteCredentialStore::new(store.clone())),
		Arc::new(NoRefresh),
	));
	accounts
		.attest_callback_capability(profile.account_callback_attestation())
		.await
		.expect("restore account callback capability");
	let restarted = runtime(root, &store, accounts, profile).await;
	submit::assert_warning_history(&restarted, &store, home).await;
	submit::submit_inherited(
		&restarted,
		&store,
		home,
		"native-runtime-restarted",
		"62000000-0000-4000-8000-000000000002",
	)
	.await;
	submit::archive(&restarted, &store, home).await;
	restarted.begin_shutdown();
	restarted.wait_for_shutdown().await;
}

async fn enroll(
	store: &SqliteStore,
	service: &AccountService,
	home: &std::path::Path,
) -> AccountId {
	use std::{io::Write as _, os::unix::fs::OpenOptionsExt as _};
	let account = AccountId::new("10000000-0000-4000-8000-000000000001")
		.expect("synthetic account enrollment");
	let claims = json!({"email":"fixture@example.test","exp":4102444800_u64,"https://api.openai.com/auth":{"chatgpt_account_id":"workspace-fixture","chatgpt_user_id":"user-fixture","chatgpt_plan_type":"team"}});
	let token = format!(
		"{}.{}.fixture-signature",
		URL_SAFE_NO_PAD.encode(r#"{"alg":"none"}"#),
		URL_SAFE_NO_PAD.encode(claims.to_string())
	);
	let value = json!({"auth_mode":"chatgpt","tokens":{"access_token":token,"id_token":token,"refresh_token":"fixture-only","account_id":"workspace-fixture"},"last_refresh":"2026-09-24T00:00:00Z"});
	let path = home.join("synthetic-credential.json");
	let mut file = std::fs::OpenOptions::new()
		.write(true)
		.create_new(true)
		.mode(0o600)
		.open(&path)
		.expect("synthetic account enrollment");
	file.write_all(value.to_string().as_bytes()).expect("synthetic account enrollment");
	drop(file);
	let identity = CommandIdentity::new("cold-fixture-enroll", b"synthetic enrollment")
		.expect("synthetic account enrollment");
	let AccountCommandReceiptClaim::Owned(lease) = store
		.reserve_account_command(&identity, AccountCommandKind::Enroll, account.as_str(), None)
		.await
		.expect("synthetic account enrollment")
	else {
		panic!("enrollment owner")
	};
	service
		.enroll_from_credential_file_command(
			lease,
			AccountOperationId::new("20000000-0000-4000-8000-000000000001")
				.expect("synthetic account enrollment"),
			account.clone(),
			true,
			path.to_str().expect("synthetic account enrollment"),
			|result| {
				assert!(result.is_ok());
				Ok(json!({"enrolled":true}))
			},
		)
		.await
		.expect("synthetic account enrollment");
	std::fs::remove_file(path).expect("synthetic account enrollment");
	account
}

fn seed(root: &DecodexRoot, account: &AccountId, thread: &str, directory: &std::path::Path) {
	let source = include_str!("../../tests/fixtures/opaque_resume_authority.sql");
	let operations = &source[source
		.find("INSERT INTO account_operations")
		.expect("settled conversation history fixture")
		..source.find("INSERT INTO accounts").expect("settled conversation history fixture")];
	let facts = &source
		[source.find("INSERT INTO conversations").expect("settled conversation history fixture")..];
	let sql = format!("PRAGMA foreign_keys=ON;\n{operations}{facts}")
		.replace("46000000-0000-4000-8000-000000000001", account.as_str())
		.replace("opaque-restart-provider", "workspace-fixture")
		.replace("sha256:fixture", &format!("sha256:{}", "a".repeat(64)))
		.replace("provider/thread?after#restart%opaque", thread);
	let connection = rusqlite::Connection::open(root.paths().product_database_file())
		.expect("settled conversation history fixture");
	connection.execute_batch(&sql).expect("settled conversation history fixture");
	// The initial native process has exited. Seed settled historical ownership,
	// not a fictitious live process for the supervisor to recover.
	connection
		.execute_batch(
			"INSERT INTO process_generation_death_evidence
		(evidence_id,generation_id,kind,observed_boot_id,bound_boot_id,process_id,
		process_start_id,process_group_id,session_id,witness_sha256,observed_at_micros)
		SELECT '51000000-0000-4000-8000-000000000001',generation_id,'owned_child_exit',
		bound_boot_id,bound_boot_id,process_id,process_start_id,process_group_id,session_id,
		'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',2
		FROM process_generations;
		UPDATE process_generations SET state='dead',
		death_evidence_id='51000000-0000-4000-8000-000000000001',revision=4,updated_at_micros=2;",
		)
		.expect("settled conversation history fixture");
	connection.execute("INSERT INTO quick_task_requests(conversation_id,operation_key,correlation_id,initial_turn_id,message,working_directory,model,reasoning_effort,fast,created_at_micros,service_tier) VALUES (?1,'original-key','original-key','45000000-0000-4000-8000-000000000000','Original input',?2,'old-requested-model','low',0,1,'flex')", rusqlite::params!["44000000-0000-4000-8000-000000000001", directory.to_str().expect("settled conversation history fixture")]).expect("settled conversation history fixture");
}

async fn runtime(
	root: &DecodexRoot,
	store: &SqliteStore,
	accounts: Arc<AccountService>,
	profile: crate::account_launch::AttestedAppServerProfile,
) -> ConversationRuntime {
	ConversationRuntime::new(
		store.clone(),
		BlobStore::open(root.paths()).expect("production runtime fixture"),
		accounts,
		crate::process_supervisor::ProcessGenerationControl::start(store.clone())
			.await
			.expect("production runtime fixture"),
		crate::provider_attempt_service::ProviderAttemptControl::start(store.clone())
			.await
			.expect("production runtime fixture"),
		ProcessExecutionAuthorization::new(
			ProcessExecutionEpochId::new("30000000-0000-4000-8000-000000000001")
				.expect("production runtime fixture"),
			"1".repeat(64),
		)
		.expect("production runtime fixture"),
		profile,
		crate::account_launch::RunnerCapacity::daemon().expect("production runtime fixture"),
	)
}

async fn serve(
	listener: tokio::net::TcpListener,
	requests: Arc<std::sync::atomic::AtomicUsize>,
	metadata: Arc<std::sync::atomic::AtomicUsize>,
) {
	use tokio::io::{AsyncBufReadExt as _, AsyncReadExt as _, AsyncWriteExt as _};
	while let Ok((socket, _)) = listener.accept().await {
		let mut socket = tokio::io::BufReader::new(socket);
		let mut first = String::new();
		socket.read_line(&mut first).await.expect("loopback native backend");
		let path = first.split_whitespace().nth(1).expect("loopback native backend").to_owned();
		let mut length = 0;
		loop {
			let mut line = String::new();
			assert!(socket.read_line(&mut line).await.expect("loopback native backend") > 0);
			if line == "\r\n" {
				break;
			}
			if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
				length = value.trim().parse::<usize>().expect("loopback native backend");
			}
		}
		let (status, content_type, body) = if first.starts_with("GET ") {
			metadata.fetch_add(1, Ordering::AcqRel);
			if path.contains("/models") {
				(200, "application/json", json!({"models":[effort::fixture_model("cold-native-model", "provider-effort")]}).to_string())
			} else if path.contains("/accounts/check") {
				(200,"application/json",json!({"accounts":[{"id":"workspace-fixture","workspace_backend_origin":"https://chatgpt.com","account_routing_override":"NO_CONSTRAINT"}]}).to_string())
			} else if path.contains("/settings/user") {
				(200, "application/json", json!({"commit_attribution_enabled":false}).to_string())
			} else if path.contains("/plugins/featured") {
				(200, "application/json", "[]".into())
			} else {
				eprintln!("optional fixture metadata unavailable: {path}");
				(404, "application/json", "{}".into())
			}
		} else {
			assert!(
				first.starts_with("POST ") && path.ends_with("/responses"),
				"unexpected fixture inference route: {path}"
			);
			assert!((1..=2 * 1024 * 1024).contains(&length));
			let mut input = vec![0; length];
			socket.read_exact(&mut input).await.expect("loopback native backend");
			let input: Value = serde_json::from_slice(&input).expect("loopback native backend");
			assert_eq!(input["model"], "cold-native-model");
			assert_eq!(input["reasoning"]["effort"], "provider-effort");
			assert!(
				input.get("service_tier").is_none(),
				"cached priority must not override Standard"
			);
			let serial = requests.fetch_add(1, Ordering::AcqRel);
			let id = format!("cold-fixture-{serial}");
			let answer = if input["text"]["format"]["type"] == "json_schema" {
				assert!(
					input["tools"].as_array().is_none_or(Vec::is_empty),
					"structured fixture tool names: {:?}; schema fields: {:?}",
					input["tools"].as_array().map(|tools| tools
						.iter()
						.map(|tool| (&tool["type"], &tool["name"]))
						.collect::<Vec<_>>()),
					input["text"]["format"]["schema"]["properties"]
						.as_object()
						.map(|p| p.keys().collect::<Vec<_>>())
				);
				r#"{"summary":"The requested fix was tested; installation is still pending.","next_action":null}"#
			} else {
				"Saved native answer"
			};
			let frames = [
				json!({"type":"response.created","response":{"id":id}}),
				json!({"type":"response.output_item.done","item":{"type":"message","role":"assistant","id":"answer","content":[{"type":"output_text","text":answer}]}}),
				json!({"type":"response.completed","response":{"id":id,"usage":{"input_tokens":0,"output_tokens":0,"total_tokens":0}}}),
			];
			(
				200,
				"text/event-stream",
				frames
					.iter()
					.map(|value| {
						format!(
							"event: {}\ndata: {value}\n\n",
							value["type"].as_str().expect("loopback native backend")
						)
					})
					.collect::<String>(),
			)
		};
		socket.get_mut().write_all(format!("HTTP/1.1 {status} Fixture\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.expect("loopback native backend");
	}
}
