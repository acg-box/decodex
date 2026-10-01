//! Transport outcomes must survive durable recording without replay.
use std::{
	sync::{
		Arc,
		atomic::{AtomicBool, AtomicUsize, Ordering},
	},
	time::Duration,
};

use serde_json::{self, Value};
use tokio::{
	io::{self, AsyncBufReadExt as _, AsyncWriteExt as _, BufReader},
	sync::{mpsc, oneshot},
	task, time,
};

use crate::{
	account_launch::agent_process::native_tests::reviewer::store::{
		AgentLiveReviewerState, AgentReviewer, OwnedReviewer, SqliteStore,
	},
	agent_live_settings,
};
use decodex_codex::app_server_client::AppServerClient;

#[tokio::test]
async fn reviewer_publication_records_remote_and_uncertain_outcomes_without_replay() {
	for (scenario, expected) in [
		("applied", "applied"),
		("unavailable", "target_unavailable"),
		("rejected", "rejected"),
		("lost", "unknown"),
		("malformed", "unknown"),
		("source_changed", "unknown"),
	] {
		let home = tempfile::tempdir().unwrap();
		let (local, remote) = io::duplex(4_096);
		let (read, write) = io::split(local);
		let (client, _events) = AppServerClient::from_io(read, write);
		let owned = OwnedReviewer::new(home.path(), &client, "thread", "turn").await;
		let state =
			agent_live_settings::read(&owned.store, || async { Some(owned.source(&owned.key)) })
				.await;
		let AgentLiveReviewerState::Available { review_token, .. } = state else {
			panic!("review")
		};
		let changed = Arc::new(AtomicBool::new(false));
		let server_changed = changed.clone();
		let (release, held) = oneshot::channel::<()>();
		let server = tokio::spawn(async move {
			let (read, mut write) = io::split(remote);
			let mut lines = BufReader::new(read).lines();
			let request: Value =
				serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();

			assert_eq!(request["method"], "turn/settings/update");
			assert_eq!(
				request["params"],
				serde_json::json!({"threadId":"thread","turnId":"turn","approvalsReviewer":"user"})
			);

			if scenario == "lost" {
				return;
			}

			let response = match scenario {
				"rejected" =>
					serde_json::json!({"id":request["id"],"error":{"code":-32_600,"message":"managed requirement"}}),
				"unavailable" =>
					serde_json::json!({"id":request["id"],"result":{"status":"targetUnavailable"}}),
				"malformed" =>
					serde_json::json!({"id":request["id"],"result":{"status":"unexpected"}}),
				_ => serde_json::json!({"id":request["id"],"result":{"status":"applied"}}),
			};

			server_changed.store(scenario == "source_changed", Ordering::Release);
			write.write_all(format!("{response}\n").as_bytes()).await.unwrap();

			let _ = held.await;

			assert!(
				time::timeout(std::time::Duration::from_millis(30), lines.next_line())
					.await
					.is_err(),
				"no automatic or duplicate publication"
			);
		});
		let result = agent_live_settings::write(
			&owned.store,
			|| async {
				let mut key = owned.key.clone();

				key.revision += i64::from(changed.load(Ordering::Acquire));

				Some(owned.source(&key))
			},
			"turn",
			review_token.as_str(),
			AgentReviewer::User.into(),
			"attempt",
		)
		.await;

		assert_eq!(result.is_ok(), expected == "applied", "{scenario}");

		let reopened = SqliteStore::open(&owned.root.paths()).unwrap();
		let receipt = reopened
			.agent_live_settings_receipt("root".into(), "thread".into(), "turn".into())
			.await
			.unwrap()
			.unwrap();

		assert_eq!(receipt.outcome, expected, "{scenario}");
		assert!(reopened.list_pending_agent_events(10).await.unwrap().is_empty());
		assert!(
			agent_live_settings::write(
				&owned.store,
				|| async { Some(owned.source(&owned.key)) },
				"turn",
				review_token.as_str(),
				AgentReviewer::AutoReview.into(),
				"duplicate"
			)
			.await
			.is_err()
		);

		let _ = release.send(());

		server.await.unwrap();
	}
}

#[tokio::test]
async fn local_queue_refusal_is_durably_rejected_even_if_source_changes_afterward() {
	let home = tempfile::tempdir().unwrap();
	let (incoming, frames) = mpsc::channel(4);
	let (outgoing, mut requests) = mpsc::channel(1);
	let (client, _events) = AppServerClient::from_framed(1, frames, outgoing).unwrap();
	let owned = OwnedReviewer::new(home.path(), &client, "thread", "turn").await;
	let state =
		agent_live_settings::read(&owned.store, || async { Some(owned.source(&owned.key)) }).await;
	let AgentLiveReviewerState::Available { review_token, .. } = state else { panic!("review") };
	let peer = client.clone();
	let pending =
		tokio::spawn(
			async move { peer.thread_read(serde_json::json!({"threadId":"other"})).await },
		);

	time::timeout(Duration::from_secs(2), async {
		while requests.is_empty() {
			task::yield_now().await;
		}
	})
	.await
	.unwrap();

	let observations = AtomicUsize::new(0);
	let result = agent_live_settings::write(
		&owned.store,
		|| async {
			(observations.fetch_add(1, Ordering::AcqRel) < 3).then(|| owned.source(&owned.key))
		},
		"turn",
		review_token.as_str(),
		AgentReviewer::User.into(),
		"refused",
	)
	.await;

	assert!(matches!(result, Err(crate::agent_host::AgentHostError::Rejected(_))));

	let receipt = owned
		.store
		.agent_live_settings_receipt("root".into(), "thread".into(), "turn".into())
		.await
		.unwrap()
		.unwrap();

	assert_eq!(receipt.outcome, "rejected");

	let request = requests.recv().await.unwrap();

	assert_eq!(request["method"], "thread/read");
	assert_eq!(request["params"]["threadId"], "other");
	assert!(requests.try_recv().is_err(), "reviewer update never left the local queue");

	incoming.send(Ok(serde_json::json!({"id":request["id"],"result":{}}))).await.unwrap();
	pending.await.unwrap().unwrap();

	assert!(owned.store.list_pending_agent_events(10).await.unwrap().is_empty());
}
