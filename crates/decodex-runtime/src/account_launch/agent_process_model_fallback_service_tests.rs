//! Automatic fallback crosses the shared model journal and sends settings once.
use super::*;

use serde_json::{Value, json};

use std::sync::{
	Arc,
	atomic::{AtomicUsize, Ordering},
};

use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};

fn recovery(
	account: decodex_protocol::EntityId,
	revision: decodex_protocol::EntityRevision,
	reserve: bool,
) -> decodex_protocol::AccountRecoveryResult {
	serde_json::from_value(json!({"account_id":account,"account_revision":revision,"observed_at_unix_micros":1,"state":{"status":"current","banner":{"banner_type":"model_unavailable","title":"Model unavailable","description":"Use an ordinary alternative","reset_at":null,"model_slug":"original","blocked_model_slug":"original","fallback_model_slugs":[if reserve {"gpt-reserve"} else {"scoped"}],"dismissible":false,"actions":[],"request_url":null}}})).expect("decode recovery banner")
}

fn settings(model: &str) -> Value {
	json!({"model":model,"modelProvider":"fixture","effort":"low","serviceTier":"priority","cwd":"/fixture","approvalPolicy":"on-request","approvalsReviewer":"user","sandboxPolicy":{"type":"readOnly"}})
}

#[tokio::test]
async fn automatic_fallback_service_preserves_source_settings_and_no_replay() {
	for mode in [
		"queued",
		"unknown",
		"rejected",
		"preserve-tier",
		"custom-auth",
		"stale-banner",
		"reserve",
		"source-before",
		"source-after",
		"banner-before",
		"banner-after",
		"explicit-input",
	] {
		tokio::time::timeout(std::time::Duration::from_secs(15), scenario(mode)).await.expect(mode);
	}
}

async fn scenario(mode: &'static str) {
	let home = tempfile::tempdir().expect("create fixture home");
	let (local, remote) = tokio::io::duplex(32_768);
	let (r, w) = tokio::io::split(local);
	let (client, mut events) = AppServerClient::from_io(r, w);
	let writes = Arc::new(AtomicUsize::new(0));
	let backend = tokio::spawn(serve(remote, writes.clone(), mode));

	client.thread_resume(json!({"threadId":"thread"})).await.expect("resume fixture thread");

	let owned = OwnedReviewer::new(home.path(), &client, "thread", "turn").await;

	owned
		.store
		.complete_agent_turn("root".into(), "turn".into())
		.await
		.expect("complete fixture turn");

	if mode == "explicit-input" {
		owned
			.store
			.enqueue_agent_event(decodex_database::EnqueueAgentEvent {
				source_event_id: "manual-input".into(),
				work_item_id: "root".into(),
				event_kind: "user_message".into(),
				payload: json!({"text":"later","options":{"execution":{"model":"manual"}}})
					.to_string(),
			})
			.await
			.expect("queue explicit model input");
	}

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
			value.state = decodex_protocol::AccountRecoveryState::Unavailable;
		}

		async move { value }
	};

	crate::agent_models::recover_ordinary_model(&owned.store, &source, &banner, &events)
		.await
		.expect("evaluate fallback");

	let sent = matches!(mode, "queued" | "unknown" | "rejected" | "preserve-tier");

	assert_eq!(writes.load(Ordering::Acquire), usize::from(sent), "{mode}");

	let receipt = owned
		.store
		.agent_model_receipt("root".into(), "thread".into())
		.await
		.expect("read fallback receipt");
	let reserved = sent || matches!(mode, "source-after" | "banner-after");

	assert_eq!(receipt.is_some(), reserved, "{mode}");

	if reserved {
		let expected = if matches!(mode, "queued" | "preserve-tier") {
			"queued"
		} else if mode == "unknown" {
			"unknown"
		} else {
			"rejected"
		};

		assert_eq!(receipt.as_ref().expect("reserved receipt").state, expected, "{mode}");
		assert!(receipt.as_ref().expect("reserved recovery context").attempt.recovery.is_some());

		while events.try_recv().is_ok() {}

		crate::agent_models::persist_current(
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

		crate::agent_models::recover_ordinary_model(&reopened, &source, &banner, &events)
			.await
			.expect("evaluate fallback after reopen");

		assert_eq!(writes.load(Ordering::Acquire), usize::from(sent), "no retry for {mode}");
	}

	client.close();
	backend.abort();
}

async fn serve(remote: tokio::io::DuplexStream, writes: Arc<AtomicUsize>, mode: &str) {
	let (r, mut w) = tokio::io::split(remote);
	let mut lines = BufReader::new(r).lines();

	while let Some(line) = lines.next_line().await.expect("read native request") {
		let request: Value = serde_json::from_str(&line).expect("decode native request");
		let id = &request["id"];
		let reply = match request["method"].as_str().expect("native request method") {
			"thread/resume" => {
				let mut value = settings("original");

				value["thread"] = json!({"id":"thread"});
				value["reasoningEffort"] = value["effort"].take();
				value["sandbox"] = value["sandboxPolicy"].clone();

				json!({"id":id,"result":value})
			},
			"getAuthStatus" => {
				assert_eq!(request["params"], json!({"includeToken":false,"refreshToken":false}));

				json!({"id":id,"result":{"authMethod":"chatgpt","requiresOpenaiAuth":mode!="custom-auth","authToken":null}})
			},
			"model/list" =>
				json!({"id":id,"result":{"data":[{"id":"scoped","model":"scoped","displayName":"Scoped","supportedReasoningEfforts":[{"reasoningEffort":"low"},{"reasoningEffort":"medium"}],"defaultReasoningEffort":"medium","serviceTiers":[{"id":"priority"}],"defaultServiceTier":"priority"}],"nextCursor":null}}),
			"experimentalFeature/list" =>
				json!({"id":id,"result":{"data":[{"name":"fast_mode","enabled":mode!="preserve-tier"}],"nextCursor":null}}),
			"thread/settings/update" => {
				writes.fetch_add(1, Ordering::AcqRel);

				let mut expected = json!({"threadId":"thread","model":"scoped","effort":"low"});

				if mode != "preserve-tier" {
					expected["serviceTier"] = json!("priority");
				}

				assert_eq!(request["params"], expected);

				match mode {
					"queued" | "preserve-tier" => {
						let event = json!({"method":"thread/settings/updated","params":{"threadId":"thread","threadSettings":settings("scoped")}});

						w.write_all(format!("{event}\n").as_bytes())
							.await
							.expect("write model publication");

						json!({"id":id,"result":{}})
					},
					"rejected" => json!({"id":id,"error":{"code":-32_602,"message":"Rejected"}}),
					_ => json!({"id":id,"error":{"code":-32_001,"message":"Unknown delivery"}}),
				}
			},
			method => panic!("unexpected native method {method}"),
		};

		w.write_all(format!("{reply}\n").as_bytes()).await.expect("write native reply");
	}
}
