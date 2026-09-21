//! Model observations must retain exact ownership across the native read.
use super::*;
use decodex_protocol::ChiefModelSettingsResult as Result;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[tokio::test]
async fn model_settings_discard_changed_sources_and_preserve_null_metadata() {
	for change in [
		"none", "null", "missing", "account", "revision", "history", "process", "thread", "work",
		"closed",
	] {
		let home = tempfile::tempdir().unwrap();
		let (local, remote) = tokio::io::duplex(4096);
		let (r, w) = tokio::io::split(local);
		let (client, _events) = AppServerClient::from_io(r, w);
		let owner = OwnedReviewer::new(home.path(), &client, "thread", "turn").await;
		let server = tokio::spawn(async move {
			let (r, mut w) = tokio::io::split(remote);
			let mut lines = BufReader::new(r).lines();
			let request: serde_json::Value =
				serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
			assert_eq!(request["method"], "thread/read");
			assert_eq!(
				request["params"],
				serde_json::json!({"threadId":"thread","includeTurns":false})
			);
			let thread = match change {
				"missing" => serde_json::json!({"id":"thread"}),
				"null" => serde_json::json!({"id":"thread","model":null,"reasoningEffort":null}),
				_ =>
					serde_json::json!({"id":"thread","model":"configured-model","reasoningEffort":"future-effort"}),
			};
			w.write_all(
				format!("{}\n", serde_json::json!({"id":request["id"],"result":{"thread":thread}}))
					.as_bytes(),
			)
			.await
			.unwrap();
		});
		let calls = AtomicUsize::new(0);
		let result = crate::chief_model_settings::read(&owner.store, || {
			let later = calls.fetch_add(1, Ordering::SeqCst) > 0;
			let mut key = owner.key.clone();
			if later {
				match change {
					"account" =>
						key.account =
							AccountId::new("50000000-0000-4000-8000-000000000001").unwrap(),
					"revision" => key.revision += 1,
					"history" => key.history_revision += 1,
					"process" =>
						key.generation =
							ProcessGenerationId::new("60000000-0000-4000-8000-000000000001")
								.unwrap(),
					"thread" => key.thread = "other".into(),
					"work" => key.work = "other".into(),
					_ => {},
				}
			}
			let source = (!(later && change == "closed")).then(|| owner.source(&key));
			async move { source }
		})
		.await;
		server.await.unwrap();
		match change {
			"none" => assert!(
				matches!(result, Result::Available { model: Some(ref model), reasoning_effort: Some(ref effort), .. } if model.as_str()=="configured-model" && effort.as_str()=="future-effort")
			),
			"null" => assert!(matches!(
				result,
				Result::Available { model: None, reasoning_effort: None, .. }
			)),
			"missing" => assert_eq!(result, Result::NotReported),
			_ => assert_eq!(result, Result::Unavailable, "{change}"),
		}
		assert!(owner.store.list_pending_chief_events(100).await.unwrap().is_empty());
	}
}

#[tokio::test]
async fn model_settings_do_not_read_a_foreign_thread() {
	let home = tempfile::tempdir().unwrap();
	let (local, mut remote) = tokio::io::duplex(4096);
	let (r, w) = tokio::io::split(local);
	let (client, _events) = AppServerClient::from_io(r, w);
	let owner = OwnedReviewer::new(home.path(), &client, "thread", "turn").await;
	let mut key = owner.key.clone();
	key.thread = "foreign".into();
	assert_eq!(
		crate::chief_model_settings::read(&owner.store, || async { Some(owner.source(&key)) })
			.await,
		Result::Unavailable
	);
	let mut byte = [0];
	assert!(
		tokio::time::timeout(
			std::time::Duration::from_millis(25),
			tokio::io::AsyncReadExt::read(&mut remote, &mut byte)
		)
		.await
		.is_err()
	);
}
