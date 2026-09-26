//! Installed-native discovery for a durable model-source review.
use super::*;
use crate::application::Application;
use decodex_database::SqliteStore;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "run alone in an isolated fixture HOME below the OS account home and outside its .codex"]
async fn native_model_review_uses_saved_directory_without_starting_a_turn() {
	tokio::time::timeout(Duration::from_secs(90), qualify_review_discovery(false, false))
		.await
		.expect("bounded native model review");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "run alone in an isolated fixture HOME below the OS account home and outside its .codex"]
async fn native_model_review_confirmation_submits_once() {
	tokio::time::timeout(Duration::from_secs(90), qualify_review_discovery(true, false))
		.await
		.expect("bounded native confirmation");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "run alone with isolated fixture HOME and DECODEX_TEST_CODEX_BINARY"]
async fn native_project_warning_does_not_block_ordinary_creation_and_survives_restart() {
	tokio::time::timeout(Duration::from_secs(90), qualify_review_discovery(true, true))
		.await
		.expect("native warning qualification");
}

async fn qualify_review_discovery(confirm: bool, warning: bool) {
	let home = isolated_home();
	let binary = std::env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit native binary");
	assert!(std::path::Path::new(&binary).is_absolute());
	let saved = home.join("saved-request");
	std::fs::create_dir_all(saved.join(".codex")).expect("saved working directory");
	std::fs::create_dir(home.join(".codex")).expect("isolated native home");
	let listener =
		tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("loopback-only provider");
	let address = listener.local_addr().expect("fixture address");
	let quoted = serde_json::to_string(saved.to_str().expect("saved cwd")).expect("quoted cwd");
	std::fs::write(home.join(".codex/config.toml"), format!(
        "model = \"gpt-6-astra\"\nmodel_reasoning_effort = \"low\"\nmodel_provider = \"fixture\"\nchatgpt_base_url = \"http://{address}\"\ncli_auth_credentials_store = \"file\"\n[model_providers.fixture]\nname = \"Model review fixture\"\nbase_url = \"http://{address}\"\nwire_api = \"responses\"\nrequires_openai_auth = true\nsupports_websockets = false\n[projects.{quoted}]\ntrust_level = \"trusted\"\n[features]\napps = false\nremote_plugins = false\n[analytics]\nenabled = false\n"
    )).expect("isolated native configuration");
	std::fs::write(
		saved.join(".codex/config.toml"),
		"model = \"gpt-5.6-sol\"\nmodel_reasoning_effort = \"high\"\n",
	)
	.expect("saved-directory native defaults");
	if warning {
		add_project_warning(&saved);
	}

	let root = DecodexRoot::new(home.join("product")).expect("product root");
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
	.expect("native attestation owner");
	accounts
		.attest_callback_capability(profile.account_callback_attestation())
		.await
		.expect("callback attestation");
	let revision = accounts.inspect(&account).await.expect("account observation").account.revision;
	let conversation = seed_review(&store, &account, revision, &saved).await;
	let runtime = runtime(&root, &store, accounts, profile).await;
	assert_runtime_review_boundary(&runtime, &conversation).await;
	let mut app = super::super::tests::application(store.clone());
	app.conversations = ConversationCapability::Ready(runtime);
	app.blob_store = Some(BlobStore::open(root.paths()).expect("fixture blob store"));
	let id = EntityId::new(conversation.as_str()).expect("conversation entity");
	assert!(
		app.saved_model_review_request(&conversation, EntityRevision(1)).await.is_some(),
		"saved request before native discovery"
	);
	let query = decodex_protocol::QueryEnvelope {
		version: CURRENT_VERSION,
		query_id: decodex_protocol::QueryId::new("native-review-discovery").expect("query"),
		payload: decodex_protocol::QueryPayload::GetConversationModelReview {
			conversation_id: id.clone(),
			expected_revision: EntityRevision(1),
		},
	};
	let result = tokio::select! {
		_ = crate::account_launch::serve_native_nudge_fixture(&listener, "200 OK",
			decodex_codex::app_server_client::AccountNudgeCreditType::Credits) => panic!("discovery cannot send a notification"),
		result = app.query(&query) => result,
	};
	let decodex_protocol::QueryResultPayload::ConversationModelReview(
		decodex_protocol::ConversationModelReviewResult::Available(review),
	) = result
	else {
		panic!("native review discovery must be available");
	};
	assert_eq!(review.message.as_str(), "Preserve the original request");
	assert_eq!(review.execution.model.as_str(), "gpt-6-astra");
	let decodex_protocol::InitialModelCatalogResult::Available {
		account_id,
		account_revision,
		working_directory,
		defaults,
		models,
	} = review.catalog
	else {
		panic!("complete native catalog");
	};
	assert_eq!(account_id.as_str(), account.as_str());
	assert_eq!(account_revision, revision);
	assert_eq!(working_directory.as_str(), saved.to_str().expect("saved cwd"));
	let defaults = defaults.expect("native defaults");
	assert_eq!(defaults.configured.model.expect("cwd model").as_str(), "gpt-5.6-sol");
	assert_eq!(defaults.configured.reasoning_effort.expect("cwd effort").as_str(), "high");
	assert!(!models.is_empty());
	assert_review_still_unstarted(&store, &conversation).await;
	assert_stale_review_unavailable(&app, query, id).await;

	assert!(
		tokio::time::timeout(Duration::from_millis(250), listener.accept()).await.is_err(),
		"discovery must not call inference"
	);
	if confirm {
		confirmation::qualify(&app, &listener, &conversation, account_id, revision).await;
	}
	app.begin_shutdown();
	app.wait_for_shutdown().await;
	if confirm {
		confirmation::assert_cold_readback(&root, &conversation).await;
		if warning {
			assert_cold_warning(&root, &conversation).await;
		}
	}
}

