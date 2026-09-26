//! Disposable ownership fixture. This does not qualify kernel admission or credential enrollment.
use crate::chief_usage_estimate::{Source, SourceKey};
use decodex_codex::app_server_client::AppServerClient;
use decodex_core::{
	AccountId, AccountOperationId, AccountProvider, CredentialBinding, CredentialFingerprint,
	CredentialStoreSchemaVersion, CredentialVersion, DecodexRoot, ProcessBootIdentity,
	ProcessControlKind, ProcessExecutionAuthorization, ProcessExecutionEpochId,
	ProcessGenerationAccountBinding, ProcessGenerationId, ProcessGenerationIntent, ProcessIdentity,
	ProcessIsolationKind, ProcessRunnerIdentity, ProcessStartIdentity, ProviderIdentity,
};
use decodex_database::{
	ChiefDispatchState, ChiefWorkItem, ChiefWorkKind, ChiefWorkStatus,
	CodexAccountCapabilityAttestation, SqliteStore,
};
const DIGEST: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const ACCOUNT: &str = "10000000-0000-4000-8000-000000000001";
const OPERATION: &str = "20000000-0000-4000-8000-000000000001";
const GENERATION: &str = "30000000-0000-4000-8000-000000000001";

pub(crate) struct OwnedReviewer {
	pub(crate) store: SqliteStore,
	pub(crate) key: SourceKey,
	client: AppServerClient,
}

impl OwnedReviewer {
	pub(crate) async fn new(
		home: &std::path::Path,
		client: &AppServerClient,
		thread: &str,
		turn: &str,
	) -> Self {
		let root = DecodexRoot::new(home.canonicalize().expect("fixture home").join("state"))
			.expect("fixture root");
		root.paths().ensure_layout().expect("fixture layout");
		let store = SqliteStore::open(&root.paths()).expect("fixture database");
		seed_account(&root);
		store
			.attest_codex_account_capability(&CodexAccountCapabilityAttestation {
				build_identity: "fixture".into(),
				executable_sha256: DIGEST.into(),
				schema_sha256: DIGEST.into(),
				callback_profile_sha256: DIGEST.into(),
				login_chatgpt_auth_tokens: true,
				refresh_callback: true,
			})
			.await
			.expect("fixture capability");
		store
			.create_chief_work_item(ChiefWorkItem {
				id: "root".into(),
				parent_goal_id: None,
				kind: ChiefWorkKind::Goal,
				title: "Fixture".into(),
				instructions: "Fixture".into(),
				codex_thread_id: None,
				status: ChiefWorkStatus::Open,
				next_check_at_micros: None,
				created_at_micros: 1,
				updated_at_micros: 1,
				active_turn_id: None,
				dispatch_state: ChiefDispatchState::Idle,
			})
			.await
			.expect("fixture root work");
		let generation = ProcessGenerationId::new(GENERATION).expect("fixture generation");
		let account = AccountId::new(ACCOUNT).expect("fixture account");
		let boot = ProcessBootIdentity::new("fixture-boot").expect("fixture boot");
		let intent = ProcessGenerationIntent {
			generation_id: generation.clone(),
			account_id: account.clone(),
			runner_identity: ProcessRunnerIdentity::new(format!("sha256:{DIGEST}"))
				.expect("fixture runner"),
			intended_boot_id: boot.clone(),
			control_kind: ProcessControlKind::StdioOnlyBestEffortEof,
			isolation_kind: ProcessIsolationKind::Session,
			execution_authorization: ProcessExecutionAuthorization::new(
				ProcessExecutionEpochId::new("40000000-0000-4000-8000-000000000001")
					.expect("fixture epoch"),
				DIGEST,
			)
			.expect("fixture authorization"),
		};
		let binding = ProcessGenerationAccountBinding::new(
			1,
			CredentialBinding {
				schema_version: CredentialStoreSchemaVersion::V1,
				version: CredentialVersion::new(1).expect("fixture version"),
				fingerprint: CredentialFingerprint::new(DIGEST).expect("fixture fingerprint"),
				provider: ProviderIdentity::new(AccountProvider::Chatgpt, "provider-1")
					.expect("fixture provider"),
				writer_operation_id: AccountOperationId::new(OPERATION).expect("fixture operation"),
			},
			DIGEST,
		)
		.expect("fixture binding");
		store
			.prepare_chief_bound_process_generation(&intent, &binding, "root", "reviewer-admission")
			.await
			.expect("fixture admission");
		let identity = ProcessIdentity::new(
			boot,
			1234,
			ProcessStartIdentity::new("fixture-start").expect("fixture start"),
			1234,
			1234,
		)
		.expect("fixture identity");
		store
			.bind_process_generation_identity(&generation, 1, &identity)
			.await
			.expect("fixture identity binding");
		store.mark_process_generation_ready(&generation, 2).await.expect("fixture readiness");
		store.bind_chief_thread("root".into(), thread.into()).await.expect("native thread binding");
		store.begin_chief_dispatch("root".into()).await.expect("fixture dispatch");
		store
			.acknowledge_chief_dispatch("root".into(), turn.into())
			.await
			.expect("native turn binding");
		Self {
			store,
			key: SourceKey {
				generation,
				account,
				revision: 1,
				history_revision: client.history_revision(),
				thread: thread.into(),
				work: "root".into(),
			},
			client: client.clone(),
		}
	}

	fn source(&self, key: &SourceKey) -> Source {
		Source { key: key.clone(), client: self.client.clone() }
	}
}

