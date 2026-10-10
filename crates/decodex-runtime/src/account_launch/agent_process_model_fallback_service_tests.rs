//! Automatic fallback crosses the shared model journal and sends settings once.
use std::{
	sync::{
		Arc,
		atomic::{AtomicUsize, Ordering},
	},
	time::Duration,
};

use serde_json::{self, Value};
use tokio::{
	io::{self, AsyncBufReadExt as _, AsyncWriteExt as _, BufReader, DuplexStream},
	time,
};

use crate::{
	account_launch::agent_process::native_tests::reviewer::store::{
		AppServerClient, OwnedReviewer, SqliteStore,
	},
	agent_models,
};
use decodex_database::EnqueueAgentEvent;
use decodex_protocol::{AccountRecoveryResult, AccountRecoveryState, EntityId, EntityRevision};

fn recovery(account: EntityId, revision: EntityRevision, reserve: bool) -> AccountRecoveryResult {
	serde_json::from_value(serde_json::json!({"account_id":account,"account_revision":revision,"observed_at_unix_micros":1,"state":{"status":"current","banner":{"banner_type":"model_unavailable","title":"Model unavailable","description":"Use an ordinary alternative","reset_at":null,"model_slug":"original","blocked_model_slug":"original","fallback_model_slugs":[if reserve {"gpt-reserve"} else {"scoped"}],"dismissible":false,"actions":[],"request_url":null}}})).expect("decode recovery banner")
}

fn settings(model: &str) -> Value {
	serde_json::json!({"model":model,"modelProvider":"fixture","effort":"low","serviceTier":"priority","cwd":"/fixture","approvalPolicy":"on-request","approvalsReviewer":"user","sandboxPolicy":{"type":"readOnly"}})
}

#[tokio::test]
async fn automatic_fallback_service_preserves_source_settings_and_no_replay() {
	for mode in [
		"queued",
		"unknown",
		"rejected",
		"preserve-tier",
		"ultrafast-allowed",
		"ultrafast-default",
		"ultrafast-denied",
		"custom-auth",
		"stale-banner",
		"reserve",
		"source-before",
		"source-after",
		"banner-before",
		"banner-after",
		"explicit-input",
	] {
		time::timeout(Duration::from_secs(15), scenario(mode)).await.expect(mode);
	}
}

async fn scenario(mode: &'static str) {
	let home = tempfile::tempdir().expect("create fixture home");
	let (local, remote) = io::duplex(32_768);
	let (r, w) = io::split(local);
	let (client, mut events) = AppServerClient::from_io(r, w);
	let writes = Arc::new(AtomicUsize::new(0));
	let backend = tokio::spawn(serve(remote, writes.clone(), mode));

	client
		.thread_resume(serde_json::json!({"threadId":"thread"}))
		.await
		.expect("resume fixture thread");

	let owned = OwnedReviewer::new(home.path(), &client, "thread", "turn").await;

	owned
		.store
		.complete_agent_turn("root".into(), "turn".into())
		.await
		.expect("complete fixture turn");

	queue_explicit_model_input(&owned, mode).await;

	let source_reads = AtomicUsize::new(0);
	let source = || {
		let call = source_reads.fetch_add(1, Ordering::AcqRel);
		let owner = &owned;

		async move {
			let mut key = owner.key.clone();

			if (mode == "source-before" && call == 1) || (mode == "source-after" && call == 2) {
				key.revision += 1;
			}

			Some(owner.source(&key))
		}
	};
	let banner_reads = AtomicUsize::new(0);
	let banner = |account, revision| {
		let call = banner_reads.fetch_add(1, Ordering::AcqRel);
		let mut value = recovery(account, revision, mode == "reserve");

		if mode == "stale-banner"
			|| (mode == "banner-before" && call == 1)
			|| (mode == "banner-after" && call == 2)
		{
			value.state = AccountRecoveryState::Unavailable;
		}

		async move { value }
	};

	agent_models::recover_ordinary_model(&owned.store, &source, &banner, &events)
		.await
		.expect("evaluate fallback");

	let sent = matches!(
		mode,
		"queued"
			| "unknown"
			| "rejected"
			| "preserve-tier"
			| "ultrafast-allowed"
			| "ultrafast-default"
			| "ultrafast-denied"
	);

	assert_eq!(writes.load(Ordering::Acquire), usize::from(sent), "{mode}");

	let receipt = owned
		.store
		.agent_model_receipt("root".into(), "thread".into())
		.await
		.expect("read fallback receipt");
	let reserved = sent || matches!(mode, "source-after" | "banner-after");

	assert_eq!(receipt.is_some(), reserved, "{mode}");

	if reserved {
		let expected = if matches!(
			mode,
			"queued"
				| "preserve-tier"
				| "ultrafast-allowed"
				| "ultrafast-default"
				| "ultrafast-denied"
		) {
			"queued"
		} else if mode == "unknown" {
			"unknown"
		} else {
			"rejected"
		};

		assert_eq!(receipt.as_ref().expect("reserved receipt").state, expected, "{mode}");
		assert!(receipt.as_ref().expect("reserved recovery context").attempt.recovery.is_some());

		while events.try_recv().is_ok() {}

		agent_models::persist_current(
			&owned.store,
			&client,
			"thread",
			Some(owned.key.generation.as_str().into()),
		)
		.await
		.expect("persist native model publication");

		let expected = if expected == "queued" { "target_observed" } else { expected };
		let reopened = SqliteStore::open(&owned.root.paths()).expect("reopen fixture store");

		assert_eq!(
			reopened
				.agent_model_receipt("root".into(), "thread".into())
				.await
				.expect("read durable fallback receipt")
				.expect("durable receipt exists")
				.state,
			expected
		);

		agent_models::recover_ordinary_model(&reopened, &source, &banner, &events)
			.await
			.expect("evaluate fallback after reopen");

		assert_eq!(writes.load(Ordering::Acquire), usize::from(sent), "no retry for {mode}");
	}

	client.close();
	backend.abort();
}

