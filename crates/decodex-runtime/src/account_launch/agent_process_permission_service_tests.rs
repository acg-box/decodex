//! Permission command ownership tests use disposable process records, not kernel admission.
use super::*;
use decodex_protocol::AgentPermissionState;
use serde_json::{Value, json};
use std::sync::{
	Arc,
	atomic::{AtomicUsize, Ordering},
};
use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};

#[tokio::test]
async fn permission_service_preserves_unknown_receipts_and_never_replays_a_review() {
	for outcome in ["queued", "rejected", "unknown"] {
		tokio::time::timeout(std::time::Duration::from_secs(15), scenario(outcome))
			.await
			.expect("bounded permission service");
	}
}

async fn scenario(outcome: &'static str) {
	let home = tempfile::tempdir().expect("fixture home");
	let (local, remote) = tokio::io::duplex(32_768);
	let (r, w) = tokio::io::split(local);
	let (client, _events) = AppServerClient::from_io(r, w);
	let writes = Arc::new(AtomicUsize::new(0));
	let backend = tokio::spawn(serve(remote, writes.clone(), outcome));

	client.thread_resume(json!({"threadId":"thread"})).await.expect("hydrate");

	let owned = OwnedReviewer::new(home.path(), &client, "thread", "turn").await;
	let source = || async { Some(owned.source(&owned.key)) };
	let state = crate::agent_permissions::read(&owned.store, source).await;
	let AgentPermissionState::Available { mut review_token, profiles, can_update, .. } = state
	else {
		panic!("available permission review")
	};

	assert!(can_update);

	if outcome == "queued" {
		reject_changed_sources(&owned, review_token.as_str()).await;

		review_token = reject_restored_settings(&owned, review_token).await;
	}

	assert!(profiles.iter().any(|p| p.id.as_str() == "scoped" && p.can_select));
	assert!(profiles.iter().any(|p| p.id.as_str() == "forbidden" && !p.can_select));
	assert!(
		crate::agent_permissions::write(
			&owned.store,
			source,
			"foreign",
			review_token.as_str(),
			"scoped",
			"foreign"
		)
		.await
		.is_err()
	);
	assert!(
		crate::agent_permissions::write(
			&owned.store,
			source,
			"thread",
			review_token.as_str(),
			"forbidden",
			"forbidden"
		)
		.await
		.is_err()
	);
	assert_eq!(writes.load(Ordering::Acquire), 0);

	let result = crate::agent_permissions::write(
		&owned.store,
		source,
		"thread",
		review_token.as_str(),
		"scoped",
		"first",
	)
	.await;

	assert_eq!(result.is_ok(), outcome == "queued");
	assert_eq!(writes.load(Ordering::Acquire), 1);

	if outcome == "queued" {
		let state = crate::agent_permissions::read(&owned.store, source).await;

		assert!(matches!(
			state,
			AgentPermissionState::Available {
				last_outcome: Some(decodex_protocol::AgentPermissionOutcome::TargetObserved),
				..
			}
		));
	}

	let expected = if outcome == "queued" { "target_observed" } else { outcome };
	let receipt = owned
		.store
		.agent_permission_receipt("root".into(), "thread".into())
		.await
		.expect("receipt")
		.expect("saved");

	assert_eq!(receipt.state, expected);
	assert!(
		crate::agent_permissions::write(
			&owned.store,
			source,
			"thread",
			review_token.as_str(),
			"scoped",
			"different-key"
		)
		.await
		.is_err()
	);
	assert_eq!(writes.load(Ordering::Acquire), 1);

	let reopened = SqliteStore::open(&owned.root.paths()).expect("reopen");

	assert_eq!(
		reopened
			.agent_permission_receipt("root".into(), "thread".into())
			.await
			.expect("receipt")
			.expect("saved")
			.state,
		expected
	);
	assert!(reopened.list_pending_agent_events(10).await.expect("pending").is_empty());

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
				for excluded in [json!("temporary"), json!(":read-only")] {
					let event = json!({"method":"thread/settings/updated","params":{"threadId":"thread","threadSettings":{"cwd":"/fixture","approvalPolicy":"on-request","approvalsReviewer":"user","sandboxPolicy":{"type":"readOnly"},"activePermissionProfile":{"id":excluded}}}});

					w.write_all(format!("{event}\n").as_bytes())
						.await
						.expect("settings publication");
				}

				json!({"id":id,"result":{}})
			},
			"thread/resume" =>
				json!({"id":id,"result":{"thread":{"id":"thread"},"cwd":"/fixture","approvalPolicy":"on-request","approvalsReviewer":"user","sandbox":{"type":"readOnly"},"activePermissionProfile":{"id":":read-only"}}}),
			"permissionProfile/list" =>
				json!({"id":id,"result":{"data":[{"id":"scoped","allowed":true},{"id":"forbidden","allowed":false}],"nextCursor":null}}),
			"thread/settings/update" => {
				writes.fetch_add(1, Ordering::AcqRel);

				assert_eq!(request["params"], json!({"threadId":"thread","permissions":"scoped"}));

				match outcome {
					"queued" => {
						let event = json!({"method":"thread/settings/updated","params":{"threadId":"thread","threadSettings":{"cwd":"/fixture","approvalPolicy":"on-request","approvalsReviewer":"user","sandboxPolicy":{"type":"readOnly"},"activePermissionProfile":{"id":"scoped"}}}});

						w.write_all(format!("{event}\n").as_bytes()).await.expect("publication");

						json!({"id":id,"result":{}})
					},
					"rejected" =>
						json!({"id":id,"error":{"code":-32_602,"message":"Native policy refused"}}),
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
			crate::agent_permissions::read(&owned.store, source).await,
			AgentPermissionState::Unavailable,
			"{change}"
		);

		calls.store(0, Ordering::Release);

		assert!(
			matches!(
				crate::agent_permissions::write(
					&owned.store,
					source,
					"thread",
					review,
					"scoped",
					"stale"
				)
				.await,
				Err(crate::agent_host::AgentHostError::Rejected(_))
			),
			"{change}"
		);
	}
}

async fn reject_restored_settings(
	owned: &OwnedReviewer,
	old: decodex_protocol::WireText,
) -> decodex_protocol::WireText {
	owned.client.request("test/bounce", json!({})).await.expect("wire barrier");

	let source = || async { Some(owned.source(&owned.key)) };

	assert!(
		crate::agent_permissions::write(
			&owned.store,
			source,
			"thread",
			old.as_str(),
			"scoped",
			"restored"
		)
		.await
		.is_err(),
		"A-B-A settings must not revive an old review without owner event draining"
	);

	let AgentPermissionState::Available { review_token, .. } =
		crate::agent_permissions::read(&owned.store, source).await
	else {
		panic!("fresh review")
	};

	assert_ne!(review_token, old);

	review_token
}
