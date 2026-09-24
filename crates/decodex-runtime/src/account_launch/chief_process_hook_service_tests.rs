//! Shared hook production-service tests use disposable process ownership records.
use super::*;
use decodex_protocol::{ChiefHookChange, ChiefHookSettingsState as State};
use serde_json::{Value, json};
use std::sync::{
	Arc,
	atomic::{AtomicUsize, Ordering},
};
use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};

#[tokio::test]
async fn hook_service_retains_unknown_receipts_and_reconciles_native_raw_state() {
	for change in [ChiefHookChange::Trust, ChiefHookChange::Enabled(false)] {
		for result in ["saved", "overridden", "rejected", "unknown", "unknown-applied", "closed"] {
			tokio::time::timeout(std::time::Duration::from_secs(15), scenario(result, change))
				.await
				.expect("bounded hook scenario");
		}
	}
}
async fn scenario(result: &'static str, change: ChiefHookChange) {
	let home = tempfile::tempdir().expect("home");
	let (local, remote) = tokio::io::duplex(32768);
	let (r, w) = tokio::io::split(local);
	let (client, _events) = AppServerClient::from_io(r, w);
	let writes = Arc::new(AtomicUsize::new(0));
	let backend = tokio::spawn(serve(remote, writes.clone(), result, change));
	let owned = OwnedReviewer::new(home.path(), &client, "thread", "turn").await;
	let source = || async { Some(owned.source(&owned.key)) };
	let State::Available { mut review_token, .. } =
		crate::chief_hooks::read(&owned.store, source).await
	else {
		panic!("review")
	};
	if result == "saved" {
		reject_changed_sources(&owned, review_token.as_str()).await;
		client.request("test/change-hook", json!({})).await.expect("change reviewed content");
		assert!(write(&owned, review_token.as_str(), "hook", "old-hash", change).await.is_err());
		let State::Available { review_token: fresh, .. } =
			crate::chief_hooks::read(&owned.store, source).await
		else {
			panic!("fresh hook review")
		};
		assert_ne!(fresh, review_token);
		review_token = fresh;
	}
	for key in ["missing", "managed"] {
		assert!(write(&owned, review_token.as_str(), key, "invalid", change).await.is_err());
	}
	assert_eq!(writes.load(Ordering::Acquire), 0);
	let response = write(&owned, review_token.as_str(), "hook", "first", change).await;
	assert_eq!(
		response.is_ok(),
		matches!(result, "saved" | "overridden"),
		"{result}: {response:?}"
	);
	assert_eq!(writes.load(Ordering::Acquire), 1);
	let state = crate::chief_hooks::read(&owned.store, source).await;
	let expected = match result {
		"unknown-applied" => "target_observed",
		"closed" => "unknown",
		other => other,
	};
	if result != "closed" {
		let State::Available { last_edit: Some(edit), can_update, .. } = state else {
			panic!("current state")
		};
		assert_eq!(edit.outcome, expected);
		assert_eq!(can_update, result != "unknown");
	}
	assert!(write(&owned, review_token.as_str(), "hook", "replay", change).await.is_err());
	assert_eq!(writes.load(Ordering::Acquire), 1);
	let scope: String =
		sha2::Sha256::digest(b"/native/config.toml").iter().map(|b| format!("{b:02x}")).collect();
	let reopened = SqliteStore::open(&owned.root.paths()).expect("reopen");
	assert_eq!(
		reopened.chief_hook_receipt(scope).await.expect("receipt").expect("saved").state,
		expected
	);
	assert!(reopened.list_pending_chief_events(100).await.expect("no wake").is_empty());
	backend.abort();
}
async fn write(
	owned: &OwnedReviewer,
	review: &str,
	hook: &str,
	key: &str,
	change: ChiefHookChange,
) -> Result<(), crate::chief_host::ChiefHostError> {
	crate::chief_hooks::write(
		&owned.store,
		|| async { Some(owned.source(&owned.key)) },
		crate::chief_hooks::Selection { thread: "thread", review, hook, change, attempt_id: key },
	)
	.await
}
fn metadata(key: &str, managed: bool, hash: &str) -> Value {
	json!({"key":key,"pluginId":"sample@test","eventName":"UserPromptSubmit","handlerType":"command","command":"echo fixture","sourcePath":"/native/hooks.json","currentHash":hash,"trustStatus":if managed{"managed"}else{"untrusted"},"isManaged":managed,"enabled":true})
}
async fn serve(
	remote: tokio::io::DuplexStream,
	writes: Arc<AtomicUsize>,
	outcome: &str,
	change: ChiefHookChange,
) {
	let (r, mut w) = tokio::io::split(remote);
	let mut lines = BufReader::new(r).lines();
	let mut applied = false;
	let mut hash = "hash";
	while let Some(line) = lines.next_line().await.expect("request") {
		let request: Value = serde_json::from_str(&line).expect("json");
		let id = &request["id"];
		let result = match request["method"].as_str().expect("method") {
			"test/change-hook" => {
				hash = "new-hash";
				json!({"id":id,"result":{}})
			},
			"thread/read" => json!({"id":id,"result":{"thread":{"id":"thread","cwd":"/native"}}}),
			"config/read" =>
				json!({"id":id,"result":{"layers":[{"name":{"type":"user","file":"/native/config.toml"},"version":if applied{"after"}else{"before"},"config":{"hooks":{"state":if applied{match change {ChiefHookChange::Trust=>json!({"hook":{"trusted_hash":hash}}),ChiefHookChange::Enabled(v)=>json!({"hook":{"enabled":v}})}}else{json!({})}}}}]}}),
			"hooks/list" =>
				json!({"id":id,"result":{"data":[{"cwd":"/native","hooks":[metadata("hook",false,hash),metadata("managed",true,hash)],"warnings":[],"errors":[]}]}}),
			"config/batchWrite" => {
				writes.fetch_add(1, Ordering::AcqRel);
				assert_eq!(request["params"]["expectedVersion"], "before");
				let (field, value) = match change {
					ChiefHookChange::Trust => ("trusted_hash", json!(hash)),
					ChiefHookChange::Enabled(v) => ("enabled", json!(v)),
				};
				assert_eq!(request["params"]["edits"][0]["value"], value);
				assert_eq!(
					request["params"]["edits"][0]["keyPath"],
					format!("hooks.state.\"hook\".{field}")
				);
				if matches!(outcome, "saved" | "overridden" | "unknown-applied") {
					applied = true;
				}
				match outcome {
					"saved" | "overridden" =>
						json!({"id":id,"result":{"status":if outcome=="saved"{"ok"}else{"okOverridden"},"version":"after","filePath":"/native/config.toml"}}),
					"rejected" =>
						json!({"id":id,"error":{"code":-32600,"message":"version conflict","data":{"config_write_error_code":"configVersionConflict"}}}),
					"closed" => return,
					_ => json!({"id":id,"error":{"code":-32001,"message":"unknown"}}),
				}
			},
			_ => panic!("unexpected native method"),
		};
		w.write_all(format!("{result}\n").as_bytes()).await.expect("response");
	}
}
async fn reject_changed_sources(owned: &OwnedReviewer, review: &str) {
	for change in ["account", "generation", "revision", "history", "thread", "work", "closed"] {
		let calls = AtomicUsize::new(0);
		let source = || {
			let later = calls.fetch_add(1, Ordering::AcqRel) > 0;
			async move {
				if later && change == "closed" {
					return None;
				}
				let mut key = owned.key.clone();
				if later {
					match change {
						"account" =>
							key.account = AccountId::new("10000000-0000-4000-8000-000000000002")
								.expect("account"),
						"generation" =>
							key.generation =
								ProcessGenerationId::new("30000000-0000-4000-8000-000000000002")
									.expect("generation"),
						"revision" => key.revision += 1,
						"history" => key.history_revision += 1,
						"thread" => key.thread = "foreign".into(),
						"work" => key.work = "foreign".into(),
						_ => {},
					}
				}
				Some(owned.source(&key))
			}
		};
		assert_eq!(
			crate::chief_hooks::read(&owned.store, source).await,
			State::Unavailable,
			"{change}"
		);
		calls.store(0, Ordering::Release);
		assert!(
			matches!(
				crate::chief_hooks::write(
					&owned.store,
					source,
					crate::chief_hooks::Selection {
						thread: "thread",
						review,
						hook: "hook",
						change: ChiefHookChange::Trust,
						attempt_id: "stale"
					}
				)
				.await,
				Err(crate::chief_host::ChiefHostError::Rejected(_))
			),
			"{change}"
		);
	}
}