async fn assert_runtime_review_boundary(
	runtime: &ConversationRuntime,
	id: &decodex_core::ConversationId,
) {
	let outcome = runtime
		.resume_routing(crate::conversation::RecoverConversation {
			operation_key: "review-runtime-boundary".into(),
			correlation_id: "review-runtime-boundary".into(),
			causation_id: None,
			conversation_id: id.clone(),
			expected_conversation_revision: 1,
		})
		.await;
	assert!(matches!(outcome, crate::conversation::ConversationOutcome::PreSession(readback)
        if readback.state == crate::conversation::ConversationLocalState::ModelSettingsReviewRequired));
}

async fn assert_stale_review_unavailable(
	app: &crate::application::ServiceApplication,
	query: decodex_protocol::QueryEnvelope,
	id: EntityId,
) {
	let stale = decodex_protocol::QueryEnvelope {
		query_id: decodex_protocol::QueryId::new("stale-review-discovery").expect("stale query"),
		payload: decodex_protocol::QueryPayload::GetConversationModelReview {
			conversation_id: id,
			expected_revision: EntityRevision(2),
		},
		..query
	};
	assert!(matches!(
		app.query(&stale).await,
		decodex_protocol::QueryResultPayload::ConversationModelReview(
			decodex_protocol::ConversationModelReviewResult::Unavailable
		)
	));
}

fn add_project_warning(saved: &std::path::Path) {
	use std::io::Write as _;
	let mut file = std::fs::OpenOptions::new()
		.append(true)
		.open(saved.join(".codex/config.toml"))
		.expect("fixture project config");
	writeln!(file, "decodex_fixture_unknown_key = \"private-fixture-value\"")
		.expect("write fixture warning");
}

