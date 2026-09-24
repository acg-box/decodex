//! PluginSelection command ownership tests use disposable process records, not kernel admission.
use super::*;
use decodex_protocol::ChiefPluginSelectionState;
use serde_json::{Value, json};
use std::sync::{
	Arc,
	atomic::{AtomicUsize, Ordering},
};
use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};

#[tokio::test]
async fn plugin_service_preserves_unknown_receipts_and_never_replays_a_review() {
	for outcome in ["queued", "rejected", "unknown", "unknown-live"] {
		tokio::time::timeout(std::time::Duration::from_secs(15), scenario(outcome))
			.await
			.expect("bounded plugin service");
	}
}

async fn scenario(outcome: &'static str) {
	let home = tempfile::tempdir().expect("fixture home");
	let (local, remote) = tokio::io::duplex(32768);
	let (r, w) = tokio::io::split(local);
	let (client, _events) = AppServerClient::from_io(r, w);
	let writes = Arc::new(AtomicUsize::new(0));
	let backend = tokio::spawn(serve(remote, writes.clone(), outcome));
	client.thread_resume(json!({"threadId":"thread"})).await.expect("hydrate");
	let owned = OwnedReviewer::new(home.path(), &client, "thread", "turn").await;
	let source = || async { Some(owned.source(&owned.key)) };
	let state = crate::chief_plugins::read(&owned.store, source).await;
	let ChiefPluginSelectionState::Available { mut review_token, catalog, can_update, .. } = state
	else {
		panic!("available plugin review")
	};
	assert!(can_update);
	if outcome == "queued" {
		reject_changed_sources(&owned, review_token.as_str()).await;
		review_token = reject_restored_settings(&owned, review_token).await;
	}
	assert!(
		matches!(catalog,decodex_protocol::ChiefPluginInventory::Available {plugins,..} if plugins.iter().any(|p|p.id=="scoped"&&p.installed))
	);

	assert!(
		write(&owned.store, source, "foreign", review_token.as_str(), "scoped", "foreign")
			.await
			.is_err()
	);
	assert!(
		write(&owned.store, source, "thread", review_token.as_str(), "forbidden", "forbidden")
			.await
			.is_err()
	);
	assert_eq!(writes.load(Ordering::Acquire), 0);
	let result =
		write(&owned.store, source, "thread", review_token.as_str(), "scoped", "first").await;
	assert_eq!(result.is_ok(), outcome == "queued");
	assert_eq!(writes.load(Ordering::Acquire), 1);
	if outcome == "queued" {
		let state = crate::chief_plugins::read(&owned.store, source).await;
		assert!(matches!(
			state,
			ChiefPluginSelectionState::Available {
				last_outcome: Some(decodex_protocol::ChiefPluginOutcome::TargetObserved),
				..
			}
		));
	}
	let expected = if outcome == "queued" {
		"target_observed"
	} else if outcome == "unknown-live" {
		"unknown"
	} else {
		outcome
	};
	if outcome == "unknown-live" {
		let state = crate::chief_permissions::read(&owned.store, source).await;
		assert!(matches!(
			state,
			decodex_protocol::ChiefPermissionState::Available { can_update: false, .. }
		));
	}
	let receipt = owned
		.store
		.chief_plugin_receipt("root".into(), "thread".into())
		.await
		.expect("receipt")
		.expect("saved");
	assert_eq!(receipt.state, expected);
	assert!(
		write(&owned.store, source, "thread", review_token.as_str(), "scoped", "different-key")
			.await
			.is_err()
	);
	assert_eq!(writes.load(Ordering::Acquire), 1);
	let reopened = SqliteStore::open(&owned.root.paths()).expect("reopen");
	assert_eq!(
		reopened
			.chief_plugin_receipt("root".into(), "thread".into())
			.await
			.expect("receipt")
			.expect("saved")
			.state,
		expected
	);
	assert!(reopened.list_pending_chief_events(10).await.expect("pending").is_empty());
	backend.abort();
}

