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

pub(super) async fn check(
	client: &AgentClient,
	runtime: &ConversationRuntime,
	store: &SqliteStore,
	accounts: &AccountService,
	home: &Path,
	first: &AccountId,
	requests: &std::sync::atomic::AtomicUsize,
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

	let count = requests.load(Ordering::Acquire);
	let started = Instant::now();

	observe(accounts, first, 100).await;
	await_account(runtime, &second, &original.generation_id).await;

	let handover_ms = started.elapsed().as_millis();

	assert_eq!(settled(client).await, thread);
	assert_eq!(requests.load(Ordering::Acquire), count, "handover cannot replay earlier input");

	let started = Instant::now();
	let action = input("Continue once after account handover.");

	tokio::join!(
		accepted(client, action.clone(), "handover-followup"),
		accepted(client, action, "handover-followup"),
	);
	assert_eq!(settled(client).await, thread);
	assert_eq!(
		requests.load(Ordering::Acquire),
		count + 1,
		"concurrent duplicate commands must infer once"
	);

	let followup_ms = started.elapsed().as_millis();
	let bound = store.read_agent_process_binding("recap-root").await.unwrap().unwrap();

	assert_eq!(bound.account_id, second);

	let native = runtime.agent_client().unwrap();
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
