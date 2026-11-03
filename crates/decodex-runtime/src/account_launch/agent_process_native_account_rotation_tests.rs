//! Exercise account handover through the public socket and the installed Codex runtime.
use std::time::Instant;

use super::*;
use decodex_protocol::AgentExecutionOverrides;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "run alone with isolated HOME, DECODEX_TEST_ACCOUNT_HOME and DECODEX_TEST_CODEX_BINARY"]
async fn installed_account_handover_preserves_thread_and_deduplicates_messages() {
	let home = PathBuf::from(env::var_os("DECODEX_TEST_ACCOUNT_HOME").expect("isolated home"))
		.canonicalize()
		.expect("fixture home");

	assert_eq!(PathBuf::from(env::var_os("HOME").expect("HOME")), home);
	assert_eq!(
		fs::read_to_string(home.join(".decodex-recap-fixture")).unwrap(),
		"isolated-recap\n"
	);
	assert!(!home.join(".codex").exists());

	time::timeout(Duration::from_secs(100), qualify(&home, true))
		.await
		.expect("bounded account handover qualification");
}

async fn observe(accounts: &AccountService, account: &AccountId, used: u8) {
	let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_micros() as i64;

	for minutes in [300, 10_080] {
		accounts
			.observe_quota(
				account,
				AccountQuotaWindow::new(minutes, used, now + 3_600_000_000).unwrap(),
				now,
			)
			.await
			.unwrap();
	}
}

async fn await_account(
	runtime: &ConversationRuntime,
	account: &AccountId,
	previous: &decodex_core::ProcessGenerationId,
) {
	loop {
		if runtime
			.agent_usage_source()
			.await
			.is_some_and(|(generation, source, _, _)| source == *account && generation != *previous)
		{
			return;
		}
		time::sleep(Duration::from_millis(10)).await;
	}
}

fn input(text: &str) -> AgentActionDto {
	AgentActionDto::SendConfigured {
		root_id: EntityId::new("recap-root").unwrap(),
		text: HistoryText::new(text).unwrap(),
		execution: AgentExecutionOverrides {
			service_tier: Some(decodex_protocol::ServiceTier::new("ultrafast").unwrap()),
			..Default::default()
		},
		attachments: Vec::new(),
		task_references: Vec::new(),
	}
}

async fn settled(client: &AgentClient) -> String {
	loop {
		if let AgentSnapshotResult::Available(snapshot) = client.query().await.unwrap()
			&& let Some(work) = snapshot.work_items.iter().find(|work| work.id == "recap-root")
			&& work.dispatch_state == AgentDispatchStateDto::Idle
			&& !snapshot.pending_events.iter().any(|event| {
				event.work_item_id == "recap-root"
					&& event.event_kind == "user_message"
					&& !event.delivery_claimed
			})
			&& let Some(thread) = &work.codex_thread_id
		{
			// Failed, already-delivered input remains readable in the inbox. It is not
			// an unsent message and must not prevent observing a later idle turn.
			return thread.clone();
		}
		time::sleep(Duration::from_millis(10)).await;
	}
}

