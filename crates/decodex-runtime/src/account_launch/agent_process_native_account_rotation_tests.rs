//! Exercise account handover through the public socket and the installed Codex runtime.
use super::{accepted, cold_settings, fixture_working_directory, qualify, start_recap_parent};
use crate::{account_service::AccountService, conversation::ConversationRuntime};
use decodex_core::{AccountId, AccountQuotaWindow, ProcessGenerationId};
use decodex_database::SqliteStore;
use decodex_protocol::{
	AgentActionDto, AgentClient, AgentDispatchStateDto, AgentExecutionOverrides,
	AgentSnapshotResult, EntityId, HistoryText,
};
use std::{
	env, fs,
	path::{Path, PathBuf},
	sync::atomic::{AtomicUsize, Ordering},
	time::{Instant, SystemTime, UNIX_EPOCH},
};
use tokio::time::{self, Duration};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "run alone with isolated HOME, DECODEX_TEST_ACCOUNT_HOME and DECODEX_TEST_CODEX_BINARY"]
async fn installed_account_handover_preserves_thread_and_deduplicates_messages() {
	let home = PathBuf::from(env::var_os("DECODEX_TEST_ACCOUNT_HOME").expect("isolated home"))
		.canonicalize()
		.expect("fixture home");

	assert_eq!(PathBuf::from(env::var_os("HOME").expect("HOME")), home);
	assert_eq!(
		fs::read_to_string(home.join(".decodex-recap-fixture")).expect("account handover fixture"),
		"isolated-recap\n"
	);
	assert!(!home.join(".codex").exists());

	time::timeout(Duration::from_secs(100), qualify(&home, true))
		.await
		.expect("bounded account handover qualification");
}

async fn observe(accounts: &AccountService, account: &AccountId, used: u8) {
	let now =
		SystemTime::now().duration_since(UNIX_EPOCH).expect("account handover fixture").as_micros()
			as i64;

	for minutes in [300, 10_080] {
		accounts
			.observe_quota(
				account,
				AccountQuotaWindow::new(minutes, used, now + 3_600_000_000)
					.expect("account handover fixture"),
				now,
			)
			.await
			.expect("account handover fixture");
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
		root_id: EntityId::new("recap-root").expect("account handover fixture"),
		text: HistoryText::new(text).expect("account handover fixture"),
		execution: AgentExecutionOverrides {
			service_tier: Some(
				decodex_protocol::ServiceTier::new("ultrafast").expect("account handover fixture"),
			),
			..Default::default()
		},
		attachments: Vec::new(),
		task_references: Vec::new(),
	}
}

fn set_tiers(home: &Path, tiers: &[&str]) {
	let path = home.join(".codex/models.json");
	let mut catalog: serde_json::Value =
		serde_json::from_slice(&fs::read(&path).expect("account handover fixture"))
			.expect("account handover fixture");
	catalog["models"][0]["service_tiers"] = serde_json::json!(
		tiers
			.iter()
			.map(
				|id| serde_json::json!({"id":id,"name":id,"description":"Synthetic account capability"})
			)
			.collect::<Vec<_>>()
	);
	fs::write(path, serde_json::to_vec(&catalog).expect("account handover fixture"))
		.expect("account handover fixture");
}

