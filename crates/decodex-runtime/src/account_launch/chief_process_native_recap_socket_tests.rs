//! Real local socket, Chief host and installed Codex; only the model provider is synthetic.
use super::*;
#[path = "chief_process_native_app_ui_socket_tests.rs"] mod app_ui;
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
	if std::env::var("DECODEX_TEST_APP_UI").as_deref() == Ok("1") {
		app_ui::configure(home);
	}
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
	let runtime = runtime(&root, &store, accounts.clone(), profile).await;
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
	let outcome =
		std::panic::AssertUnwindSafe(tokio::time::timeout(Duration::from_secs(50), async {
			if std::env::var("DECODEX_TEST_APP_UI").as_deref() == Ok("1") {
				app_ui::check(&client, &runtime, home, &account, &requests).await;
			} else {
				check(&client, &runtime, &store, home, &account, &requests).await;
			}
			if std::env::var("DECODEX_TEST_ACCOUNT_ROTATION").as_deref() == Ok("1") {
				qualify_account_rotation(
					&client, &runtime, &store, &accounts, home, &account, &requests,
				)
				.await;
			}
		}))
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
	qualify_prompt_selection(&native, &thread, requests).await;
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
	assert!(client.recap(work.clone()).await.expect("cancelled query").recap.is_none());
	if std::env::var("DECODEX_TEST_PROMPT_REVERT").as_deref() == Ok("1") {
		qualify_native_prompt_revert(
			store,
			&native,
			work.as_str(),
			&thread,
			requests,
			client,
			home,
		)
		.await;
	}
}