pub(super) async fn check(
	client: &AgentClient,
	runtime: &ConversationRuntime,
	store: &SqliteStore,
	accounts: &AccountService,
	home: &Path,
	first: &AccountId,
	requests: &std::sync::atomic::AtomicUsize,
	fault: &cold_settings::QuotaFault,
) {
	let second = cold_settings::enroll_numbered(store, accounts, home, 2).await;

	observe(accounts, first, 80).await;
	observe(accounts, &second, 0).await;

	let started = Instant::now();
	accepted(
		client,
		start_recap_parent(
			&EntityId::new("recap-root").unwrap(),
			&fixture_working_directory(),
			first,
		),
		"handover-start",
	)
	.await;
	let thread = settled(client).await;
	let initial_ms = started.elapsed().as_millis();
	let original = store.read_agent_process_binding("recap-root").await.unwrap().unwrap();

	assert_eq!(&original.account_id, first);
	accepted(
		client,
		input("Keep the same account despite the Ultrafast selection."),
		"sticky-ultrafast",
	)
	.await;
	assert_eq!(settled(client).await, thread);
	assert_eq!(
		store.read_agent_process_binding("recap-root").await.unwrap().unwrap(),
		original,
		"speed and a less utilized account must not change the route"
	);

	fault.pending.store(true, Ordering::Release);
	accepted(
		client,
		input("Exercise an interrupted response and preserve the partial result."),
		"quota-failure",
	)
	.await;
	fault.started.notified().await;
	let failed =
		store.get_agent_work_item("recap-root".into()).await.unwrap().active_turn_id.unwrap();
	let count = requests.load(Ordering::Acquire);

	observe(accounts, first, 100).await;
	let action = input("Continue once after account handover.");

	tokio::join!(
		accepted(client, action.clone(), "handover-followup"),
		accepted(client, action, "handover-followup"),
	);
	let queued = store.list_agent_wake_events("recap-root".into(), 100).await.unwrap();
	assert_eq!(
		queued
			.iter()
			.filter(|event| event.event_kind == "user_message" && event.delivered_turn_id.is_none())
			.count(),
		1,
		"one queued message while the exhausted turn is still running"
	);
	assert_eq!(
		runtime.agent_usage_source().await.unwrap().1,
		*first,
		"a running turn keeps its account"
	);
	assert_eq!(
		requests.load(Ordering::Acquire),
		count,
		"do not send queued input to the exhausted account"
	);

	let started = Instant::now();
	fault.release.notify_one();
	await_account(runtime, &second, &original.generation_id).await;
	let handover_ms = started.elapsed().as_millis();
	let continued = Instant::now();
	assert_eq!(settled(client).await, thread);
	assert_eq!(
		requests.load(Ordering::Acquire),
		count + 1,
		"concurrent duplicate commands must infer once"
	);

	let followup_ms = continued.elapsed().as_millis();
	let bound = store.read_agent_process_binding("recap-root").await.unwrap().unwrap();

	assert_eq!(bound.account_id, second);

	let native = runtime.agent_client().unwrap();
	let failed_history = native.thread_read_turn(&thread, &failed).await.unwrap();
	let failed_turn = failed_history["thread"]["turns"]
		.as_array()
		.unwrap()
		.iter()
		.find(|turn| turn["id"] == failed)
		.unwrap();
	assert_eq!(failed_turn["status"], "failed");
	assert_eq!(failed_turn["error"]["codexErrorInfo"], "usageLimitExceeded");
	assert!(
		failed_turn["items"].as_array().unwrap().iter().any(|item| item["type"] == "agentMessage"
			&& item["text"] == "Partial result before quota exhaustion.")
	);
	let turn = native.thread_latest_turn_id(&thread).await.unwrap().unwrap();
	let items = native.thread_read_turn_items(&thread, &turn).await.unwrap();

	assert!(items.as_array().unwrap().iter().any(|item| item["type"] == "userMessage"
		&& item["content"][0]["text"] == "Continue once after account handover."));

	let started = Instant::now();

	runtime.close_agent_connection("recap-root").await.unwrap();
	await_account(runtime, &second, &bound.generation_id).await;

	let reconnect_ms = started.elapsed().as_millis();

	assert_eq!(settled(client).await, thread);
	accepted(client, input("Continue once after account handover."), "handover-followup").await;
	assert_eq!(
		requests.load(Ordering::Acquire),
		count + 1,
		"reconnecting cannot replay a delivered command"
	);

	let started = Instant::now();

	accepted(
		client,
		input("Continue after reconnect without repeating the earlier request."),
		"reconnected-followup",
	)
	.await;
	assert_eq!(settled(client).await, thread);
	assert_eq!(requests.load(Ordering::Acquire), count + 2);

	let report = serde_json::json!({
		"provider":"local_synthetic",
		"native_runtime":"installed_codex",
		"initial_start_to_completion_ms":initial_ms,
		"exhaustion_to_new_account_bound_ms":handover_ms,
		"followup_to_completion_ms":followup_ms,
		"disconnect_to_same_account_bound_ms":reconnect_ms,
		"reconnected_followup_to_completion_ms":started.elapsed().as_millis(),
		"sticky_account_with_ultrafast":true,
		"quota_error":"usageLimitExceeded",
		"partial_output_preserved":true,
		"queued_during_exhaustion":true,
		"same_native_thread":true,
		"duplicate_messages_submitted":3,
		"duplicate_inference_requests":0,
		"conversation_inference_requests":requests.load(Ordering::Acquire),
	});

	eprintln!("account_handover_report={report}");
	if let Some(path) = env::var_os("DECODEX_TEST_ACCOUNT_HANDOVER_REPORT") {
		fs::write(path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
	}
}
