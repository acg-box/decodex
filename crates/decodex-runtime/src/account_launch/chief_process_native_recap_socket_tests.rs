//! Real local socket, Chief host and installed Codex; only the model provider is synthetic.
use super::*;
use crate::{ProtocolServer, ServerConfig};
use decodex_protocol::{
	ChiefActionDto as Action, ChiefClient, ChiefCommandResponse, ChiefDispatchStateDto,
	ChiefSandboxDto, ChiefSnapshotResult, ChiefStartDto, ClientProfile, ConversationModel,
	ConversationReasoningEffort, ConversationWorkingDirectory, EntityId, HistoryText,
	IdempotencyKey, LocalTransportAuthority, ServerId, TaskRecapPhase as Phase, WireText,
};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "run alone with isolated HOME, DECODEX_TEST_ACCOUNT_HOME and DECODEX_TEST_CODEX_BINARY"]
async fn installed_recap_public_socket_preserves_parent_and_exact_request_identity() {
	let home = std::path::PathBuf::from(
		std::env::var_os("DECODEX_TEST_ACCOUNT_HOME").expect("isolated home"),
	)
	.canonicalize()
	.expect("fixture home");
	assert_eq!(std::path::PathBuf::from(std::env::var_os("HOME").expect("HOME")), home);
	assert_eq!(
		std::fs::read_to_string(home.join(".decodex-recap-fixture"))
			.expect("explicit fixture marker"),
		"isolated-recap\n"
	);
	assert!(!home.join(".codex").exists());
	tokio::time::timeout(Duration::from_secs(90), qualify(&home))
		.await
		.expect("bounded real service fixture");
}

async fn qualify(home: &std::path::Path) {
	let native_home = home.join(".codex");
	std::fs::create_dir(&native_home).expect("fixture native home");
	let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("loopback provider");
	let address = listener.local_addr().expect("loopback address");
	let requests = Arc::new(std::sync::atomic::AtomicUsize::new(0));
	let metadata = Arc::new(std::sync::atomic::AtomicUsize::new(0));
	let backend = tokio::spawn(serve(
		listener,
		requests.clone(),
		metadata,
		Some("Spoken fixture correction"),
	));
	let catalog = native_home.join("models.json");
	std::fs::write(
		&catalog,
		serde_json::to_vec(
			&json!({"models":[effort::fixture_model("cold-native-model", "provider-effort")]}),
		)
		.expect("catalog"),
	)
	.expect("catalog");
	std::fs::write(native_home.join("config.toml"), format!("model=\"cold-native-model\"\nmodel_reasoning_effort=\"provider-effort\"\nmodel_catalog_json={}\nmodel_provider=\"fixture\"\nchatgpt_base_url=\"http://{address}/backend-api\"\ncli_auth_credentials_store=\"file\"\n[features]\nenable_request_compression=false\napps=false\nremote_plugins=false\n[analytics]\nenabled=false\n[model_providers.fixture]\nname=\"Isolated fixture\"\nbase_url=\"http://{address}\"\nwire_api=\"responses\"\nrequires_openai_auth=true\nsupports_websockets=false\n", json!(catalog))).expect("fixture config");
	let root = DecodexRoot::new(home.join("product")).expect("fixture root");
	root.paths().ensure_layout().expect("private layout");
	let store = SqliteStore::open(&root.paths()).expect("product database");
	let accounts = Arc::new(AccountService::new(
		store.clone(),
		Arc::new(SqliteCredentialStore::new(store.clone())),
		Arc::new(NoRefresh),
	));
	let account = enroll(&store, &accounts, home).await;
	let observed = std::time::SystemTime::now()
		.duration_since(std::time::UNIX_EPOCH)
		.expect("fixture clock")
		.as_micros() as i64;
	for minutes in [300, 10080] {
		accounts
			.observe_quota(
				&account,
				decodex_core::AccountQuotaWindow::new(minutes, 0, observed + 3_600_000_000)
					.expect("synthetic available quota"),
				observed,
			)
			.await
			.expect("fresh fixture quota");
	}
	let directory = home.to_owned();
	let profile = tokio::task::spawn_blocking(move || {
		crate::account_launch::AttestedAppServerProfile::attest(directory, Duration::from_secs(15))
	})
	.await
	.expect("profile task")
	.expect("installed profile");
	accounts
		.attest_callback_capability(profile.account_callback_attestation())
		.await
		.expect("native callback attestation");
	let runtime = runtime(&root, &store, accounts, profile).await;
	let server_id = ServerId::new("20000000-0000-4000-8000-000000000001").expect("server id");
	let authority = || {
		LocalTransportAuthority::new(
			root.paths(),
			decodex_core::LocalTrustPolicy::SameUid,
			Some(unsafe { libc::geteuid() }),
		)
		.expect("same-UID authority")
	};
	let uid = unsafe { libc::geteuid() };
	let config = root.as_path().join("config.toml");
	std::fs::write(&config,format!("version = 1\nactive_profile = \"local\"\ncache = {{}}\n[profiles.local]\nkind = \"local\"\npolicy = \"same_uid\"\nservice_owner_uid = {uid}\nexpected_server_identity = \"20000000-0000-4000-8000-000000000001\"\n")).expect("local profile");
	use std::os::unix::fs::PermissionsExt as _;
	std::fs::set_permissions(config, std::fs::Permissions::from_mode(0o600))
		.expect("private profile");
	let client = ChiefClient::new(ClientProfile::load(root.as_path(), None).expect("local client"));
	let app = submit::application(&runtime, &store, home);
	let mut server = ProtocolServer::new(server_id, app, ServerConfig::default())
		.bind(authority())
		.await
		.expect("public local server");
	use futures_util::FutureExt as _;
	let outcome = std::panic::AssertUnwindSafe(tokio::time::timeout(
		Duration::from_secs(50),
		check(&client, &runtime, &store, home, &account, &requests),
	))
	.catch_unwind()
	.await;
	assert!(server.shutdown().await.expect("service shutdown").is_success());
	backend.abort();
	outcome.expect("fixture assertions").expect("bounded command checks");
}