async fn assert_cold_warning(root: &DecodexRoot, conversation: &decodex_core::ConversationId) {
	let reopened = SqliteStore::open(&root.paths()).expect("reopened fixture store");
	let mut readback = super::super::tests::application(reopened);
	readback.blob_store = Some(BlobStore::open(root.paths()).expect("fixture blob store"));
	let decodex_protocol::ConversationHistoryResult::Page(page) = readback
		.conversation_history(
			&EntityId::new(conversation.as_str()).expect("fixture conversation ID"),
			None,
			decodex_protocol::MAX_HISTORY_PAGE_SIZE,
		)
		.await
	else {
		panic!("persisted ordinary history");
	};
	assert!(page.next_cursor.is_none(), "Fixture history must fit the checked page");
	let notices: Vec<_> = page
		.items
		.iter()
		.filter(|item| {
			item.payload
				.inline_text()
				.is_some_and(|text| text.as_str().contains("decodex_fixture_unknown_key"))
		})
		.collect();
	assert_eq!(notices.len(), 1, "Native startup warning must be stored exactly once");
	assert_eq!(notices[0].kind, decodex_protocol::HistoryItemKindDto::Status);
	assert_eq!(notices[0].turn_role, decodex_protocol::HistoryTurnRole::User);
	assert!(
		!notices[0]
			.payload
			.inline_text()
			.expect("inline warning text")
			.as_str()
			.contains("private-fixture-value")
	);
}

async fn seed_review(
	store: &SqliteStore,
	account: &AccountId,
	revision: i64,
	saved: &std::path::Path,
) -> decodex_core::ConversationId {
	let now = i64::try_from(
		std::time::SystemTime::now()
			.duration_since(std::time::UNIX_EPOCH)
			.expect("fixture clock")
			.as_micros(),
	)
	.expect("fixture timestamp");
	assert!(
		store
			.observe_account_usage(
				account,
				decodex_core::AccountUsageObservation {
					account_revision: revision,
					observed_at_unix_micros: now,
					ordinary_usage_allowed: Some(true),
					conditions: decodex_core::AccountUsageConditions {
						has_credits: None,
						unlimited_credits: None,
						spend_control_reached: Some(false),
						rate_limit_reached: Some(false),
					},
				},
				[None, None]
			)
			.await
			.expect("synthetic usage permission")
	);
	let control = store.read_account_routing_control().await.expect("routing control");
	assert!(matches!(
		store
			.set_fixed_account_selection(control.revision, account, revision)
			.await
			.expect("fixed fixture account"),
		decodex_database::RoutingControlOutcome::Updated { .. }
	));
	let id = decodex_core::ConversationId::new("50000000-0000-4000-8000-000000000001")
		.expect("conversation");
	store
		.create_conversation(
			&CommandIdentity::new("native-review-create", b"original source").expect("command"),
			&decodex_database::CreateConversationRecord {
				conversation_id: id.clone(),
				title: "Native model review".into(),
				message: "Preserve the original request".into(),
				working_directory: saved.display().to_string(),
				model: "gpt-6-astra".into(),
				reasoning_effort: Some("low".into()),
				fast: false,
				service_tier: None,
				initial_model_source: Some(decodex_database::InitialModelSource {
					account_id: account.clone(),
					account_revision: revision + 1,
				}),
			},
		)
		.await
		.expect("saved request");
	assert!(matches!(
		store
			.route_conversation_initial(
				"native-review-route",
				&decodex_database::RouteConversationInitial {
					conversation_id: id.clone(),
					expected_conversation_revision: 1,
				}
			)
			.await
			.expect("reject old catalog source"),
		decodex_database::ConversationInitialRouteOutcome::Rejected(rejection) if rejection.code == "initial_model_source_changed"
	));
	id
}

async fn assert_review_still_unstarted(store: &SqliteStore, id: &decodex_core::ConversationId) {
	let rows =
		store.read_ordinary_task_conversations(Some(id), None, 1).await.expect("saved projection");
	let decodex_database::OrdinaryTaskConversationProjection::Current(task) = &rows[0] else {
		panic!("active task");
	};
	assert!(task.runtime_session_id.is_none());
	assert!(
		!task.has_admitted_user_turn
			&& !task.has_active_provider_attempt
			&& !task.has_unknown_provider_attempt
	);
	assert_eq!(
		task.pre_session_state,
		Some(decodex_database::OrdinaryTaskPreSessionState::ModelSettingsReviewRequired)
	);
}

#[path = "application_model_review_confirmation_tests.rs"] mod confirmation;
