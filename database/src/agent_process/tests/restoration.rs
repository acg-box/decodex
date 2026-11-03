use crate::{AgentVoiceCall, SqliteStore, agent_process::tests};

#[tokio::test]
async fn selected_conversation_actions_keep_host_and_thread_identity_across_manager_roles() {
	let (_directory, store) = hosted_store().await;
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

async fn hosted_store() -> (tempfile::TempDir, SqliteStore) {
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

	(directory, store)
}

#[tokio::test]
async fn reviewed_history_allows_a_hosted_manager_without_granting_agent_tool_authority() {
	use crate::{AgentForkAttempt, AgentForkBoundary, AgentPromptEditAttempt};
	let (_directory, store) = hosted_store().await;
	let source = store.get_agent_work_item("conversation".into()).await.unwrap();
	let review = AgentPromptEditAttempt {
		work: "conversation".into(),
		thread: "native-chat".into(),
		generation: Some(tests::generation_id(1).as_str().into()),
		review_token: "a".repeat(64),
		attempt_id: "review".into(),
		before_turn_id: "turn".into(),
		item_id: "input".into(),
		turn_ids: vec!["turn".into()],
		content: vec![serde_json::json!({"type":"text","text":"Source input"})],
	};
	let branch = AgentForkAttempt {
		source: review.clone(),
		target_work: "side-chat".into(),
		boundary: AgentForkBoundary::AttachedAfterTurn,
		model_provider: None,
	};
	for (work, thread, generation) in [
		("conversation", "other-native", review.generation.clone()),
		("second-root", "other-native", review.generation.clone()),
		("conversation", "native-chat", Some(tests::generation_id(2).as_str().into())),
		("conversation", "native-chat", None),
	] {
		let mut invalid = review.clone();
		invalid.work = work.into();
		invalid.thread = thread.into();
		invalid.generation = generation;
		assert!(store.reserve_agent_prompt_edit(invalid.clone()).await.unwrap().is_none());
		let invalid = AgentForkAttempt { source: invalid, ..branch.clone() };
		assert!(store.reserve_agent_fork(invalid).await.unwrap().is_none());
	}
	let id = store.reserve_agent_prompt_edit(review.clone()).await.unwrap().unwrap();
	assert!(store.reject_agent_prompt_edit_without_mutation(id, review.clone()).await.unwrap());
	assert!(store.reserve_agent_fork(branch).await.unwrap().is_some());
	assert_eq!(store.get_agent_work_item("conversation".into()).await.unwrap(), source);
	assert!(
		!store
			.agent_thread_is_owned("conversation".into(), "native-chat".into(), review.generation)
			.await
			.unwrap()
	);
}
