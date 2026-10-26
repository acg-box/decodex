use crate::{AgentVoiceCall, SqliteStore, agent_process::tests};

#[tokio::test]
async fn selected_conversation_actions_keep_host_and_thread_identity_across_manager_roles() {
	let directory = tempfile::tempdir().unwrap();
	let store = SqliteStore::open_test(&directory.path().join("restore.sqlite3")).unwrap();

	tests::seed(&store).await;

	let mut conversation = store.get_agent_work_item("root".into()).await.unwrap();

	conversation.id = "conversation".into();
	conversation.parent_goal_id = Some("root".into());

	store.create_agent_manager(conversation).await.unwrap();
	store.bind_agent_thread("conversation".into(), "native-chat".into()).await.unwrap();
	store.bind_agent_thread("second-root".into(), "other-native".into()).await.unwrap();
	store
		.prepare_agent_bound_process_generation(
			&tests::intent(1, 1),
			&tests::binding(1),
			"root",
			"first",
		)
		.await
		.unwrap();
	store
		.bind_process_generation_identity(&tests::generation_id(1), 1, &tests::identity(123))
		.await
		.unwrap();
	store.mark_process_generation_ready(&tests::generation_id(1), 2).await.unwrap();

	let generation = Some(tests::generation_id(1).as_str().to_owned());

	for (work, thread, allowed) in [
		("conversation", "native-chat", true),
		("conversation", "other-native", false),
		("second-root", "other-native", false),
	] {
		assert_eq!(
			store
				.agent_thread_is_restorable(work.into(), thread.into(), generation.clone())
				.await
				.unwrap(),
			allowed
		);

		let call = AgentVoiceCall {
			session_id: format!("voice-{work}-{thread}"),
			work_id: work.into(),
			thread_id: thread.into(),
			generation_id: tests::generation_id(1).as_str().into(),
			baseline_turn_id: None,
		};

		assert_eq!(store.begin_agent_voice_call(call).await.is_ok(), allowed);
	}

	assert_eq!(store.open_agent_voice_calls().await.unwrap().len(), 1);
	assert!(
		store
			.begin_agent_voice_call(AgentVoiceCall {
				session_id: "old-host-voice".into(),
				work_id: "conversation".into(),
				thread_id: "native-chat".into(),
				generation_id: tests::generation_id(2).as_str().into(),
				baseline_turn_id: None,
			})
			.await
			.is_err()
	);
	assert!(
		!store
			.agent_thread_is_restorable(
				"conversation".into(),
				"native-chat".into(),
				Some(tests::generation_id(2).as_str().into())
			)
			.await
			.unwrap()
	);
	assert!(
		!store
			.agent_thread_is_restorable("conversation".into(), "native-chat".into(), None)
			.await
			.unwrap()
	);
	assert!(
		!store
			.agent_thread_is_owned("conversation".into(), "native-chat".into(), generation)
			.await
			.unwrap()
	);
}
