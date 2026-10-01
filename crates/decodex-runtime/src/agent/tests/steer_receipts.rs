use super::*;

#[tokio::test]
async fn lost_steer_reply_is_resolved_by_exact_live_or_cold_receipt_without_replay() {
	for mode in ["live", "running", "idle"] {
		let (mut agent, mut sent, directory) =
			fixture_with_history(json!({"_steer_disconnect":true})).await;

		agent.start_agent("agent", "Supplement").await.unwrap();

		while sent.try_recv().is_ok() {}

		assert!(
			agent
				.steer_work("agent", "opaque turn/1", "exact-submission", "Supplement", &[])
				.await
				.is_err()
		);
		assert_eq!(sent.recv().await.unwrap()["method"], "turn/steer");
		assert!(sent.try_recv().is_err());

		let pending = agent
			.store
			.list_agent_events_for_turn("opaque turn/1".into(), 100)
			.await
			.unwrap()
			.into_iter()
			.find(|e| e.event_kind == "steer_pending")
			.unwrap()
			.id;
		let receipt = |id: &str| json!({"id":format!("item-{id}"),"type":"userMessage","clientId":id,"content":[]});
		let root =
			decodex_core::DecodexRoot::new(directory.path().canonicalize().unwrap().join("root"))
				.unwrap();

		if mode != "live" {
			if mode == "idle" {
				agent
					.store
					.complete_agent_turn("agent".into(), "opaque turn/1".into())
					.await
					.unwrap();
			}

			drop(agent);

			let history = json!({"opaque thread/1":{"thread":{"id":"opaque thread/1","status":{"type":if mode == "idle" {"idle"} else {"active"}},"turns":[{"id":"opaque turn/1","status":if mode == "idle" {"completed"} else {"inProgress"},"items":[receipt("older-submission"),receipt("exact-submission")]}]}}});
			let (mut recovered, mut calls, _new_directory) = fixture_with_history(history).await;

			recovered.store = SqliteStore::open(&root.paths()).unwrap();

			recovered.recover_persisted().await.unwrap();

			assert!(
				recovered.store.get_agent_inbox_event(pending).await.unwrap().disposition.is_some()
			);
			assert!(
				std::iter::from_fn(|| calls.try_recv().ok())
					.all(|r| r["method"] != "turn/start" && r["method"] != "turn/steer")
			);
		} else {
			for (id, accepted) in [
				("older-submission", false),
				("exact-submission", true),
				("exact-submission", true),
			] {
				agent
					.handle_event(ServerEvent::Notification {
						method: "item/completed".into(),
						params: json!({"threadId":"opaque thread/1","turnId":"opaque turn/1","item":receipt(id)}),
					})
					.await
					.unwrap();

				assert_eq!(
					agent.store.get_agent_inbox_event(pending).await.unwrap().disposition.is_some(),
					accepted
				);
			}

			assert!(sent.try_recv().is_err());
		}

		let reopened = SqliteStore::open(&root.paths()).unwrap();
		let (rows, _) = reopened.read_agent_transcript("agent".into(), None, 100).await.unwrap();
		let expected_receipt = json!(["steer_receipt", pending]).to_string();

		assert_eq!(
			rows.iter().filter(|e| e.source_event_id == expected_receipt).count(),
			1,
			"{mode}"
		);
		assert!(reopened.list_undelivered_agent_events(100).await.unwrap().is_empty());
	}
}