async fn check(
	client: &ChiefClient,
	runtime: &ConversationRuntime,
	store: &SqliteStore,
	home: &std::path::Path,
	account: &AccountId,
	requests: &std::sync::atomic::AtomicUsize,
) {
	let work = EntityId::new("recap-root").expect("work");
	assert_eq!(client.recap(work.clone()).await.expect("cold query").phase, Phase::Idle);
	assert_eq!(requests.load(Ordering::Acquire), 0, "queries must not infer");
	accepted(
		client,
		Action::Start(ChiefStartDto {
			root_id: work.clone(),
			prompt: HistoryText::new("Test the fix. Do not install it.").expect("prompt"),
			model: ConversationModel::new("cold-native-model").expect("model"),
			effort: Some(ConversationReasoningEffort::new("provider-effort").expect("effort")),
			cwd: ConversationWorkingDirectory::new(home.to_str().expect("directory"))
				.expect("directory"),
			account_id: Some(EntityId::new(account.as_str()).expect("account")),
			sandbox: ChiefSandboxDto::ReadOnly,
		}),
		"recap-parent",
	)
	.await;
	let thread = settled(client).await;
	qualify_completed_progress(client, &work, &thread, account, requests).await;
	let native = runtime.chief_client().expect("active native client");
	let before = native.thread_latest_turn_id(&thread).await.expect("native parent turn");
	prepare_voice_call(client, runtime, store, &work, &thread, before.clone()).await;
	assert_eq!(requests.load(Ordering::Acquire), 9, "active voice must not infer a recap");
	let automatic = qualify_desktop_recap(client, store, home, &work).await;
	let generate = Action::GenerateRecap {
		work_id: work.clone(),
		thread_id: WireText::new(&thread).expect("native thread"),
	};
	accepted(client, generate.clone(), "recap-one").await;
	wait_phase(client, &work, Phase::Ready).await;
	accepted(client, generate.clone(), "recap-one").await;
	let state = client.recap(work.clone()).await.expect("ready result");
	assert!(
		state.recap.expect("summary").summary.as_str().contains("installation is still pending")
	);
	assert_eq!(state.request_id.as_ref().map(WireText::as_str), Some("recap-one"));
	assert_eq!(
		requests.load(Ordering::Acquire),
		10 + automatic,
		"same-key socket retry must not infer again"
	);
	assert_eq!(native.thread_latest_turn_id(&thread).await.expect("parent history"), before);
	store
		.record_chief_voice_transcript(
			"recap-voice".into(),
			36,
			"user".into(),
			"Late spoken correction: do not publish.".into(),
			true,
		)
		.await
		.expect("late caption");
	assert_eq!(
		client.recap(work.clone()).await.expect("voice version read").phase,
		Phase::Cancelled
	);
	assert_eq!(
		requests.load(Ordering::Acquire),
		10 + automatic,
		"voice version query is read-only"
	);
	accepted(client, generate.clone(), "recap-voice-refresh").await;
	wait_phase(client, &work, Phase::Ready).await;
	accepted(
		client,
		Action::Send {
			root_id: work.clone(),
			text: HistoryText::new("New correction: keep installation pending.")
				.expect("correction"),
		},
		"recap-new-input",
	)
	.await;
	wait_phase(client, &work, Phase::Cancelled).await;
	settled(client).await;
	accepted(client, generate, "recap-two").await;
	accepted(
		client,
		Action::CancelRecap {
			work_id: work.clone(),
			request_id: WireText::new("recap-one").expect("old key"),
		},
		"recap-stale-cancel",
	)
	.await;
	assert_ne!(client.recap(work.clone()).await.expect("current request").phase, Phase::Cancelled);
	accepted(
		client,
		Action::CancelRecap {
			work_id: work.clone(),
			request_id: WireText::new("recap-two").expect("current key"),
		},
		"recap-cancel",
	)
	.await;
	wait_phase(client, &work, Phase::Cancelled).await;
	assert!(client.recap(work).await.expect("cancelled query").recap.is_none());
}