async fn settled(client: &AgentClient) -> String {
	loop {
		if let AgentSnapshotResult::Available(snapshot) =
			client.query().await.expect("account handover fixture")
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

pub(super) struct Scenario<'a> {
	pub client: &'a AgentClient,
	pub runtime: &'a ConversationRuntime,
	pub store: &'a SqliteStore,
	pub accounts: &'a AccountService,
	pub home: &'a Path,
	pub first: &'a AccountId,
	pub requests: &'a AtomicUsize,
	pub fault: &'a cold_settings::QuotaFault,
}

impl Scenario<'_> {
	pub(super) async fn check(self) {
		let (thread, second, original, initial_ms) = self.start().await;
		let (count, handover_ms, followup_ms, failed) =
			self.handover(&thread, &second, &original).await;
		self.verify_history(&thread, &failed).await;
		let (reconnect_ms, reconnected_followup_ms) = self.reconnect(&thread, &second, count).await;
		let refill_ms = self.refill(&thread, &second, count).await;
		let requests = self.requests;
		let report = serde_json::json!({
			"qualification_format":3,
			"provider":"local_synthetic",
			"native_runtime":"installed_codex",
			"initial_start_to_completion_ms":initial_ms,
			"exhaustion_to_new_account_bound_ms":handover_ms,
			"followup_to_completion_ms":followup_ms,
			"disconnect_to_same_account_bound_ms":reconnect_ms,
			"reconnected_followup_to_completion_ms":reconnected_followup_ms,
			"quota_refill_to_account_bound_ms":refill_ms,
			"exhausted_pool_preserves_queued_input":true,
			"sticky_account_with_ultrafast":true,
			"requested_speed":"ultrafast",
			"effective_speed_after_handover":"priority",
			"effective_speed_after_refill":"ultrafast",
			"native_speed_preference_preserved":true,
			"quota_error":"usageLimitExceeded",
			"quota_error_precedes_observation":true,
			"partial_output_preserved":true,
			"queued_during_exhaustion":true,
			"same_native_thread":true,
			"idempotent_delivery_submissions":6,
			"idempotent_delivery_unique_messages":2,
			"duplicate_submissions":4,
			"duplicate_inference_requests":0,
			"conversation_inference_requests":requests.load(Ordering::Acquire),
		});

		eprintln!("account_handover_report={report}");
		if let Some(path) = env::var_os("DECODEX_TEST_ACCOUNT_HANDOVER_REPORT") {
			fs::write(path, serde_json::to_vec_pretty(&report).expect("account handover fixture"))
				.expect("account handover fixture");
		}
	}

	async fn start(&self) -> (String, AccountId, ProcessGenerationId, u128) {
		let Self { client, store, accounts, home, first, .. } = *self;
		let second = cold_settings::enroll_numbered(store, accounts, home, 2).await;
		set_tiers(home, &["priority", "ultrafast"]);
		let config = home.join(".codex/config.toml");
		let native = fs::read_to_string(&config).expect("account handover fixture");
		fs::write(
			config,
			native.replace("[features]", "[features]\nfast_mode=true\nultrafast_mode=true"),
		)
		.expect("account handover fixture");

		observe(accounts, first, 80).await;
		observe(accounts, &second, 0).await;

		let started = Instant::now();
		accepted(
			client,
			start_recap_parent(
				&EntityId::new("recap-root").expect("account handover fixture"),
				&fixture_working_directory(),
				first,
			),
			"handover-start",
		)
		.await;
		let thread = settled(client).await;
		let initial_ms = started.elapsed().as_millis();
		let original = store
			.read_agent_process_binding("recap-root")
			.await
			.expect("account handover fixture")
			.expect("account handover fixture");

		assert_eq!(&original.account_id, first);
		accepted(
			client,
			input("Keep the same account despite the Ultrafast selection."),
			"sticky-ultrafast",
		)
		.await;
		assert_eq!(settled(client).await, thread);
		assert_eq!(
			store
				.read_agent_process_binding("recap-root")
				.await
				.expect("account handover fixture")
				.expect("account handover fixture"),
			original,
			"speed and a less utilized account must not change the route"
		);

		(thread, second, original.generation_id, initial_ms)
	}

	async fn handover(
		&self,
		thread: &str,
		second: &AccountId,
		original: &ProcessGenerationId,
	) -> (usize, u128, u128, String) {
		let Self { client, runtime, store, accounts, home, first, requests, fault } = *self;
		fault.pending.store(true, Ordering::Release);
		accepted(
			client,
			input("Exercise an interrupted response and preserve the partial result."),
			"quota-failure",
		)
		.await;
		fault.started.notified().await;
		let failed = store
			.get_agent_work_item("recap-root".into())
			.await
			.expect("account handover fixture")
			.active_turn_id
			.expect("account handover fixture");
		let count = requests.load(Ordering::Acquire);

		let action = input("Continue once after account handover.");

		tokio::join!(
			accepted(client, action.clone(), "handover-followup"),
			accepted(client, action, "handover-followup"),
		);
		self.verify_queued(count).await;

		let started = Instant::now();
		fault.release.notify_one();
		loop {
			let work = store
				.get_agent_work_item("recap-root".into())
				.await
				.expect("account handover fixture");
			if work.dispatch_state == decodex_database::AgentDispatchState::Idle {
				break;
			}
			time::sleep(Duration::from_millis(10)).await;
		}
		// This command is handled after the terminal event. A delayed quota read must
		// not let either terminal handling or the next command send to the old account.
		accepted(client, input("Continue once after account handover."), "handover-followup").await;
		self.verify_queued(count).await;
		set_tiers(home, &["priority"]);
		fault.expect_fast.store(true, Ordering::Release);
		observe(accounts, first, 100).await;
		await_account(runtime, second, original).await;
		let handover_ms = started.elapsed().as_millis();
		let continued = Instant::now();
		assert_eq!(settled(client).await, thread);
		assert_eq!(
			requests.load(Ordering::Acquire),
			count + 1,
			"concurrent duplicate commands must infer once"
		);

		let followup_ms = continued.elapsed().as_millis();
		let bound = store
			.read_agent_process_binding("recap-root")
			.await
			.expect("account handover fixture")
			.expect("account handover fixture");

		assert_eq!(&bound.account_id, second);

		(count, handover_ms, followup_ms, failed)
	}

	async fn verify_queued(&self, count: usize) {
		let Self { store, runtime, first, requests, .. } = *self;
		let queued = store
			.list_agent_wake_events("recap-root".into(), 100)
			.await
			.expect("account handover fixture");
		assert_eq!(
			queued
				.iter()
				.filter(
					|event| event.event_kind == "user_message" && event.delivered_turn_id.is_none()
				)
				.count(),
			1,
			"one queued message until the exhausted account is verified"
		);
		assert_eq!(
			runtime.agent_usage_source().await.expect("account handover fixture").1,
			*first,
			"keep the account until a newer quota observation permits handover"
		);
		assert_eq!(
			requests.load(Ordering::Acquire),
			count,
			"do not send queued input to the exhausted account"
		);
	}

	async fn verify_history(&self, thread: &str, failed: &str) {
		let Self { client, runtime, .. } = *self;
		let native = runtime.agent_client().expect("account handover fixture");
		let settings = native
			.thread_model_settings(
				thread,
				native.thread_settings_guard(thread).expect("account handover fixture"),
			)
			.await
			.expect("account handover fixture")
			.expect("account handover fixture");
		assert_eq!(
			settings.service_tier.as_deref(),
			Some("ultrafast"),
			"per-turn fallback must preserve the conversation preference"
		);
		let decodex_protocol::AgentCapabilitiesResult::Available { models, .. } =
			client.capabilities().await.expect("account handover fixture")
		else {
			panic!("current account catalog")
		};
		let model = models
			.iter()
			.find(|model| model.model.as_str() == "cold-native-model")
			.expect("account handover fixture");
		assert_eq!(
			model
				.effective_service_tier(
					&decodex_protocol::ServiceTier::new("ultrafast")
						.expect("account handover fixture")
				)
				.as_str(),
			"priority"
		);
		let failed_history =
			native.thread_read_turn(thread, failed).await.expect("account handover fixture");
		let failed_turn = failed_history["thread"]["turns"]
			.as_array()
			.expect("account handover fixture")
			.iter()
			.find(|turn| turn["id"] == failed)
			.expect("account handover fixture");
		assert_eq!(failed_turn["status"], "failed");
		assert_eq!(failed_turn["error"]["codexErrorInfo"], "usageLimitExceeded");
		assert!(
			failed_turn["items"]
				.as_array()
				.expect("account handover fixture")
				.iter()
				.any(|item| item["type"] == "agentMessage"
					&& item["text"] == "Partial result before quota exhaustion.")
		);
		let turn = native
			.thread_latest_turn_id(thread)
			.await
			.expect("account handover fixture")
			.expect("account handover fixture");
		let items =
			native.thread_read_turn_items(thread, &turn).await.expect("account handover fixture");

		assert!(
			items
				.as_array()
				.expect("account handover fixture")
				.iter()
				.any(|item| item["type"] == "userMessage"
					&& item["content"][0]["text"] == "Continue once after account handover.")
		);
	}

	async fn reconnect(&self, thread: &str, second: &AccountId, count: usize) -> (u128, u128) {
		let Self { client, runtime, store, requests, .. } = *self;
		let bound = store
			.read_agent_process_binding("recap-root")
			.await
			.expect("account handover fixture")
			.expect("account handover fixture");
		let started = Instant::now();

		runtime.close_agent_connection("recap-root").await.expect("account handover fixture");
		await_account(runtime, second, &bound.generation_id).await;

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
		let reconnected_followup_ms = started.elapsed().as_millis();
		(reconnect_ms, reconnected_followup_ms)
	}

	async fn refill(&self, thread: &str, second: &AccountId, count: usize) -> u128 {
		let Self { client, runtime, store, accounts, home, first, requests, fault } = *self;
		let active = store
			.read_agent_process_binding("recap-root")
			.await
			.expect("account handover fixture")
			.expect("account handover fixture");

		observe(accounts, second, 100).await;
		loop {
			if runtime.agent_usage_source().await.is_none() {
				break;
			}
			time::sleep(Duration::from_millis(10)).await;
		}
		let action = input("Continue once when quota becomes available again.");
		tokio::join!(
			accepted(client, action.clone(), "quota-refill-followup"),
			accepted(client, action, "quota-refill-followup")
		);
		assert_eq!(
			requests.load(Ordering::Acquire),
			count + 2,
			"an exhausted pool must keep the message local"
		);
		let queued = store
			.list_agent_wake_events("recap-root".into(), 100)
			.await
			.expect("account handover fixture");
		assert_eq!(
			queued
				.iter()
				.filter(
					|event| event.event_kind == "user_message" && event.delivered_turn_id.is_none()
				)
				.count(),
			1
		);
		let restored = Instant::now();
		set_tiers(home, &["priority", "ultrafast"]);
		fault.expect_fast.store(false, Ordering::Release);
		observe(accounts, first, 0).await;
		time::timeout(Duration::from_secs(3), await_account(runtime, first, &active.generation_id))
			.await
			.expect("committed quota availability must bypass retry backoff");
		let refill_ms = restored.elapsed().as_millis();
		assert_eq!(settled(client).await, thread);
		assert_eq!(requests.load(Ordering::Acquire), count + 3);

		refill_ms
	}
}