async fn qualify_prompt_selection(
	client: &decodex_codex::app_server_client::AppServerClient,
	thread: &str,
	requests: &std::sync::atomic::AtomicUsize,
) {
	let headers = client.thread_turns_since(thread, None).await.expect("native turn headers");
	let latest = headers.last().expect("latest turn")["id"].as_str().expect("latest id");
	for header in [headers.first().expect("first turn"), headers.last().expect("last turn")] {
		let turn = header["id"].as_str().expect("turn id");
		let items = client.thread_read_turn_items(thread, turn).await.expect("native turn items");
		let input = items
			.as_array()
			.expect("items")
			.iter()
			.find(|item| item["type"] == "userMessage")
			.expect("first input");
		let item = input["id"].as_str().expect("input id");
		let selected = client
			.prompt_edit_candidate(thread, turn, item)
			.await
			.expect("native selection")
			.expect("editable first input");
		assert_eq!(selected.thread_id, thread);
		assert_eq!(selected.before_turn_id, turn);
		assert_eq!(selected.item_id, item);
		assert_eq!(selected.latest_turn_id, latest);
		assert_eq!(serde_json::Value::Array(selected.content), input["content"]);
		assert!(selected.guard.is_live());
	}
	assert_eq!(requests.load(Ordering::Acquire), 9, "prompt selection is read-only");
	assert_eq!(
		client.thread_latest_turn_id(thread).await.expect("unchanged native history").as_deref(),
		Some(latest)
	);
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

async fn qualify_native_prompt_revert(
	store: &SqliteStore,
	native: &decodex_codex::app_server_client::AppServerClient,
	work: &str,
	thread: &str,
	requests: &std::sync::atomic::AtomicUsize,
	client: &ChiefClient,
	home: &std::path::Path,
) {
	let before = native
		.request("thread/resume", json!({"threadId":thread,"excludeTurns":true}))
		.await
		.expect("native settings");
	let headers = native.thread_turns_since(thread, None).await.expect("all native turns");
	let selected = headers.last().expect("last input")["id"].as_str().expect("turn id");
	let items = native.thread_read_turn_items(thread, selected).await.expect("native input");
	let input = items
		.as_array()
		.expect("items")
		.iter()
		.find(|i| i["type"] == "userMessage")
		.expect("input");
	let work_id = EntityId::new(work).expect("work");
	let thread_id = WireText::new(thread).expect("thread");
	accepted(
		client,
		Action::PreparePromptEdit {
			work_id: work_id.clone(),
			thread_id: thread_id.clone(),
			turn_id: WireText::new(selected).expect("turn"),
			item_id: WireText::new(input["id"].as_str().expect("input id")).expect("item"),
		},
		"installed-edit-review",
	)
	.await;
	let (review, content) =
		client.prompt_edit(work_id.clone(), thread_id.clone()).await.expect("public review");
	assert_eq!(review.phase, decodex_protocol::PromptEditPhase::Review);
	let content = content.expect("canonical input");
	assert_eq!(content, input["content"].as_array().expect("canonical content").clone());
	let token = review.evidence.expect("review evidence").review_token;
	let count = requests.load(Ordering::Acquire);
	for key in ["installed-edit-confirm", "installed-edit-confirm-readback"] {
		accepted(
			client,
			Action::ConfirmPromptEdit {
				work_id: work_id.clone(),
				thread_id: thread_id.clone(),
				review_token: token.clone(),
			},
			key,
		)
		.await;
	}
	let (status, restored) =
		client.prompt_edit(work_id.clone(), thread_id.clone()).await.expect("public receipt");
	assert_eq!(status.phase, decodex_protocol::PromptEditPhase::Applied);
	assert_eq!(restored.as_ref(), Some(&content));
	let retained = native.thread_turns_since(thread, None).await.expect("retained history");
	assert_eq!(
		retained.iter().map(|t| t["id"].clone()).collect::<Vec<_>>(),
		headers[..headers.len() - 1].iter().map(|t| t["id"].clone()).collect::<Vec<_>>()
	);
	let after = native
		.request("thread/resume", json!({"threadId":thread,"excludeTurns":true}))
		.await
		.expect("retained settings");
	assert_eq!(before["model"], "cold-native-model");
	assert_eq!(after["thread"]["id"], before["thread"]["id"]);
	for field in [
		"model",
		"modelProvider",
		"reasoningEffort",
		"cwd",
		"approvalPolicy",
		"approvalsReviewer",
		"sandbox",
		"disabledPluginIds",
		"activePermissionProfile",
	] {
		assert_eq!(after[field], before[field], "preserve {field}");
	}
	// Fixed upstream ModelInfo::service_tier_for_request omits both null and default.
	// Native restoration can materialize the current step's default tier in resume metadata.
	let request_tier = |value: &serde_json::Value| {
		value.as_str().filter(|tier| *tier != "default").map(str::to_owned)
	};
	assert_eq!(request_tier(&after["serviceTier"]), request_tier(&before["serviceTier"]));
	assert_eq!(
		requests.load(Ordering::Acquire),
		count,
		"revert must not infer or send the restored draft"
	);
	assert!(
		store.begin_chief_dispatch(work.into()).await.is_err(),
		"desktop draft handback remains required"
	);
	accepted(
		client,
		Action::RecoverPromptEdit { work_id: work_id.clone(), thread_id: thread_id.clone() },
		"installed-edit-recover",
	)
	.await;
	let receipt_id = status.evidence.as_ref().unwrap().receipt_id.unwrap();
	qualify_prompt_acknowledgement(client, status, &content, home).await;
	assert_eq!(requests.load(Ordering::Acquire), count, "acknowledgement must not send the draft");
	let relative = decodex_protocol::PromptDraft::new(vec![
		json!({"type":"localImage","path":"images/photo.png","detail":"original"}),
	])
	.unwrap();
	let resolved = client
		.resolve_prompt_media(work_id.clone(), thread_id.clone(), &relative)
		.await
		.expect("owned native directory");
	assert_eq!(resolved.parts()[0]["path"], home.join("images/photo.png").to_str().unwrap());
	assert!(
		client
			.resolve_prompt_media(
				work_id.clone(),
				WireText::new("foreign-thread").unwrap(),
				&relative
			)
			.await
			.is_err()
	);
	assert_eq!(requests.load(Ordering::Acquire), count, "media directory queries do not infer");
	qualify_canonical_prompt_send(client, native, work_id, thread_id, receipt_id, requests).await;
}

async fn qualify_prompt_acknowledgement(
	client: &ChiefClient,
	status: decodex_protocol::PromptEditStatus,
	content: &[serde_json::Value],
	home: &std::path::Path,
) {
	let evidence = status.evidence.expect("edit receipt");
	let draft = home.join("saved-canonical-prompt.json");
	std::fs::write(&draft, serde_json::to_vec(content).expect("canonical bytes"))
		.expect("save client draft");
	std::fs::File::open(&draft).expect("saved draft").sync_all().expect("durable client draft");
	std::fs::File::open(home).expect("fixture directory").sync_all().expect("durable draft entry");
	let saved: Vec<serde_json::Value> =
		serde_json::from_slice(&std::fs::read(&draft).expect("saved bytes"))
			.expect("saved canonical input");
	assert_eq!(saved, content);
	let receipt_id = evidence.receipt_id.expect("durable receipt");
	for (id, key) in [
		(receipt_id + 1, "wrong-draft-receipt"),
		(receipt_id, "draft-saved"),
		(receipt_id, "draft-saved-readback"),
	] {
		let result = client
			.execute(
				Action::AcknowledgePromptEditDraft {
					work_id: status.work_id.clone(),
					thread_id: status.thread_id.clone(),
					receipt_id: id,
					review_token: evidence.review_token.clone(),
				},
				IdempotencyKey::new(key).expect("key"),
			)
			.await
			.expect("ack response");
		assert_eq!(matches!(result, ChiefCommandResponse::Accepted { .. }), id == receipt_id);
	}
	let (restored, _) =
		client.prompt_edit(status.work_id, status.thread_id).await.expect("released receipt");
	assert_eq!(restored.phase, decodex_protocol::PromptEditPhase::Restored);
}

async fn qualify_canonical_prompt_send(
	client: &ChiefClient,
	native: &decodex_codex::app_server_client::AppServerClient,
	work_id: EntityId,
	thread_id: WireText,
	receipt_id: i64,
	requests: &std::sync::atomic::AtomicUsize,
) {
	let text = format!("Edited canonical input: {} END-OF-FULL-INPUT", "x".repeat(70_000));
	let input = decodex_protocol::PromptDraft::new(vec![json!({"type":"text","text":text})])
		.expect("full input");
	let execution = decodex_protocol::ChiefExecutionOverrides::default();
	let count = requests.load(Ordering::Acquire);
	client
		.preflight_prompt_input(work_id.clone(), thread_id.clone(), &input, &execution)
		.await
		.expect("native envelope preflight");
	let upload = decodex_protocol::PromptInputUpload {
		work_id: work_id.clone(),
		thread_id: thread_id.clone(),
		edit_receipt_id: receipt_id,
		upload_id: IdempotencyKey::new("installed-native-edited-input").unwrap(),
		sha256: input.fingerprint().unwrap(),
		total_bytes: serde_json::to_vec(&input).unwrap().len() as u64,
	};
	let input_id =
		client.stage_prompt_input(upload.clone(), &input).await.expect("durable multichunk input");
	assert_eq!(client.stage_prompt_input(upload.clone(), &input).await.unwrap(), input_id);
	assert_eq!(requests.load(Ordering::Acquire), count, "staging must not submit input");
	let identity = decodex_protocol::PromptInputSendIdentity {
		work_id: work_id.clone(),
		thread_id: thread_id.clone(),
		edit_receipt_id: receipt_id,
		send: decodex_protocol::PromptInputSend {
			input_id,
			sha256: upload.sha256.clone(),
			command_key: IdempotencyKey::new("installed-edited-send").unwrap(),
			execution: execution.clone(),
		},
	};
	assert!(
		client
			.prompt_input_send_status(identity.clone())
			.await
			.unwrap()
			.accepted_event_id
			.is_none()
	);
	accepted(
		client,
		Action::SendPromptInput {
			work_id,
			thread_id: thread_id.clone(),
			input_id,
			edit_receipt_id: receipt_id,
			sha256: upload.sha256,
			execution,
		},
		"installed-edited-send",
	)
	.await;
	let accepted = client
		.prompt_input_send_status(identity.clone())
		.await
		.unwrap()
		.accepted_event_id
		.expect("exact queue receipt");
	assert_eq!(settled(client).await, thread_id.as_str());
	assert_eq!(
		requests.load(Ordering::Acquire),
		count + 1,
		"one explicit send performs one native turn"
	);
	let turn = native.thread_latest_turn_id(thread_id.as_str()).await.unwrap().unwrap();
	let items = native.thread_read_turn_items(thread_id.as_str(), &turn).await.unwrap();
	let user = items
		.as_array()
		.unwrap()
		.iter()
		.find(|item| item["type"] == "userMessage")
		.expect("new native input");
	assert_eq!(
		user["content"][0]["text"], text,
		"full canonical input must reach native history without preview truncation"
	);
	assert_eq!(
		client.prompt_input_send_status(identity).await.unwrap().accepted_event_id,
		Some(accepted)
	);
	assert_eq!(requests.load(Ordering::Acquire), count + 1, "receipt readback must not replay");
}

async fn qualify_account_rotation(
	client: &ChiefClient,
	runtime: &ConversationRuntime,
	store: &SqliteStore,
	accounts: &AccountService,
	home: &std::path::Path,
	first: &AccountId,
	requests: &std::sync::atomic::AtomicUsize,
) {
	let thread = settled(client).await;
	let original = store.read_chief_process_binding("recap-root").await.unwrap().unwrap();
	assert_eq!(&original.account_id, first);
	let second = enroll_numbered(store, accounts, home, 2).await;
	let count = requests.load(Ordering::Acquire);
	let observed =
		std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_micros()
			as i64;
	for (account, used) in [(&second, 0), (first, 100)] {
		for minutes in [300, 10080] {
			accounts
				.observe_quota(
					account,
					decodex_core::AccountQuotaWindow::new(minutes, used, observed + 3_600_000_000)
						.unwrap(),
					observed,
				)
				.await
				.unwrap();
		}
	}
	tokio::time::timeout(Duration::from_secs(20), async {
		loop {
			if runtime.chief_usage_source().await.is_some_and(|(generation, account, _, _)| {
				account == second && generation != original.generation_id
			}) {
				break;
			}
			tokio::time::sleep(Duration::from_millis(25)).await;
		}
	})
	.await
	.expect("exhausted native account rotates after positive process death");
	assert_eq!(settled(client).await, thread);
	assert_eq!(requests.load(Ordering::Acquire), count, "rotation must not replay parent input");
	accepted(
		client,
		Action::Send {
			root_id: EntityId::new("recap-root").unwrap(),
			text: HistoryText::new("Continue once after account rotation.").unwrap(),
		},
		"after-account-rotation",
	)
	.await;
	assert_eq!(settled(client).await, thread);
	assert_eq!(requests.load(Ordering::Acquire), count + 1);
	let binding = store.read_chief_process_binding("recap-root").await.unwrap().unwrap();
	assert_eq!(binding.account_id, second);
	let native = runtime.chief_client().unwrap();
	let turn = native.thread_latest_turn_id(&thread).await.unwrap().unwrap();
	let items = native.thread_read_turn_items(&thread, &turn).await.unwrap();
	assert!(items.as_array().unwrap().iter().any(|item| item["type"] == "userMessage"
		&& item["content"][0]["text"] == "Continue once after account rotation."));
}
