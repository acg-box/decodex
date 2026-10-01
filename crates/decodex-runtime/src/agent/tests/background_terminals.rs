use super::*;

#[tokio::test]
async fn background_commands_keep_native_identity_and_do_not_start_turns() {
	for terminated in [true, false] {
		let (mut agent,mut sent,_directory)=fixture_with_history(json!({"_background":{"data":[{"processId":"712","itemId":"item","command":"sleep 90","cwd":"/fixture"}],"nextCursor":null},"_terminated":terminated})).await;
		let manager = agent.start_agent("agent", "Coordinate").await.unwrap();

		while sent.try_recv().is_ok() {}

		let mut args = json!({"id":"agent","threadId":manager.codex_thread_id,"operation":"list"});
		let page = agent.background_terminals(&manager, &args).await.unwrap();

		assert_eq!(page["terminals"][0]["processId"], "712");
		assert_eq!(page["terminals"][0]["commandTruncated"], false);

		args["operation"] = json!("terminate");
		args["processId"] = json!("712");

		assert_eq!(
			agent.background_terminals(&manager, &args).await.unwrap()["terminated"],
			terminated
		);

		let requests: Vec<_> = std::iter::from_fn(|| sent.try_recv().ok()).collect();

		assert_eq!(requests.len(), 2);
		assert_eq!(requests[0]["method"], "thread/backgroundTerminals/list");
		assert_eq!(requests[1]["method"], "thread/backgroundTerminals/terminate");
		assert_eq!(
			requests[1]["params"],
			json!({"threadId":manager.codex_thread_id,"processId":"712"})
		);
		assert_eq!(
			agent.store.get_agent_work_item("agent".into()).await.unwrap().active_turn_id,
			manager.active_turn_id
		);
	}
}

#[tokio::test]
async fn background_commands_reject_foreign_worker_and_stale_thread_before_rpc() {
	let (mut agent, mut sent, _directory) = fixture().await;
	let manager = agent.start_agent("agent", "Coordinate").await.unwrap();

	agent.create_manager("agent", "child", "Manage", None).await.unwrap();

	let worker = agent.create_worker("child", "worker", "Work").await.unwrap();
	let reference = agent.store.begin_agent_steer("agent".into(),manager.active_turn_id.clone().unwrap(),"read-reference".into(),
		json!({"text":"Read selected work","source":"user","options":{"taskReferences":[{"workId":"worker","threadId":worker.codex_thread_id,"title":"Selected work"}]}}).to_string()).await.unwrap();

	agent.store.finish_agent_steer(reference, true).await.unwrap();

	assert!(
		agent
			.store
			.agent_has_task_reference(
				"agent".into(),
				"worker".into(),
				worker.codex_thread_id.clone().unwrap()
			)
			.await
			.unwrap()
	);

	while sent.try_recv().is_ok() {}

	for operation in ["list", "terminate"] {
		let args = json!({"id":"worker","threadId":worker.codex_thread_id,"operation":operation,"processId":"712"});

		assert!(agent.background_terminals(&manager, &args).await.is_err());
		assert!(agent.background_terminals(&worker, &args).await.is_err());
		assert!(
			agent
				.background_terminals(
					&manager,
					&json!({"id":"agent","threadId":"old-thread","operation":operation,"processId":"712"})
				)
				.await
				.is_err()
		);
	}

	assert!(sent.try_recv().is_err());
}