async fn qualify_desktop_recap(
	client: &ChiefClient,
	store: &SqliteStore,
	home: &std::path::Path,
	work: &EntityId,
) -> usize {
	let Some(binary) = std::env::var_os("DECODEX_TEST_RECAP_GUI_BINARY") else {
		return 0;
	};
	assert!(std::path::Path::new(&binary).is_absolute());
	let settings = store.read_desktop_settings().await.expect("fixture preferences");
	assert!(!settings.auto_recap, "fixture starts with automatic recaps disabled");
	let enabled = store
		.set_desktop_settings(settings.revision, settings.show_in_menu_bar, None, Some(true))
		.await
		.expect("enable only disposable fixture preference");
	let screenshot = home.join("automatic-recap.png");
	let log = home.join("automatic-recap-capture.log");
	let stdout = std::fs::File::create(&log).expect("capture diagnostics");
	let stderr = stdout.try_clone().expect("shared capture log");
	let mut child = tokio::process::Command::new(binary)
		.env("DECODEX_VISUAL_CHIEF_ROOT", home.join("product"))
		.env("DECODEX_VISUAL_CHIEF_WORK", work.as_str())
		.env("DECODEX_VISUAL_AUTO_RECAP", "1")
		.env("DECODEX_VISUAL_OUTPUT", &screenshot)
		.stdout(stdout)
		.stderr(stderr)
		.kill_on_drop(true)
		.spawn()
		.expect("desktop capture process");
	std::fs::write(home.join("automatic-recap.pid"), child.id().expect("child PID").to_string())
		.expect("child identity");
	let status = tokio::time::timeout(Duration::from_secs(30), child.wait())
		.await
		.expect("bounded desktop capture")
		.expect("desktop capture exit");
	assert!(status.success(), "desktop capture failed; inspect {}", log.display());
	let evidence: serde_json::Value = serde_json::from_slice(
		&std::fs::read(screenshot.with_extension("recap.json")).expect("desktop evidence"),
	)
	.expect("desktop JSON");
	assert_eq!(evidence["automatic"], true);
	assert_eq!(evidence["state"]["phase"], "ready");
	assert_eq!(evidence["completed_turns"].as_array().expect("native progress").len(), 3);
	let ready = client.recap(work.clone()).await.expect("public automatic result");
	assert_eq!(serde_json::to_value(ready).expect("result JSON"), evidence["state"]);
	assert!(screenshot.is_file());
	let disabled = store
		.set_desktop_settings(enabled.revision, settings.show_in_menu_bar, None, Some(false))
		.await
		.expect("restore fixture opt-out");
	assert!(!disabled.auto_recap);
	1
}

async fn qualify_completed_progress(
	client: &ChiefClient,
	work: &EntityId,
	thread: &str,
	account: &AccountId,
	requests: &std::sync::atomic::AtomicUsize,
) {
	use decodex_protocol::{ChiefTimelineContent, ChiefTimelineResult};
	for index in 1..=8 {
		accepted(
			client,
			Action::Send {
				root_id: work.clone(),
				text: HistoryText::new(format!("Validate step {index}; do not install."))
					.expect("input"),
			},
			&format!("progress-{index}"),
		)
		.await;
		assert_eq!(settled(client).await, thread);
	}
	assert_eq!(requests.load(Ordering::Acquire), 9, "only parent turns inferred");
	let mut cursor = None;
	let mut completed = std::collections::BTreeSet::new();
	let mut seen = std::collections::BTreeSet::new();
	let mut pages = 0;
	loop {
		let result = client
			.timeline(work.clone(), EntityId::new(thread).expect("thread"), cursor)
			.await
			.expect("native progress query");
		let ChiefTimelineResult::Available { work_id, account_id, page } = result else {
			panic!("native progress unavailable: {result:?}")
		};
		assert_eq!(&work_id, work);
		assert_eq!(account_id.as_str(), account.as_str());
		assert_eq!(page.thread_id, thread);
		for entry in page.entries {
			if let ChiefTimelineContent::TurnBoundary {
				turn_id,
				completed: true,
				status: Some(status),
				..
			} = entry.content
			{
				assert_eq!(status, "completed");
				completed.insert(turn_id);
			}
		}
		pages += 1;
		let Some(next) = page.next_cursor else { break };
		assert!(pages < 8 && seen.insert(next.clone()), "bounded distinct cursors");
		cursor = Some(WireText::new(next).expect("cursor"));
	}
	assert!(pages >= 2, "qualify native pagination");
	assert_eq!(completed.len(), 9, "exact successful turn identities");
	assert!(matches!(
		client
			.timeline(work.clone(), EntityId::new("other-thread").expect("other identity"), None)
			.await
			.expect("wrong binding read"),
		ChiefTimelineResult::Unavailable
	));
	assert_eq!(requests.load(Ordering::Acquire), 9, "progress reads must not infer");
}

