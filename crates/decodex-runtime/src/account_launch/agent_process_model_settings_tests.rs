//! Source-bound live model reviews and durable publication outcomes.
use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[tokio::test]
async fn live_model_publication_checks_capabilities_and_keeps_uncertain_receipts() {
	use crate::agent_live_settings::{LiveEdit, read, write};

	use decodex_protocol::{
		AgentLiveReviewerState, ConversationModel, ConversationReasoningEffort,
	};

	use serde_json::json;

	for case in ["applied", "lost", "disabled", "unsupported", "changed"] {
		let home = tempfile::tempdir().unwrap();
		let (local, remote) = tokio::io::duplex(16_384);
		let (r, w) = tokio::io::split(local);
		let (client, _events) = AppServerClient::from_io(r, w);
		let owner = OwnedReviewer::new(home.path(), &client, "thread", "turn").await;
		let AgentLiveReviewerState::Available { review_token, .. } =
			read(&owner.store, || async { Some(owner.source(&owner.key)) }).await
		else {
			panic!("owned turn");
		};
		let (release, released) = tokio::sync::oneshot::channel();
		let server = tokio::spawn(async move {
			let (r, mut w) = tokio::io::split(remote);
			let mut lines = BufReader::new(r).lines();

			for phase in 0..3 {
				let request: serde_json::Value =
					serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
				let result = if phase == 0 {
					assert_eq!(request["method"], "model/list");

					json!({"data":[{"model":"selected","displayName":"Selected","supportedReasoningEfforts":[{"reasoningEffort":if case=="unsupported" {"low"} else {"high"}}]}],"nextCursor":null})
				} else {
					assert_eq!(request["method"], "experimentalFeature/list");

					if phase == 2 {
						assert_eq!(request["params"]["threadId"], "thread");
					}

					json!({"data":[{"name":"memories","enabled":false},{"name":"step_model_switching","enabled":case!="disabled"}],"nextCursor":null})
				};

				w.write_all(
					format!("{}\n", json!({"id":request["id"],"result":result})).as_bytes(),
				)
				.await
				.unwrap();
			}

			if matches!(case, "applied" | "lost") {
				let request: serde_json::Value =
					serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();

				assert_eq!(request["method"], "turn/settings/update");
				assert_eq!(
					request["params"],
					json!({"threadId":"thread","turnId":"turn","model":"selected","effort":"high"})
				);

				if case == "lost" {
					return;
				}

				w.write_all(
					format!("{}\n", json!({"id":request["id"],"result":{"status":"applied"}}))
						.as_bytes(),
				)
				.await
				.unwrap();
			}

			let _ = released.await;

			assert!(
				tokio::time::timeout(std::time::Duration::from_millis(20), lines.next_line())
					.await
					.is_err(),
				"unexpected publication or retry"
			);
		});
		let calls = AtomicUsize::new(0);
		let outcome = write(
			&owner.store,
			|| {
				let mut key = owner.key.clone();

				if calls.fetch_add(1, Ordering::SeqCst) > 0 && case == "changed" {
					key.revision += 1;
				}

				let source = owner.source(&key);

				async move { Some(source) }
			},
			"turn",
			review_token.as_str(),
			LiveEdit::Model {
				model: ConversationModel::new("selected").unwrap(),
				effort: ConversationReasoningEffort::High,
			},
			"model-edit",
		)
		.await;

		assert_eq!(outcome.is_ok(), case == "applied", "{case}");

		let reopened = SqliteStore::open(&owner.root.paths()).unwrap();
		let receipt = reopened
			.agent_live_settings_receipt("root".into(), "thread".into(), "turn".into())
			.await
			.unwrap();

		match case {
			"applied" | "lost" => assert_eq!(
				receipt.unwrap().outcome,
				if case == "applied" { "applied" } else { "unknown" }
			),
			_ => assert!(receipt.is_none(), "rejected before reservation: {case}"),
		}

		assert!(reopened.list_pending_agent_events(100).await.unwrap().is_empty());

		let _ = release.send(());

		server.await.unwrap();
	}
}

#[tokio::test]
async fn live_model_choices_are_bound_to_the_task_and_discarded_after_source_change() {
	use serde_json::json;

	for changed in [false, true] {
		let home = tempfile::tempdir().unwrap();
		let (local, remote) = tokio::io::duplex(16_384);
		let (r, w) = tokio::io::split(local);
		let (client, _events) = AppServerClient::from_io(r, w);
		let owner = OwnedReviewer::new(home.path(), &client, "thread", "turn").await;
		let (release, released) = tokio::sync::oneshot::channel();
		let server = tokio::spawn(async move {
			let (r, mut w) = tokio::io::split(remote);
			let mut lines = BufReader::new(r).lines();

			for phase in 0..3 {
				let request: serde_json::Value =
					serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
				let result = if phase == 1 {
					assert_eq!(request["method"], "model/list");

					json!({"data":[{"model":"selected","displayName":"Selected","supportedReasoningEfforts":[{"reasoningEffort":"high"}]}],"nextCursor":null})
				} else {
					assert_eq!(request["method"], "experimentalFeature/list");

					if phase == 0 {
						assert_eq!(request["params"]["threadId"], "thread");
					}

					json!({"data":[{"name":"step_model_switching","enabled":true},{"name":"memories","enabled":false}],"nextCursor":null})
				};

				w.write_all(
					format!("{}\n", json!({"id":request["id"],"result":result})).as_bytes(),
				)
				.await
				.unwrap();
			}

			let _ = released.await;
		});
		let calls = AtomicUsize::new(0);
		let state = crate::agent_live_settings::read_options(&owner.store, true, || {
			let mut key = owner.key.clone();

			if changed && calls.fetch_add(1, Ordering::SeqCst) > 0 {
				key.revision += 1;
			}

			let source = owner.source(&key);

			async move { Some(source) }
		})
		.await;

		if changed {
			assert_eq!(state, decodex_protocol::AgentLiveReviewerState::Unavailable);
		} else {
			assert!(
				matches!(state,decodex_protocol::AgentLiveReviewerState::Available { model_choices:Some(ref models),..} if models.len()==1 && models[0].model.as_str()=="selected")
			);
		}

		let _ = release.send(());

		server.await.unwrap();
	}
}
