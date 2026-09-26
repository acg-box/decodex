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
use decodex_protocol::{ChiefLiveReviewerOutcome, ChiefLiveReviewerState, ChiefReviewer};
use sha2::Digest as _;
const DIGEST: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const ACCOUNT: &str = "10000000-0000-4000-8000-000000000001";
const OPERATION: &str = "20000000-0000-4000-8000-000000000001";
const GENERATION: &str = "30000000-0000-4000-8000-000000000001";

pub(super) struct OwnedReviewer {
	store: SqliteStore,
	root: DecodexRoot,
	key: SourceKey,
	client: AppServerClient,
}

impl OwnedReviewer {
	pub(super) async fn new(
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
			root,
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

	pub(super) async fn publish(&self, turn: &str, reviewer: ChiefReviewer) {
		let state = crate::chief_live_settings::read(&self.store, || async {
			Some(self.source(&self.key))
		})
		.await;
		let ChiefLiveReviewerState::Available {
			review_token,
			can_update: true,
			last_outcome: None,
			..
		} = state
		else {
			panic!("editable native owner");
		};
		let mut changed = self.key.clone();
		changed.revision += 1;
		assert!(
			crate::chief_live_settings::write(
				&self.store,
				|| async { Some(self.source(&changed)) },
				turn,
				review_token.as_str(),
				ChiefReviewer::User,
				"stale-source"
			)
			.await
			.is_err()
		);
		assert!(
			self.store
				.chief_live_reviewer_receipt("root".into(), self.key.thread.clone(), turn.into())
				.await
				.expect("receipt query")
				.is_none()
		);
		crate::chief_live_settings::write(
			&self.store,
			|| async { Some(self.source(&self.key)) },
			turn,
			review_token.as_str(),
			reviewer,
			"native-publish",
		)
		.await
		.expect("native reviewer publication");
		assert!(
			crate::chief_live_settings::write(
				&self.store,
				|| async { Some(self.source(&self.key)) },
				turn,
				review_token.as_str(),
				ChiefReviewer::AutoReview,
				"duplicate-review"
			)
			.await
			.is_err()
		);
		let reopened = SqliteStore::open(&self.root.paths()).expect("reopen durable receipt");
		let state =
			crate::chief_live_settings::read(&reopened, || async { Some(self.source(&self.key)) })
				.await;
		assert!(matches!(
			state,
			ChiefLiveReviewerState::Available {
				last_reviewer: Some(observed),
				last_outcome: Some(ChiefLiveReviewerOutcome::Applied),
				..
			} if observed == reviewer
		));
		assert!(reopened.list_pending_chief_events(100).await.expect("pending query").is_empty());
	}

	pub(super) async fn completed_target(&self, turn: &str) {
		// Deliberately leave the local turn running to simulate delayed terminal notification.
		let state = crate::chief_live_settings::read(&self.store, || async {
			Some(self.source(&self.key))
		})
		.await;
		let ChiefLiveReviewerState::Available { review_token, .. } = state else {
			panic!("local running receipt");
		};
		assert!(
			crate::chief_live_settings::write(
				&self.store,
				|| async { Some(self.source(&self.key)) },
				turn,
				review_token.as_str(),
				ChiefReviewer::AutoReview,
				"completed-target"
			)
			.await
			.is_err()
		);
		let state = crate::chief_live_settings::read(&self.store, || async {
			Some(self.source(&self.key))
		})
		.await;
		assert!(matches!(
			state,
			ChiefLiveReviewerState::Available {
				last_outcome: Some(ChiefLiveReviewerOutcome::TargetUnavailable),
				..
			}
		));
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

#[path = "chief_process_native_reviewer_outcome_tests.rs"] mod outcome_tests;

#[path = "chief_process_permission_service_tests.rs"] mod permission_service_tests;

impl OwnedReviewer {
	pub(super) async fn select_permission(&self) {
		use decodex_protocol::{ChiefPermissionOutcome as Outcome, ChiefPermissionState as State};
		let source = || async { Some(self.source(&self.key)) };
		let State::Available { review_token, .. } =
			crate::chief_permissions::read(&self.store, source).await
		else {
			panic!("native permission review")
		};
		crate::chief_permissions::write(
			&self.store,
			source,
			&self.key.thread,
			review_token.as_str(),
			"scoped",
			"native-permission",
		)
		.await
		.expect("native permission selection");
		tokio::time::timeout(std::time::Duration::from_secs(5), async {
			loop {
				if matches!(
					crate::chief_permissions::read(&self.store, source).await,
					State::Available { last_outcome: Some(Outcome::TargetObserved), .. }
				) {
					break;
				}
				tokio::time::sleep(std::time::Duration::from_millis(10)).await;
			}
		})
		.await
		.expect("native publication settles receipt");
		assert!(
			crate::chief_permissions::write(
				&self.store,
				source,
				&self.key.thread,
				review_token.as_str(),
				"scoped",
				"duplicate"
			)
			.await
			.is_err()
		);
		let reopened = SqliteStore::open(&self.root.paths()).expect("receipt restart");
		assert_eq!(
			reopened
				.chief_permission_receipt("root".into(), self.key.thread.clone())
				.await
				.expect("receipt")
				.expect("saved")
				.state,
			"target_observed"
		);
	}
}

#[path = "chief_process_model_service_tests.rs"] mod model_service_tests;
#[path = "chief_process_plugin_service_tests.rs"] mod plugin_service_tests;

impl OwnedReviewer {
	pub(super) async fn select_task_model(&self, model: &str, effort: Option<&str>) {
		use decodex_protocol::{ChiefModelOutcome as Outcome, ChiefModelSelectionState as State};
		let source = || async { Some(self.source(&self.key)) };
		let State::Available { review_token, .. } =
			crate::chief_models::read(&self.store, source).await
		else {
			panic!("native model review")
		};
		crate::chief_models::write(
			&self.store,
			source,
			crate::chief_models::Change {
				thread: &self.key.thread,
				review: review_token.as_str(),
				model,
				effort,
				attempt_id: "native-model",
			},
		)
		.await
		.expect("native model selection");
		tokio::time::timeout(std::time::Duration::from_secs(5), async {
			loop {
				if matches!(
					crate::chief_models::read(&self.store, source).await,
					State::Available { last_outcome: Some(Outcome::TargetObserved), .. }
				) {
					break;
				}
				tokio::time::sleep(std::time::Duration::from_millis(10)).await;
			}
		})
		.await
		.expect("native model publication");
		assert!(
			crate::chief_models::write(
				&self.store,
				source,
				crate::chief_models::Change {
					thread: &self.key.thread,
					review: review_token.as_str(),
					model,
					effort,
					attempt_id: "replay",
				}
			)
			.await
			.is_err()
		);
	}
}

impl OwnedReviewer {
	pub(super) async fn select_task_plugin(&self) {
		use decodex_protocol::{ChiefPluginOutcome as Outcome, ChiefPluginSelectionState as State};
		let source = || async { Some(self.source(&self.key)) };
		let State::Available { review_token, .. } =
			crate::chief_plugins::read(&self.store, source).await
		else {
			panic!("native plugin review")
		};
		crate::chief_plugins::write(
			&self.store,
			source,
			crate::chief_plugins::Change {
				thread: &self.key.thread,
				review: review_token.as_str(),
				plugin: "sample@test",
				enabled: false,
				attempt_id: "native-plugin",
			},
		)
		.await
		.expect("native plugin selection");
		tokio::time::timeout(std::time::Duration::from_secs(5), async {
			loop {
				if matches!(
					crate::chief_plugins::read(&self.store, source).await,
					State::Available { last_outcome: Some(Outcome::TargetObserved), .. }
				) {
					break;
				}
				tokio::time::sleep(std::time::Duration::from_millis(10)).await;
			}
		})
		.await
		.expect("native plugin publication");
		assert!(
			crate::chief_plugins::write(
				&self.store,
				source,
				crate::chief_plugins::Change {
					thread: &self.key.thread,
					review: review_token.as_str(),
					plugin: "sample@test",
					enabled: false,
					attempt_id: "replay"
				}
			)
			.await
			.is_err()
		);
		let reopened = SqliteStore::open(&self.root.paths()).expect("receipt reopen");
		assert_eq!(
			reopened
				.chief_plugin_receipt("root".into(), self.key.thread.clone())
				.await
				.expect("receipt")
				.expect("saved")
				.state,
			"target_observed"
		);
	}
}

#[path = "chief_process_app_native_tests.rs"] mod app_native_tests;
#[path = "chief_process_app_service_tests.rs"] mod app_service_tests;
#[path = "chief_process_hook_service_tests.rs"] mod hook_service_tests;

impl OwnedReviewer {
	pub(super) async fn trust_hook(&self) {
		use decodex_protocol::{ChiefHookChange, ChiefHookSettingsState as State};
		let source = || async {
			let mut key = self.key.clone();
			key.history_revision = self.client.history_revision();
			Some(self.source(&key))
		};
		let State::Available { review_token, hooks, config_file, .. } =
			crate::chief_hooks::read(&self.store, source).await
		else {
			panic!("hook service review")
		};
		let hook = hooks
			.iter()
			.find(|h| h.details.contains("echo isolated-plugin-hook"))
			.expect("known harmless fixture hook");
		assert_eq!(hook.trust_status, "untrusted");
		crate::chief_hooks::write(
			&self.store,
			source,
			crate::chief_hooks::Selection {
				thread: &self.key.thread,
				review: review_token.as_str(),
				hook: hook.key.as_str(),
				change: ChiefHookChange::Trust,
				attempt_id: "native-hook",
			},
		)
		.await
		.expect("production hook trust");
		let State::Available { last_edit: Some(edit), hooks, .. } =
			crate::chief_hooks::read(&self.store, source).await
		else {
			panic!("native hook readback")
		};
		assert_eq!(edit.outcome, "saved");
		assert_eq!(hooks.iter().find(|h| h.key == hook.key).expect("hook").trust_status, "trusted");
		assert!(
			crate::chief_hooks::write(
				&self.store,
				source,
				crate::chief_hooks::Selection {
					thread: &self.key.thread,
					review: review_token.as_str(),
					hook: hook.key.as_str(),
					change: ChiefHookChange::Trust,
					attempt_id: "replay-hook"
				}
			)
			.await
			.is_err()
		);
		let scope: String = sha2::Sha256::digest(config_file.as_str().as_bytes())
			.iter()
			.map(|b| format!("{b:02x}"))
			.collect();
		let reopened = SqliteStore::open(&self.root.paths()).expect("hook receipt reopen");
		assert_eq!(
			reopened
				.chief_hook_receipt(scope)
				.await
				.expect("receipt")
				.expect("saved receipt")
				.state,
			"saved"
		);
	}
}

#[path = "chief_process_app_exposure_tests.rs"] mod exposure;

#[path = "chief_process_warning_tests.rs"] mod warning_tests;

#[path = "chief_process_voice_settings_start_tests.rs"] mod voice_settings_start;

#[path = "chief_process_app_ui_call_tests.rs"] mod app_ui_call;