async fn serve(remote: tokio::io::DuplexStream, writes: Arc<AtomicUsize>, outcome: &str) {
	let (r, mut w) = tokio::io::split(remote);
	let mut lines = BufReader::new(r).lines();
	while let Some(line) = lines.next_line().await.expect("request") {
		let request: Value = serde_json::from_str(&line).expect("JSON");
		let id = &request["id"];
		let reply = match request["method"].as_str().expect("method") {
			"test/bounce" => {
				for excluded in [json!(["transient@market"]), json!(["other@market"])] {
					let event = json!({"method":"thread/settings/updated","params":{"threadId":"thread","threadSettings":{"cwd":"/fixture","approvalPolicy":"on-request","approvalsReviewer":"user","sandboxPolicy":{"type":"readOnly"},"disabledPluginIds":excluded,"activePermissionProfile":{"id":":read-only"}}}});
					w.write_all(format!("{event}\n").as_bytes())
						.await
						.expect("settings publication");
				}
				json!({"id":id,"result":{}})
			},
			"thread/resume" =>
				json!({"id":id,"result":{"thread":{"id":"thread"},"cwd":"/fixture","approvalPolicy":"on-request","approvalsReviewer":"user","sandbox":{"type":"readOnly"},"activePermissionProfile":{"id":":read-only"},"disabledPluginIds":["other@market"]}}),
			"thread/read" => json!({"id":id,"result":{"thread":{"id":"thread","cwd":"/fixture"}}}),
			"permissionProfile/list" =>
				json!({"id":id,"result":{"data":[{"id":"scoped","allowed":true}],"nextCursor":null}}),
			"plugin/installed" =>
				json!({"id":id,"result":{"marketplaces":[{"name":"fixture","plugins":[{"id":"scoped","name":"Scoped","installed":true,"enabled":true,"availability":"AVAILABLE"},{"id":"forbidden","name":"Unavailable","installed":false,"enabled":false,"availability":"UNAVAILABLE"}]}],"marketplaceLoadErrors":[]}}),
			"thread/settings/update" => {
				writes.fetch_add(1, Ordering::AcqRel);
				assert_eq!(
					request["params"],
					json!({"threadId":"thread","disabledPluginIds":["other@market","scoped"]})
				);
				match outcome {
					"queued" => {
						let event = json!({"method":"thread/settings/updated","params":{"threadId":"thread","threadSettings":{"cwd":"/fixture","approvalPolicy":"on-request","approvalsReviewer":"user","sandboxPolicy":{"type":"readOnly"},"activePermissionProfile":{"id":":read-only"},"disabledPluginIds":["other@market","scoped"]}}});
						w.write_all(format!("{event}\n").as_bytes()).await.expect("publication");
						json!({"id":id,"result":{}})
					},
					"rejected" =>
						json!({"id":id,"error":{"code":-32602,"message":"Native policy refused"}}),
					"unknown-live" =>
						json!({"id":id,"error":{"code":-32001,"message":"Outcome unknown"}}),
					_ => return,
				}
			},
			_ => panic!("unexpected native request"),
		};
		w.write_all(format!("{reply}\n").as_bytes()).await.expect("response");
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
			crate::chief_plugins::read(&owned.store, source).await,
			ChiefPluginSelectionState::Unavailable,
			"{change}"
		);
		calls.store(0, Ordering::Release);
		assert!(
			matches!(
				write(&owned.store, source, "thread", review, "scoped", "stale").await,
				Err(crate::chief_host::ChiefHostError::Rejected(_))
			),
			"{change}"
		);
	}
}

async fn write<F, Fut>(
	store: &SqliteStore,
	source: F,
	thread: &str,
	review: &str,
	plugin: &str,
	key: &str,
) -> Result<(), crate::chief_host::ChiefHostError>
where
	F: Fn() -> Fut,
	Fut: std::future::Future<Output = Option<Source>>,
{
	crate::chief_plugins::write(
		store,
		source,
		crate::chief_plugins::Change { thread, review, plugin, enabled: false, attempt_id: key },
	)
	.await
}

async fn reject_restored_settings(
	owned: &OwnedReviewer,
	old: decodex_protocol::WireText,
) -> decodex_protocol::WireText {
	owned.client.request("test/bounce", json!({})).await.expect("wire barrier");
	let source = || async { Some(owned.source(&owned.key)) };
	assert!(
		write(&owned.store, source, "thread", old.as_str(), "scoped", "restored").await.is_err(),
		"A-B-A settings must not revive an old review without owner event draining"
	);
	let ChiefPluginSelectionState::Available { review_token, .. } =
		crate::chief_plugins::read(&owned.store, source).await
	else {
		panic!("fresh review")
	};
	assert_ne!(review_token, old);
	review_token
}
