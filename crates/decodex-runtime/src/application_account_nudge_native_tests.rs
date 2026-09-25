//! Explicit isolated-process qualification of the durable notification command owner.
#[path = "application_account_nudge_socket_tests.rs"] mod socket;
use super::*;
use crate::{
	account_observation::AccountObservationService,
	account_service::{
		AccountService, CredentialRefreshError, CredentialRefreshPort, CredentialRefreshResult,
	},
	conversation::{ConversationCapability, ConversationRuntime},
	host_credentials::{CredentialSecretBundle, SqliteCredentialStore},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use decodex_core::{
	AccountId, AccountOperationId, BlobStore, DecodexRoot, ProcessExecutionAuthorization,
	ProcessExecutionEpochId,
};
use decodex_protocol::{
	CURRENT_VERSION, ClientCommandId, CommandPayload, CorrelationId, EntityId, EntityRevision,
	IdempotencyKey,
};
use serde_json::json;
use std::{
	io::Write as _, os::unix::fs::OpenOptionsExt as _, path::PathBuf, sync::Arc, time::Duration,
};

struct NoRefresh;
impl CredentialRefreshPort for NoRefresh {
	fn refresh(
		&self,
		_: &CredentialSecretBundle,
	) -> Result<CredentialRefreshResult, CredentialRefreshError> {
		panic!("fresh synthetic credentials must not contact a refresh provider")
	}
}

fn isolated_home() -> PathBuf {
	let requested = PathBuf::from(
		std::env::var_os("DECODEX_TEST_ACCOUNT_HOME")
			.expect("explicit disposable home under the OS account home, outside .codex"),
	);
	let home = requested.canonicalize().expect("canonical fixture home");
	assert_eq!(PathBuf::from(std::env::var_os("HOME").expect("HOME")), home);
	assert_eq!(
		std::fs::read_to_string(home.join(".decodex-native-test-home"))
			.expect("fixture opt-in marker"),
		"native-account-command-fixture\n"
	);
	assert!(!home.join(".codex").exists(), "fixture requires a fresh isolated home");
	home
}

fn credential_file(home: &std::path::Path) -> PathBuf {
	let claims = json!({"email":"fixture@example.test","exp":4102444800_u64,
		"https://api.openai.com/auth":{"chatgpt_account_id":"workspace-fixture","chatgpt_user_id":"user-fixture","chatgpt_plan_type":"team"}});
	let token = format!(
		"{}.{}.fixture-signature",
		URL_SAFE_NO_PAD.encode(r#"{"alg":"none"}"#),
		URL_SAFE_NO_PAD.encode(claims.to_string())
	);
	let value = json!({"auth_mode":"chatgpt","tokens":{"access_token":token,"id_token":token,
		"refresh_token":"fixture-only","account_id":"workspace-fixture"},"last_refresh":"2026-09-21T15:00:00Z"});
	let path = home.join("synthetic-credential.json");
	let mut file = std::fs::OpenOptions::new()
		.write(true)
		.create_new(true)
		.mode(0o600)
		.open(&path)
		.expect("private fixture input");
	file.write_all(value.to_string().as_bytes()).expect("synthetic credential input");
	path
}

async fn enroll(
	store: &decodex_database::SqliteStore,
	service: &AccountService,
	home: &std::path::Path,
) -> AccountId {
	let account = AccountId::new("10000000-0000-4000-8000-000000000001").expect("account identity");
	let identity = CommandIdentity::new("fixture-enrollment", b"synthetic account enrollment")
		.expect("enrollment command");
	let AccountCommandReceiptClaim::Owned(lease) = store
		.reserve_account_command(&identity, AccountCommandKind::Enroll, account.as_str(), None)
		.await
		.expect("enrollment reservation")
	else {
		panic!("enrollment owner")
	};
	let source = credential_file(home);
	service
		.enroll_from_credential_file_command(
			lease,
			AccountOperationId::new("20000000-0000-4000-8000-000000000001")
				.expect("enrollment operation"),
			account.clone(),
			true,
			source.to_str().expect("fixture path"),
			|result| {
				assert!(result.is_ok(), "synthetic account enrollment failed");
				Ok(json!({"enrolled":true}))
			},
		)
		.await
		.expect("owned credential enrollment");
	std::fs::remove_file(source).expect("remove synthetic import file");
	account
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "run alone in explicit DECODEX_TEST_ACCOUNT_HOME with matching HOME and fixture marker"]
async fn native_notification_command_sends_once_and_replays_after_store_reopen() {
	let home = isolated_home();
	let binary = std::env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit native binary");
	assert!(std::path::Path::new(&binary).is_absolute());
	std::fs::create_dir(home.join(".codex")).expect("isolated Codex home");
	let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("fixture backend");
	let address = listener.local_addr().expect("loopback address");
	std::fs::write(
		home.join(".codex/config.toml"),
		format!("chatgpt_base_url = \"http://{address}\"\ncli_auth_credentials_store = \"file\"\n"),
	)
	.expect("fixture configuration");
	let root = DecodexRoot::new(home.join("product")).expect("isolated product root");
	root.paths().ensure_layout().expect("product layout");
	let store = decodex_database::SqliteStore::open(&root.paths()).expect("product store");
	let accounts = Arc::new(AccountService::new(
		store.clone(),
		Arc::new(SqliteCredentialStore::new(store.clone())),
		Arc::new(NoRefresh),
	));
	let account = enroll(&store, &accounts, &home).await;
	let profile_home = home.clone();
	let profile = tokio::task::spawn_blocking(move || {
		crate::account_launch::process::native_control_tests::attested_profile(
			&binary,
			&profile_home,
		)
	})
	.await
	.expect("native profile owner");
	assert!(
		accounts
			.attest_callback_capability(profile.account_callback_attestation())
			.await
			.expect("native callback attestation")
	);
	let revision = accounts.inspect(&account).await.expect("enrolled account").account.revision;
	assert!(
		store.account_is_ready_at_revision(&account, revision).await.expect("account readiness")
	);
	let observations = AccountObservationService::new(Arc::clone(&accounts), None, None, None);
	let usage = fixture_usage();
	observations
		.cache_recovery_fixture(
			account.clone(),
			revision,
			usage,
			"workspace-fixture",
			"user-fixture",
		)
		.await;
	let source = observations
		.recovery(
			&EntityId::new(account.as_str()).expect("account entity"),
			EntityRevision(revision as u64),
		)
		.await;
	let runtime = runtime(&root, &store, accounts, profile).await;
	let mut app = super::tests::application(store.clone())
		.with_account_observations(Some(observations.clone()));
	app.conversations = ConversationCapability::Ready(runtime);
	let command = CommandEnvelope {
		version: CURRENT_VERSION,
		client_command_id: ClientCommandId::new("native-nudge").expect("command id"),
		idempotency_key: IdempotencyKey::new("native-nudge-once").expect("operation key"),
		expected_revision: Some(source.account_revision),
		correlation_id: CorrelationId::new("native-nudge-correlation").expect("correlation"),
		causation_id: None,
		payload: CommandPayload::SendAccountRecoveryNudge {
			source: Box::new(source.clone()),
			action: AccountRecoveryAction::NotifyOwner,
		},
	};
	let send = concurrent_sends(&app, &command, &source);
	let (result, ()) = tokio::time::timeout(Duration::from_secs(45), async {
		tokio::join!(
			send,
			crate::account_launch::serve_native_nudge_fixture(
				&listener,
				"200 OK",
				decodex_codex::app_server_client::AccountNudgeCreditType::Credits
			)
		)
	})
	.await
	.expect("bounded native command");
	assert!(matches!(
		result.expect("command result").result,
		ResultPayload::AccountRecoveryNudge { status: Status::Sent, .. }
	));
	assert_source_invalidated_during_launch(&app, &command, &source, &observations, &listener)
		.await;
	socket::qualify(app, &root, &source, &observations, &listener).await;
	drop(store);
	let reopened = super::tests::application(
		decodex_database::SqliteStore::open(&root.paths()).expect("reopen store"),
	);
	let replay = reopened
		.execute_recovery_nudge(&command, &source, AccountRecoveryAction::NotifyOwner)
		.await
		.expect("durable replay");
	assert!(matches!(
		replay.result,
		ResultPayload::AccountRecoveryNudge { status: Status::Sent, .. }
	));
	assert!(
		tokio::time::timeout(Duration::from_millis(300), listener.accept()).await.is_err(),
		"replay must not send again"
	);
}

async fn runtime(
	root: &DecodexRoot,
	store: &decodex_database::SqliteStore,
	accounts: Arc<AccountService>,
	profile: crate::account_launch::AttestedAppServerProfile,
) -> ConversationRuntime {
	ConversationRuntime::new(
		store.clone(),
		BlobStore::open(root.paths()).expect("blob store"),
		accounts,
		crate::process_supervisor::ProcessGenerationControl::start(store.clone())
			.await
			.expect("process control"),
		crate::provider_attempt_service::ProviderAttemptControl::start(store.clone())
			.await
			.expect("attempt control"),
		ProcessExecutionAuthorization::new(
			ProcessExecutionEpochId::new("30000000-0000-4000-8000-000000000001")
				.expect("fixture epoch"),
			"1".repeat(64),
		)
		.expect("fixture execution authority"),
		profile,
		crate::account_launch::RunnerCapacity::daemon().expect("fixture capacity"),
	)
}

async fn assert_source_invalidated_during_launch(
	app: &ServiceApplication,
	original: &CommandEnvelope,
	source: &AccountRecoveryResult,
	observations: &AccountObservationService,
	listener: &tokio::net::TcpListener,
) {
	let mut command = original.clone();
	command.idempotency_key =
		IdempotencyKey::new("invalidate-during-native-launch").expect("new explicit operation");
	let gate = Arc::new((tokio::sync::Notify::new(), tokio::sync::Notify::new()));
	let change = async {
		gate.0.notified().await;
		observations
			.invalidate_account(&AccountId::new(source.account_id.as_str()).expect("exact account"))
			.await;
		gate.1.notify_one();
	};
	let send = app.execute_recovery_nudge(&command, source, AccountRecoveryAction::NotifyOwner);
	let backend = crate::account_launch::serve_native_nudge_with_gate(
		listener,
		"200 OK",
		decodex_codex::app_server_client::AccountNudgeCreditType::Credits,
		Some(Arc::clone(&gate)),
	);
	let outcome = tokio::time::timeout(Duration::from_secs(30), async {
		tokio::select! {
		 _ = backend => panic!("invalidated source sent a notification"),
		 (result, ()) = async { tokio::join!(send, change) } => result,
		}
	})
	.await
	.expect("bounded source mutation")
	.expect("persist refused command");
	assert!(matches!(
		outcome.result,
		ResultPayload::AccountRecoveryNudge { status: Status::Unavailable, .. }
	));
	let replay = app
		.execute_recovery_nudge(&command, source, AccountRecoveryAction::NotifyOwner)
		.await
		.expect("replay refused command");
	assert!(matches!(
		replay.result,
		ResultPayload::AccountRecoveryNudge { status: Status::Unavailable, .. }
	));
}

async fn concurrent_sends(
	app: &ServiceApplication,
	command: &CommandEnvelope,
	source: &AccountRecoveryResult,
) -> Result<ApplicationPublication, CommandError> {
	let mut peer = command.clone();
	peer.client_command_id =
		ClientCommandId::new("native-nudge-peer").expect("second client command");
	let (first, second) = tokio::join!(
		app.execute_recovery_nudge(command, source, AccountRecoveryAction::NotifyOwner),
		app.execute_recovery_nudge(&peer, source, AccountRecoveryAction::NotifyOwner)
	);
	let results = [first?, second?];
	assert_eq!(
		results
			.iter()
			.filter(|r| matches!(
				r.result,
				ResultPayload::AccountRecoveryNudge { status: Status::Sent, .. }
			))
			.count(),
		1
	);
	assert_eq!(
		results
			.iter()
			.filter(|r| matches!(
				r.result,
				ResultPayload::AccountRecoveryNudge { status: Status::Uncertain, .. }
			))
			.count(),
		1
	);
	Ok(results
		.into_iter()
		.find(|r| {
			matches!(r.result, ResultPayload::AccountRecoveryNudge { status: Status::Sent, .. })
		})
		.expect("one native send owner"))
}

fn fixture_usage() -> decodex_codex::AccountApiUsage {
	decodex_codex::decode_account_api_usage(br#"{"account_id":"workspace-fixture","user_id":"user-fixture","plan_type":"team","rate_limit":{},"rate_limit_upsell":{"banner_type":"limit","title":"Workspace limit","description":"Ask the owner","ctas":[{"action":"notify_owner","label":"Notify owner"}]}}"#).expect("backend recovery source")
}