async fn accepted(client: &ChiefClient, action: Action, key: &str) {
	let response = client
		.execute(action, IdempotencyKey::new(key).expect("command key"))
		.await
		.expect("public command response");
	assert!(
		matches!(response, ChiefCommandResponse::Accepted { .. }),
		"public acceptance: {response:?}"
	);
}
async fn settled(client: &ChiefClient) -> String {
	loop {
		if let ChiefSnapshotResult::Available(snapshot) =
			client.query().await.expect("public snapshot")
			&& let Some(work) = snapshot.work_items.iter().find(|w| w.id == "recap-root")
			&& !snapshot
				.pending_events
				.iter()
				.any(|e| e.work_item_id == "recap-root" && e.event_kind == "user_message")
			&& work.dispatch_state == ChiefDispatchStateDto::Idle
			&& let Some(thread) = &work.codex_thread_id
		{
			return thread.clone();
		}
		tokio::time::sleep(Duration::from_millis(20)).await;
	}
}
async fn wait_phase(client: &ChiefClient, work: &EntityId, phase: Phase) {
	loop {
		let state = client.recap(work.clone()).await.expect("public recap status");
		if state.phase == phase {
			return;
		}
		assert_ne!(state.phase, Phase::Failed, "native recap failed: {state:?}");
		tokio::time::sleep(Duration::from_millis(20)).await;
	}
}

async fn prepare_voice_call(
	client: &ChiefClient,
	runtime: &ConversationRuntime,
	store: &SqliteStore,
	work: &EntityId,
	thread: &str,
	baseline: Option<String>,
) {
	let (generation, _, _, _) = runtime.chief_usage_source().await.expect("voice source");
	store
		.begin_chief_voice_call(decodex_database::ChiefVoiceCall {
			session_id: "recap-voice".into(),
			work_id: work.as_str().into(),
			thread_id: thread.into(),
			generation_id: generation.as_str().into(),
			baseline_turn_id: baseline.clone(),
		})
		.await
		.expect("authorized voice call");
	for sequence in 1..=35 {
		let text = if sequence == 35 {
			"Spoken fixture correction: keep deployment paused.".into()
		} else {
			format!("Earlier spoken sentence {sequence}")
		};
		store
			.record_chief_voice_transcript(
				"recap-voice".into(),
				sequence,
				"user".into(),
				text,
				true,
			)
			.await
			.expect("visible voice input");
	}
	let history = store
		.read_chief_voice_history(work.as_str().into(), thread.into())
		.await
		.expect("voice source read");
	assert_eq!(history.calls.len(), 1);
	assert_eq!(history.calls[0].entries.len(), 32);
	assert_eq!(history.calls[0].entries[0].sequence, 4);
	assert_eq!(history.calls[0].entries[31].sequence, 35);
	assert_eq!(history.calls[0].baseline_turn_id, baseline);
	assert!(history.truncated);
	assert_eq!(history.revision.open_calls, 1);
	assert!(
		store
			.read_chief_voice_history(work.as_str().into(), "another-thread".into())
			.await
			.expect("other source")
			.calls
			.is_empty()
	);
	let response = client
		.execute(
			Action::GenerateRecap {
				work_id: work.clone(),
				thread_id: WireText::new(thread).expect("thread"),
			},
			IdempotencyKey::new("recap-during-voice").expect("key"),
		)
		.await
		.expect("voice overlap response");
	assert!(matches!(response, ChiefCommandResponse::Rejected { .. }));
	store.close_chief_voice_call("recap-voice".into()).await.expect("voice call closed");
}
