use super::*;

#[tokio::test]
async fn native_changes_during_question_rebuild_preserve_recovery_until_fresh_read() {
	for change in ["revert", "input", "disconnect"] {
		let (mut chief, mut sent, _directory) = fixture().await;
		chief.start_chief("chief", "Coordinate").await.unwrap();
		let question = json!({"id":"retained-question","type":"agentMessage","delivery":"async","questions":[{"title":"Continue?"}]});
		chief.observe_async_question_item("opaque thread/1", "old", &question).await.unwrap();
		let queued = chief.store.enqueue_chief_event(EnqueueChiefEvent {
			source_event_id: "pending-answer".into(),
			work_item_id: "chief".into(),
			event_kind: "async_question_answer".into(),
			payload: json!({"text":"Yes","asyncQuestionId":decodex_protocol::chief_async_question_id("retained-question",0)}).to_string(),
		}).await.unwrap();
		chief.store.queue_chief_async_reconnection().await.unwrap();
		while sent.try_recv().is_ok() {}
		let (incoming, frames) = tokio::sync::mpsc::channel(8);
		let (outgoing, mut writes) = tokio::sync::mpsc::channel(8);
		let (client, _events) = AppServerClient::from_framed(1, frames, outgoing).unwrap();
		chief.client = client.clone();
		let (release, finished) = tokio::sync::oneshot::channel();
		let server = tokio::spawn(async move {
			for index in 0..4 {
				let request = writes.recv().await.unwrap();
				assert_eq!(request["method"], "thread/read");
				assert_eq!(request["params"]["threadId"], "opaque thread/1");
				if index == 3 {
					if change == "disconnect" {
						return;
					}
					let notification = if change == "revert" {
						json!({"method":"thread/reverted","params":{"threadId":"opaque thread/1"}})
					} else {
						json!({"method":"item/completed","params":{"threadId":"opaque thread/1","turnId":"new","item":{"id":"new-prompt","type":"userMessage","content":[{"type":"text","text":"A new task"}]}}})
					};
					incoming.send(Ok(notification)).await.unwrap();
				}
				// A stale complete read with no questions would delete the retained card and
				// answer.
				incoming.send(Ok(json!({"id":request["id"],"result":{"thread":{"id":"opaque thread/1","historyMode":"legacy","turns":[{"id":"old","status":"completed","items":[]}]}}}))).await.unwrap();
			}
			let _ = finished.await;
		});
		tokio::time::timeout(std::time::Duration::from_secs(3), chief.recover_async_questions())
			.await
			.unwrap()
			.unwrap();
		let _ = release.send(());
		server.await.unwrap();
		assert_eq!(client.question_revision(), u64::from(change != "disconnect"));
		assert!(chief.store.chief_async_questions_recovering("chief".into()).await.unwrap());
		assert!(chief.store.read_chief_async_questions("chief".into()).await.unwrap().is_empty());
		assert!(chief.store.get_chief_inbox_event(queued.id).await.unwrap().disposition.is_none());
		assert!(sent.try_recv().is_err());

		let history = json!({"opaque thread/1":{"thread":{"id":"opaque thread/1","turns":[{"id":"old","status":"completed","items":[question]}]}}});
		let (mut fresh, mut reads, _other_directory) = fixture_with_history(history).await;
		fresh.store = chief.store.clone();
		fresh.recover_async_questions().await.unwrap();
		assert!(!fresh.store.chief_async_questions_recovering("chief".into()).await.unwrap());
		let questions = fresh.store.read_chief_async_questions("chief".into()).await.unwrap();
		assert_eq!(questions.len(), 1);
		assert_eq!(questions[0].item_id, "retained-question");
		assert!(fresh.store.get_chief_inbox_event(queued.id).await.unwrap().disposition.is_none());
		while let Ok(request) = reads.try_recv() {
			assert_eq!(request["method"], "thread/read");
		}
	}
}

#[tokio::test]
async fn only_live_item_events_mark_question_arrivals() {
	let (mut chief, mut sent, _directory) = fixture().await;
	chief.start_chief("chief", "Coordinate").await.unwrap();
	while sent.try_recv().is_ok() {}
	let question = |id: &str| json!({"id":id,"type":"agentMessage","delivery":"async","questions":[{"title":"Continue?"}]});
	chief
		.observe_async_question_item("opaque thread/1", "old", &question("history"))
		.await
		.unwrap();
	for id in ["history", "live", "live"] {
		chief
			.handle_event(ServerEvent::Notification {
				method: "item/completed".into(),
				params: json!({"threadId":"opaque thread/1","turnId":"old","item":question(id)}),
			})
			.await
			.unwrap();
	}
	let questions = chief.store.read_chief_async_questions("chief".into()).await.unwrap();
	assert_eq!(questions.len(), 2);
	assert!(!questions[0].arrived_live);
	assert!(questions[1].arrived_live);
	assert!(sent.try_recv().is_err(), "arrival observation cannot submit a turn");
}
