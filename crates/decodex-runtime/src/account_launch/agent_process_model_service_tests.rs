//! ModelSelection command ownership tests use disposable process records, not kernel admission.
use std::{
	future::Future,
	sync::{
		Arc,
		atomic::{AtomicUsize, Ordering},
	},
	time::Duration,
};

use rusqlite::Connection;
use serde_json::{self, Value};
use tokio::{
	io::{self, AsyncBufReadExt as _, AsyncWriteExt as _, BufReader, DuplexStream},
	time,
};

use crate::{
	account_launch::agent_process::native_tests::reviewer::store::*,
	agent_host::AgentHostError,
	agent_models::{self, Change},
	agent_permissions,
};
use decodex_protocol::{AgentModelSelectionState, AgentPermissionState, WireText};

fn settings(model: &str) -> Value {
	serde_json::json!({"model":model,"modelProvider":"fixture","effort":"high","serviceTier":"priority","cwd":"/fixture","approvalPolicy":"on-request","approvalsReviewer":"user","sandboxPolicy":{"type":"readOnly"},"disabledPluginIds":[],"activePermissionProfile":{"id":":read-only"}})
}

#[tokio::test]
async fn model_service_preserves_unknown_receipts_and_never_replays_a_review() {
	for outcome in ["queued", "queued-unobserved", "rejected", "unknown", "unknown-live"] {
		time::timeout(Duration::from_secs(15), scenario(outcome))
			.await
			.expect("bounded model service");
	}
}

async fn scenario(outcome: &'static str) {
	let home = tempfile::tempdir().expect("fixture home");
	let (local, remote) = io::duplex(32_768);
	let (r, w) = io::split(local);
	let (client, _events) = AppServerClient::from_io(r, w);
	let writes = Arc::new(AtomicUsize::new(0));
	let backend = tokio::spawn(serve(remote, writes.clone(), outcome));

	client.thread_resume(serde_json::json!({"threadId":"thread"})).await.expect("hydrate");

	let owned = OwnedReviewer::new(home.path(), &client, "thread", "turn").await;
	let source = || async { Some(owned.source(&owned.key)) };
	let state = agent_models::read(&owned.store, source).await;
	let AgentModelSelectionState::Available { mut review_token, models, can_update, .. } = state
	else {
		panic!("available model review")
	};

	assert!(can_update);

	if outcome == "queued" {
		reject_changed_sources(&owned, review_token.as_str()).await;

		review_token = reject_restored_settings(&owned, review_token).await;
	}

	assert!(models.iter().any(|m| m.model.as_str() == "scoped" && m.efforts.is_empty()));
	assert!(models.iter().all(|model| model.model.as_str() != "gpt-reserve"));
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
	assert!(
		agent_models::write(
			&owned.store,
			source,
			crate::agent_models::Change {
				thread: "thread",
				review: review_token.as_str(),
				model: "scoped",
				effort: Some("not-advertised"),
				attempt_id: "bad-effort",
			}
		)
		.await
		.is_err()
	);
	assert_eq!(writes.load(Ordering::Acquire), 0);

	let result =
		write(&owned.store, source, "thread", review_token.as_str(), "scoped", "first").await;

	assert_eq!(result.is_ok(), matches!(outcome, "queued" | "queued-unobserved"));
	assert_eq!(writes.load(Ordering::Acquire), 1);

	assert_manual_source(&owned).await;

	if outcome == "queued" {
		let state = agent_models::read(&owned.store, source).await;

		assert!(matches!(
			state,
			AgentModelSelectionState::Available {
				last_outcome: Some(decodex_protocol::AgentModelOutcome::TargetObserved),
				last_receipt: Some(decodex_protocol::AgentModelSelectionReceipt {
					manual: true,
					response: decodex_protocol::AgentModelResponse::Queued,
					target_observed: true,
					reconciled: false,
					..
				}),
				..
			}
		));
	}

	let expected = if outcome == "queued" {
		"target_observed"
	} else if outcome == "queued-unobserved" {
		assert!(matches!(
			agent_models::read(&owned.store, source).await,
			AgentModelSelectionState::Pending { .. }
		));
		"queued"
	} else if outcome == "unknown-live" {
		"unknown"
	} else {
		outcome
	};

	if outcome == "unknown-live" {
		let state = agent_permissions::read(&owned.store, source).await;

		assert!(matches!(state, AgentPermissionState::Available { can_update: false, .. }));
	}

	let receipt = owned
		.store
		.agent_model_receipt("root".into(), "thread".into())
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
			.agent_model_receipt("root".into(), "thread".into())
			.await
			.expect("receipt")
			.expect("saved")
			.state,
		expected
	);
	assert!(reopened.list_pending_agent_events(10).await.expect("pending").is_empty());

	backend.abort();
}