async fn serve(remote: DuplexStream, writes: Arc<AtomicUsize>, mode: &str) {
	let (r, mut w) = io::split(remote);
	let tier = if mode.starts_with("ultrafast-") { "ultrafast" } else { "priority" };
	let settings_for = |model| {
		let mut value = settings(model);
		value["serviceTier"] = if mode == "ultrafast-default" && model == "original" {
			Value::Null
		} else {
			serde_json::json!(tier)
		};
		value
	};
	let mut lines = BufReader::new(r).lines();

	while let Some(line) = lines.next_line().await.expect("read native request") {
		let request: Value = serde_json::from_str(&line).expect("decode native request");
		let id = &request["id"];
		let reply = match request["method"].as_str().expect("native request method") {
			"thread/resume" => {
				let mut value = settings_for("original");

				value["thread"] = serde_json::json!({"id":"thread"});
				value["reasoningEffort"] = value["effort"].take();
				value["sandbox"] = value["sandboxPolicy"].clone();

				serde_json::json!({"id":id,"result":value})
			},
			"getAuthStatus" => {
				assert_eq!(
					request["params"],
					serde_json::json!({"includeToken":false,"refreshToken":false})
				);

				serde_json::json!({"id":id,"result":{"authMethod":"chatgpt","requiresOpenaiAuth":mode!="custom-auth","authToken":null}})
			},
			"model/list" =>
				serde_json::json!({"id":id,"result":{"data":[{"id":"scoped","model":"scoped","displayName":"Scoped","supportedReasoningEfforts":[{"reasoningEffort":"low"},{"reasoningEffort":"medium"}],"defaultReasoningEffort":"medium","serviceTiers":[{"id":tier}],"defaultServiceTier":tier}],"nextCursor":null}}),
			"experimentalFeature/list" =>
				serde_json::json!({"id":id,"result":{"data":[{"name":"fast_mode","enabled":!matches!(mode,"preserve-tier" | "ultrafast-allowed" | "ultrafast-default")},{"name":"ultrafast_mode","enabled":mode!="ultrafast-denied"}],"nextCursor":null}}),
			"thread/settings/update" => {
				writes.fetch_add(1, Ordering::AcqRel);

				let mut expected =
					serde_json::json!({"threadId":"thread","model":"scoped","effort":"low"});

				if !matches!(mode, "preserve-tier" | "ultrafast-denied") {
					expected["serviceTier"] = serde_json::json!(tier);
				}

				assert_eq!(request["params"], expected);

				match mode {
					"queued" | "preserve-tier" | "ultrafast-allowed" | "ultrafast-default"
					| "ultrafast-denied" => {
						let event = serde_json::json!({"method":"thread/settings/updated","params":{"threadId":"thread","threadSettings":settings_for("scoped")}});

						w.write_all(format!("{event}\n").as_bytes())
							.await
							.expect("write model publication");

						serde_json::json!({"id":id,"result":{}})
					},
					"rejected" =>
						serde_json::json!({"id":id,"error":{"code":-32_602,"message":"Rejected"}}),
					_ =>
						serde_json::json!({"id":id,"error":{"code":-32_001,"message":"Unknown delivery"}}),
				}
			},
			method => panic!("unexpected native method {method}"),
		};

		w.write_all(format!("{reply}\n").as_bytes()).await.expect("write native reply");
	}
}

async fn queue_explicit_model_input(owned: &OwnedReviewer, mode: &str) {
	if mode == "explicit-input" {
		owned
			.store
			.enqueue_agent_event(EnqueueAgentEvent {
				source_event_id: "manual-input".into(),
				work_item_id: "root".into(),
				event_kind: "user_message".into(),
				payload:
					serde_json::json!({"text":"later","options":{"execution":{"model":"manual"}}})
						.to_string(),
			})
			.await
			.expect("queue explicit model input");
	}
}