fn seed_account(root: &DecodexRoot) {
	let connection = rusqlite::Connection::open(root.paths().product_database_file())
		.expect("disposable fixture connection");
	connection
		.execute("INSERT INTO account_identities VALUES (?1,1)", [ACCOUNT])
		.expect("fixture identity");
	connection.execute("INSERT INTO account_operations (operation_id,account_id,kind,phase,provider,provider_account_id,requested_display_label,requested_enabled,created_at_micros,updated_at_micros,completed_at_micros) VALUES (?1,?2,'enroll','committed','chatgpt','provider-1','Fixture',1,1,1,1)",rusqlite::params![OPERATION,ACCOUNT]).expect("fixture operation");
	connection.execute("INSERT INTO accounts VALUES (?1,'Fixture',1,'available',1,'chatgpt','provider-1','exact',1,1,NULL)",[ACCOUNT]).expect("fixture account");
	connection
		.execute(
			"INSERT INTO account_credentials VALUES (?1,1,1,?2,?3,'chatgpt','provider-1',X'01020304',1)",
			rusqlite::params![ACCOUNT, DIGEST, OPERATION],
		)
		.expect("inert fixture credential row");
}

// Model observations must retain exact ownership across the native read.

use decodex_protocol::ChiefModelSettingsResult as Result;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[tokio::test]
async fn model_settings_discard_changed_sources_and_preserve_null_metadata() {
	for change in [
		"none",
		"null",
		"missing",
		"invalid_provider",
		"account",
		"revision",
		"history",
		"process",
		"thread",
		"work",
		"closed",
		"settings",
		"other_settings",
	] {
		let home = tempfile::tempdir().unwrap();
		let (local, remote) = tokio::io::duplex(4096);
		let (r, w) = tokio::io::split(local);
		let (client, _events) = AppServerClient::from_io(r, w);
		let owner = OwnedReviewer::new(home.path(), &client, "thread", "turn").await;
		let (release, released) = tokio::sync::oneshot::channel::<()>();
		let server = tokio::spawn(async move {
			let (r, mut w) = tokio::io::split(remote);
			let mut lines = BufReader::new(r).lines();
			let request: serde_json::Value =
				serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
			assert_eq!(request["method"], "thread/read");
			assert_eq!(
				request["params"],
				serde_json::json!({"threadId":"thread","includeTurns":false})
			);
			let mut thread = match change {
				"missing" => serde_json::json!({"id":"thread"}),
				"null" => serde_json::json!({"id":"thread","model":null,"reasoningEffort":null}),
				_ =>
					serde_json::json!({"id":"thread","model":"configured-model","reasoningEffort":"future-effort","modelProvider":"server-provider"}),
			};
			if change == "invalid_provider" {
				thread["modelProvider"] = serde_json::json!("\n");
			}
			if matches!(change, "settings" | "other_settings") {
				let target = if change == "settings" { "thread" } else { "other" };
				w.write_all(format!("{}\n",serde_json::json!({"method":"thread/settings/updated","params":{"threadId":target,"threadSettings":{"model":"new-choice"}}})).as_bytes()).await.unwrap();
			}
			w.write_all(
				format!("{}\n", serde_json::json!({"id":request["id"],"result":{"thread":thread}}))
					.as_bytes(),
			)
			.await
			.unwrap();
			let _ = released.await;
		});
		let calls = AtomicUsize::new(0);
		let result = crate::chief_model_settings::read(&owner.store, || {
			let later = calls.fetch_add(1, Ordering::SeqCst) > 0;
			let mut key = owner.key.clone();
			if later {
				match change {
					"account" =>
						key.account =
							AccountId::new("50000000-0000-4000-8000-000000000001").unwrap(),
					"revision" => key.revision += 1,
					"history" => key.history_revision += 1,
					"process" =>
						key.generation =
							ProcessGenerationId::new("60000000-0000-4000-8000-000000000001")
								.unwrap(),
					"thread" => key.thread = "other".into(),
					"work" => key.work = "other".into(),
					_ => {},
				}
			}
			let source = (!(later && change == "closed")).then(|| owner.source(&key));
			async move { source }
		})
		.await;
		let _ = release.send(());
		server.await.unwrap();
		match change {
			"none" | "other_settings" => assert!(
				matches!(result, Result::Available { model: Some(ref model), reasoning_effort: Some(ref effort), model_provider: Some(ref provider), .. } if model.as_str()=="configured-model" && effort.as_str()=="future-effort" && provider.as_str()=="server-provider")
			),
			"null" => assert!(matches!(
				result,
				Result::Available { model: None, reasoning_effort: None, .. }
			)),
			"missing" => assert_eq!(result, Result::NotReported),
			_ => assert_eq!(result, Result::Unavailable, "{change}"),
		}
		assert!(owner.store.list_pending_chief_events(100).await.unwrap().is_empty());
	}
}

#[tokio::test]
async fn model_settings_do_not_read_a_foreign_thread() {
	let home = tempfile::tempdir().unwrap();
	let (local, mut remote) = tokio::io::duplex(4096);
	let (r, w) = tokio::io::split(local);
	let (client, _events) = AppServerClient::from_io(r, w);
	let owner = OwnedReviewer::new(home.path(), &client, "thread", "turn").await;
	let mut key = owner.key.clone();
	key.thread = "foreign".into();
	assert_eq!(
		crate::chief_model_settings::read(&owner.store, || async { Some(owner.source(&key)) })
			.await,
		Result::Unavailable
	);
	let mut byte = [0];
	assert!(
		tokio::time::timeout(
			std::time::Duration::from_millis(25),
			tokio::io::AsyncReadExt::read(&mut remote, &mut byte)
		)
		.await
		.is_err()
	);
}
