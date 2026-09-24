//! Real service and durable store with a controlled native wire; no kernel admission claim.
use super::*;
use decodex_protocol::{
	ChiefAppApprovalMode as Mode, ChiefAppReviewer as Reviewer, ChiefAppSettingEdit as Edit,
	ChiefAppSettingsResult as State,
};
use serde_json::{Value, json};
use std::sync::{
	Arc,
	atomic::{AtomicUsize, Ordering},
};
use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};

fn params() -> Value {
	json!({"threadId":"thread","turnId":"earlier-origin","serverName":"codex_apps","mode":"form","message":"Review","requestedSchema":{"type":"object","properties":{}},"_meta":{"connector_id":"calendar","link_id":"work"}})
}
async fn reserve_event(owned: &OwnedReviewer) -> i64 {
	owned
		.store
		.enqueue_chief_event(decodex_database::EnqueueChiefEvent {
			source_event_id: "app-request".into(),
			work_item_id: "root".into(),
			event_kind: "server_request_pending".into(),
			payload:
				json!({"id":"approval","method":"mcpServer/elicitation/request","params":params()})
					.to_string(),
		})
		.await
		.expect("event")
		.id
}
async fn write(
	owned: &OwnedReviewer,
	event: i64,
	review: &str,
	edit: &Edit,
	key: &str,
) -> Result<(), crate::chief_host::ChiefHostError> {
	crate::chief_app_settings::write(
		&owned.store,
		|| async { Some(owned.source(&owned.key)) },
		crate::chief_app_settings::Selection { event, review, edit, attempt_id: key },
	)
	.await
}
#[tokio::test]
async fn app_service_separates_save_receipts_request_liveness_and_shared_recovery() {
	for edit in [
		Edit::ApprovalMode(Some(Mode::Approve)),
		Edit::Reviewer(Some(Reviewer::AutoReview)),
		Edit::ApprovalMode(None),
	] {
		for result in [
			"saved",
			"overridden",
			"rejected",
			"unknown",
			"unknown-applied",
			"closed",
			"read-failed",
		] {
			tokio::time::timeout(std::time::Duration::from_secs(15), scenario(result, &edit))
				.await
				.expect("bounded scenario");
		}
	}
}
async fn scenario(outcome: &'static str, edit: &Edit) {
	let home = tempfile::tempdir().expect("home");
	let (local, remote) = tokio::io::duplex(32768);
	let (r, w) = tokio::io::split(local);
	let (client, mut events) = AppServerClient::from_io(r, w);
	let writes = Arc::new(AtomicUsize::new(0));
	let backend = tokio::spawn(serve(remote, writes.clone(), outcome, edit.clone()));
	client.request("test/pending", json!({})).await.expect("pending");
	let decodex_codex::app_server_client::ServerEvent::Request { id, method, params } =
		events.recv().await.expect("request")
	else {
		panic!("pending")
	};
	let guard = client.server_request_guard(&id, &method, &params).expect("guard");
	let owned = OwnedReviewer::new(home.path(), &client, "thread", "later-active-turn").await;
	let event = reserve_event(&owned).await;
	let source = || async { Some(owned.source(&owned.key)) };
	let State::Available { review_token, can_update: true, .. } =
		crate::chief_app_settings::read(&owned.store, source, event).await
	else {
		panic!("review")
	};
	if outcome == "saved" {
		reject_changed_sources(&owned, event, &review_token, edit).await;
	}
	let result = write(&owned, event, &review_token, edit, "first").await;
	assert_eq!(
		result.is_ok(),
		matches!(outcome, "saved" | "overridden" | "read-failed"),
		"{outcome}: {result:?}"
	);
	assert_eq!(writes.load(Ordering::Acquire), 1);
	assert!(owned.store.get_chief_inbox_event(event).await.expect("event").disposition.is_none());
	if outcome != "closed" {
		assert!(guard.is_live(), "save must not answer the tool");
	}
	let expected = if outcome == "unknown-applied" {
		"target_observed"
	} else if outcome == "read-failed" {
		"saved"
	} else if outcome == "closed" {
		"unknown"
	} else {
		outcome
	};
	let resolved_early = outcome == "unknown-applied";
	if resolved_early {
		client
			.respond(id.clone(), json!({"action":"accept","content":null}))
			.await
			.expect("answer before recovery");
		owned.store.acknowledge_chief_request_event(event).await.expect("resolved");
		let decodex_protocol::ChiefHookSettingsState::Available {
			can_update: true, notices, ..
		} = crate::chief_hooks::read(&owned.store, source).await
		else {
			panic!("recover app via hook settings")
		};
		assert!(notices.iter().any(|n| n.contains("target_observed")));
	}
	let state = crate::chief_app_settings::read(&owned.store, source, event).await;
	if !matches!(outcome, "closed" | "read-failed") {
		let State::Available { last_edit: Some(last), can_update, .. } = state else {
			panic!("readback")
		};
		assert_eq!(last.outcome, expected);
		assert_eq!(can_update, outcome != "unknown" && !resolved_early);
		if !resolved_early {
			client
				.respond(id, json!({"action":"accept","content":null}))
				.await
				.expect("explicit answer");
			owned.store.acknowledge_chief_request_event(event).await.expect("resolved");
		}
		let State::Available { can_update: false, last_edit: Some(last), .. } =
			crate::chief_app_settings::read(&owned.store, source, event).await
		else {
			panic!("resolved readback")
		};
		assert_eq!(last.outcome, expected);
		let decodex_protocol::ChiefHookSettingsState::Available { can_update, notices, .. } =
			crate::chief_hooks::read(&owned.store, source).await
		else {
			panic!("shared hook read")
		};
		assert_eq!(can_update, outcome != "unknown");
		assert!(notices.iter().any(|n| n.contains("calendar") && n.contains(expected)));
	} else {
		assert_eq!(state, State::Unavailable);
	}
	assert!(write(&owned, event, &review_token, edit, "replay").await.is_err());
	assert_eq!(writes.load(Ordering::Acquire), 1);
	let reopened = SqliteStore::open(&owned.root.paths()).expect("reopen");
	let receipt = reopened
		.chief_app_settings_receipt(crate::chief_config_settings::digest("/native/config.toml"))
		.await
		.expect("receipt")
		.expect("saved");
	assert_eq!(receipt.state, expected);
	assert_eq!(
		receipt.saved_version.as_deref(),
		matches!(outcome, "saved" | "overridden" | "read-failed").then_some("after")
	);
	backend.abort();
}
async fn serve(
	remote: tokio::io::DuplexStream,
	writes: Arc<AtomicUsize>,
	outcome: &str,
	edit: Edit,
) {
	let (r, mut w) = tokio::io::split(remote);
	let mut lines = BufReader::new(r).lines();
	let mut applied = false;
	while let Some(line) = lines.next_line().await.expect("line") {
		let request: Value = serde_json::from_str(&line).expect("json");
		if request.get("method").is_none() {
			assert_eq!(request["id"], "approval");
			continue;
		}
		let id = &request["id"];
		let result = match request["method"].as_str().expect("method") {
			"test/pending" => {
				w.write_all(
					format!(
						"{}\n",
						json!({"id":"approval","method":"mcpServer/elicitation/request","params":params()})
					)
					.as_bytes(),
				)
				.await
				.expect("request");
				json!({"id":id,"result":{}})
			},
			"thread/read" => json!({"id":id,"result":{"thread":{"id":"thread","cwd":"/native"}}}),
			"hooks/list" =>
				json!({"id":id,"result":{"data":[{"cwd":"/native","hooks":[],"warnings":[],"errors":[]}]}}),
			"config/read" =>
				if applied && outcome == "read-failed" {
					json!({"id":id,"error":{"code":-32001,"message":"read unavailable"}})
				} else {
					let mut link =
						json!({"default_tools_approval_mode":"prompt","approvals_reviewer":"user"});
					if applied {
						let (field, value) = edit.native_value();
						if let Some(value) = value {
							link[field] = json!(value)
						} else {
							link.as_object_mut().expect("link").remove(field);
						}
					}
					let config = json!({"apps":{"calendar":{"links":{"work":link}}}});
					json!({"id":id,"result":{"config":config,"layers":[{"name":{"type":"user","file":"/native/config.toml"},"version":if applied{"after"}else{"before"},"config":config}]}})
				},
			"config/batchWrite" => {
				writes.fetch_add(1, Ordering::AcqRel);
				let (field, value) = edit.native_value();
				assert_eq!(request["params"]["expectedVersion"], "before");
				assert_eq!(
					request["params"]["edits"][0]["keyPath"],
					format!("apps.\"calendar\".links.\"work\".{field}")
				);
				assert_eq!(request["params"]["edits"][0]["value"], json!(value));
				applied =
					matches!(outcome, "saved" | "overridden" | "unknown-applied" | "read-failed");
				match outcome {
					"saved" | "overridden" | "read-failed" =>
						json!({"id":id,"result":{"status":if outcome=="overridden"{"okOverridden"}else{"ok"},"version":"after","filePath":"/native/config.toml"}}),
					"rejected" =>
						json!({"id":id,"error":{"code":-32600,"message":"version conflict","data":{"config_write_error_code":"configVersionConflict"}}}),
					"closed" => return,
					_ => json!({"id":id,"error":{"code":-32001,"message":"unknown"}}),
				}
			},
			other => panic!("unexpected {other}"),
		};
		w.write_all(format!("{result}\n").as_bytes()).await.expect("response");
	}
}
async fn reject_changed_sources(owned: &OwnedReviewer, event: i64, review: &str, edit: &Edit) {
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
		assert!(
			matches!(
				crate::chief_app_settings::write(
					&owned.store,
					source,
					crate::chief_app_settings::Selection {
						event,
						review,
						edit,
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

#[tokio::test]
async fn saved_app_service_survives_resolved_requests_and_rejects_unreviewed_connections() {
	for outcome in ["saved", "unknown", "unknown-applied", "read-failed"] {
		tokio::time::timeout(std::time::Duration::from_secs(15), saved_scenario(outcome))
			.await
			.expect("bounded saved settings scenario");
	}
}
async fn saved_scenario(outcome: &'static str) {
	use decodex_protocol::ChiefSavedAppSettingsResult as Saved;
	let home = tempfile::tempdir().expect("home");
	let (local, remote) = tokio::io::duplex(32768);
	let (r, w) = tokio::io::split(local);
	let (client, mut events) = AppServerClient::from_io(r, w);
	let writes = Arc::new(AtomicUsize::new(0));
	let edit = Edit::ApprovalMode(None);
	let backend = tokio::spawn(serve(remote, writes.clone(), outcome, edit.clone()));
	client.request("test/pending", json!({})).await.expect("pending");
	let decodex_codex::app_server_client::ServerEvent::Request { id, .. } =
		events.recv().await.expect("request")
	else {
		panic!("pending")
	};
	let owned = OwnedReviewer::new(home.path(), &client, "thread", "turn").await;
	let event = reserve_event(&owned).await;
	client.respond(id, json!({"action":"accept","content":null})).await.expect("answer");
	owned.store.acknowledge_chief_request_event(event).await.expect("resolved");
	let source = || async { Some(owned.source(&owned.key)) };
	let Saved::Available { connections, can_update: true, .. } =
		crate::chief_app_settings::read_saved(&owned.store, source).await
	else {
		panic!("saved catalog")
	};
	let row = connections.first().expect("saved override");
	for (thread, connector) in [("foreign", "calendar"), ("thread", "unlisted")] {
		assert!(
			crate::chief_app_settings::write_saved(
				&owned.store,
				source,
				crate::chief_app_settings::SavedSelection {
					thread,
					connector,
					link: "work",
					review: &row.review_token,
					edit: &edit,
					attempt_id: "invalid"
				}
			)
			.await
			.is_err()
		);
	}
	let calls = AtomicUsize::new(0);
	let changing = || {
		let changed = calls.fetch_add(1, Ordering::AcqRel) > 0;
		let fixture = &owned;
		async move {
			let mut key = fixture.key.clone();
			if changed {
				key.revision += 1
			}
			Some(fixture.source(&key))
		}
	};
	assert_eq!(
		crate::chief_app_settings::read_saved(&owned.store, changing).await,
		Saved::Unavailable
	);
	let response = crate::chief_app_settings::write_saved(
		&owned.store,
		source,
		crate::chief_app_settings::SavedSelection {
			thread: "thread",
			connector: "calendar",
			link: "work",
			review: &row.review_token,
			edit: &edit,
			attempt_id: "saved-origin",
		},
	)
	.await;
	assert_eq!(response.is_ok(), matches!(outcome, "saved" | "read-failed"));
	assert_eq!(writes.load(Ordering::Acquire), 1);
	let state = crate::chief_app_settings::read_saved(&owned.store, source).await;
	let expected = match outcome {
		"unknown-applied" => "target_observed",
		"read-failed" => "saved",
		other => other,
	};
	if outcome == "read-failed" {
		assert_eq!(state, Saved::Unavailable)
	} else {
		let Saved::Available { last_edit: Some(last), can_update, .. } = state else {
			panic!("result")
		};
		assert_eq!(last.outcome, expected);
		assert_eq!(can_update, outcome != "unknown");
	}
	assert!(
		crate::chief_app_settings::write_saved(
			&owned.store,
			source,
			crate::chief_app_settings::SavedSelection {
				thread: "thread",
				connector: "calendar",
				link: "work",
				review: &row.review_token,
				edit: &edit,
				attempt_id: "replay"
			}
		)
		.await
		.is_err()
	);
	assert_eq!(writes.load(Ordering::Acquire), 1);
	let reopened = SqliteStore::open(&owned.root.paths()).expect("reopen");
	let receipt = reopened
		.chief_app_settings_receipt(crate::chief_config_settings::digest("/native/config.toml"))
		.await
		.expect("receipt")
		.expect("saved");
	assert_eq!(receipt.state, expected);
	assert_eq!(receipt.attempt.request_event_id, None);
	backend.abort();
}