async fn assert_manual_source(owned: &OwnedReviewer) {
	let captured = owned
		.store
		.agent_model_receipt("root".into(), "thread".into())
		.await
		.expect("saved receipt")
		.expect("reserved selection");

	assert_eq!(
		captured.attempt.manual_source,
		Some(decodex_database::AgentManualModelSource {
			account: owned.key.account.as_str().into(),
			account_revision: owned.key.revision
		})
	);
}

async fn serve(remote: DuplexStream, writes: Arc<AtomicUsize>, outcome: &str) {
	let (r, mut w) = io::split(remote);
	let mut lines = BufReader::new(r).lines();

	while let Some(line) = lines.next_line().await.expect("request") {
		let request: Value = serde_json::from_str(&line).expect("JSON");
		let id = &request["id"];
		let reply = match request["method"].as_str().expect("method") {
			"test/bounce" => {
				for model in ["transient", "original"] {
					let event = serde_json::json!({"method":"thread/settings/updated","params":{"threadId":"thread","threadSettings":settings(model)}});

					w.write_all(format!("{event}\n").as_bytes()).await.expect("publication");
				}

				serde_json::json!({"id":id,"result":{}})
			},
			"thread/resume" => {
				let mut value = settings("original");

				value["thread"] = serde_json::json!({"id":"thread"});
				value["reasoningEffort"] = value["effort"].take();
				value["sandbox"] = value["sandboxPolicy"].clone();

				serde_json::json!({"id":id,"result":value})
			},
			"model/list" =>
				serde_json::json!({"id":id,"result":{"data":[{"id":"scoped","model":"scoped","displayName":"Scoped","supportedReasoningEfforts":[],"defaultReasoningEffort":null},{ "id":"gpt-reserve","model":"gpt-reserve","displayName":"Reserve","supportedReasoningEfforts":[{"reasoningEffort":"medium"}],"defaultReasoningEffort":"medium","hidden":false}],"nextCursor":null}}),
			"experimentalFeature/list" =>
				serde_json::json!({"id":id,"result":{"data":[],"nextCursor":null}}),
			"permissionProfile/list" =>
				serde_json::json!({"id":id,"result":{"data":[{"id":"scoped","allowed":true}],"nextCursor":null}}),

			"thread/settings/update" => {
				writes.fetch_add(1, Ordering::AcqRel);

				assert_eq!(
					request["params"],
					serde_json::json!({"threadId":"thread","model":"scoped"})
				);

				match outcome {
					"queued" => {
						let event = serde_json::json!({"method":"thread/settings/updated","params":{"threadId":"thread","threadSettings":settings("scoped")}});

						w.write_all(format!("{event}\n").as_bytes()).await.expect("publication");

						serde_json::json!({"id":id,"result":{}})
					},
					"queued-unobserved" => serde_json::json!({"id":id,"result":{}}),
					"rejected" =>
						serde_json::json!({"id":id,"error":{"code":-32_602,"message":"Native policy refused"}}),
					"unknown-live" =>
						serde_json::json!({"id":id,"error":{"code":-32_001,"message":"Outcome unknown"}}),
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
			agent_models::read(&owned.store, source).await,
			AgentModelSelectionState::Unavailable,
			"{change}"
		);

		calls.store(0, Ordering::Release);

		assert!(
			matches!(
				write(&owned.store, source, "thread", review, "scoped", "stale").await,
				Err(crate::agent_host::AgentHostError::Rejected(_))
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
	model: &str,
	key: &str,
) -> Result<(), AgentHostError>
where
	F: Fn() -> Fut,
	Fut: Future<Output = Option<Source>>,
{
	agent_models::write(
		store,
		source,
		Change { thread, review, model, effort: None, attempt_id: key },
	)
	.await
}

async fn reject_restored_settings(owned: &OwnedReviewer, old: WireText) -> WireText {
	owned.client.request("test/bounce", serde_json::json!({})).await.expect("wire barrier");

	let source = || async { Some(owned.source(&owned.key)) };

	assert!(
		write(&owned.store, source, "thread", old.as_str(), "scoped", "restored").await.is_err(),
		"A-B-A settings must not revive an old review without owner event draining"
	);

	let AgentModelSelectionState::Available { review_token, .. } =
		agent_models::read(&owned.store, source).await
	else {
		panic!("fresh review")
	};

	assert_ne!(review_token, old);

	review_token
}

#[tokio::test]
async fn legacy_model_request_blocks_current_service_mutations_without_native_writes() {
	let home = tempfile::tempdir().expect("fixture home");
	let (local, remote) = io::duplex(32_768);
	let (r, w) = io::split(local);
	let (client, _events) = AppServerClient::from_io(r, w);
	let writes = Arc::new(AtomicUsize::new(0));
	let backend = tokio::spawn(serve(remote, writes.clone(), "queued"));

	client.thread_resume(serde_json::json!({"threadId":"thread"})).await.expect("hydrate");

	let owned = OwnedReviewer::new(home.path(), &client, "thread", "turn").await;
	let connection = Connection::open(owned.root.paths().product_database_file()).unwrap();
	let attempt = serde_json::json!({"work":"root","thread":"thread","generation":owned.key.generation.as_str(),"account":owned.key.account.as_str(),"account_revision":owned.key.revision,"settings_event":1,"banner_digest":"b".repeat(64),"from_model":"original","model":"scoped","effort":"high","service_tier":"priority"});

	connection.execute("INSERT INTO agent_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) VALUES('model-recovery:legacy-service','root','model_recovery',?1,1,'resolved','Legacy fixture',1)",[serde_json::json!({"attempt":attempt,"state":"claimed"}).to_string()]).unwrap();

	let reservation = connection.last_insert_rowid();

	connection.execute("INSERT INTO agent_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) VALUES('model-recovery:legacy-service:result','root','model_recovery_result',?1,2,'resolved','Legacy response',2)",[serde_json::json!({"reservation":reservation,"state":"uncertain"}).to_string()]).unwrap();

	drop(connection);

	let source = || async { Some(owned.source(&owned.key)) };
	let state = agent_models::read(&owned.store, source).await;

	assert!(
		matches!(state,AgentModelSelectionState::Pending{model,state:decodex_protocol::AgentModelOutcome::Unknown,last_receipt:Some(decodex_protocol::AgentModelSelectionReceipt{manual:false,response:decodex_protocol::AgentModelResponse::Unknown,target_observed:false,reconciled:false,..}),..} if model.as_str()=="scoped")
	);
	assert!(write(&owned.store, source, "thread", "old-review", "scoped", "retry").await.is_err());
	assert!(matches!(
		agent_permissions::read(&owned.store, source).await,
		AgentPermissionState::Available { can_update: false, .. }
	));
	assert_eq!(writes.load(Ordering::Acquire), 0);

	let reopened = SqliteStore::open(&owned.root.paths()).unwrap();

	assert_eq!(
		reopened
			.pending_agent_legacy_model_change(
				"root".into(),
				"thread".into(),
				owned.key.generation.as_str().into()
			)
			.await
			.unwrap()
			.unwrap()
			.state,
		"unknown"
	);
	// Historical confirmation must stay visible even after native settings change again.
	let connection = Connection::open(owned.root.paths().product_database_file()).unwrap();

	connection.execute("INSERT INTO agent_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) VALUES('model-recovery:legacy-service:observation','root','model_recovery_observation',?1,3,'resolved','Historical fixture',3)",[serde_json::json!({"reservation":reservation,"settingsEvent":1,"state":"target_observed"}).to_string()]).unwrap();

	drop(connection);

	let state = agent_models::read(&reopened, source).await;
	let AgentModelSelectionState::Available { ref review_token, .. } = state else {
		panic!("historical receipt with current settings")
	};
	let previous_review = review_token.clone();

	assert!(matches!(state, AgentModelSelectionState::Available {
		model, last_receipt: Some(decodex_protocol::AgentModelSelectionReceipt {
			model: requested, manual: false, response: decodex_protocol::AgentModelResponse::Unknown,
			target_observed: true, reconciled: false, ..
		}), ..
	} if model.as_str() == "original" && requested.as_str() == "scoped"));

	let connection = Connection::open(owned.root.paths().product_database_file()).unwrap();

	connection.execute("INSERT INTO agent_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) VALUES('model-recovery:legacy-service:reconciliation','root','model_selection_reconciled',?1,4,'resolved','Historical reconciliation',4)",[serde_json::json!({"reservation":reservation,"settingsEvent":1,"generationId":owned.key.generation.as_str()}).to_string()]).unwrap();

	drop(connection);

	let state = agent_models::read(&reopened, source).await;

	assert!(matches!(state, AgentModelSelectionState::Available {
		review_token, last_receipt: Some(decodex_protocol::AgentModelSelectionReceipt {
			response: decodex_protocol::AgentModelResponse::Unknown, reconciled: true, ..
		}), ..
	} if review_token != previous_review));
	assert!(
		write(&reopened, source, "thread", previous_review.as_str(), "scoped", "stale-history")
			.await
			.is_err()
	);
	assert_eq!(writes.load(Ordering::Acquire), 0, "history reads never replay the old request");

	backend.abort();
}
