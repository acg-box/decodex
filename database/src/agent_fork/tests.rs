use crate::{
	AgentDispatchState, AgentForkAttempt, AgentForkBoundary, AgentForkReceipt,
	AgentPromptEditAttempt, AgentWorkItem, AgentWorkKind, AgentWorkStatus, SqliteStore,
};

fn attempt(boundary: AgentForkBoundary) -> AgentForkAttempt {
	AgentForkAttempt {
		model_provider: None,
		source: AgentPromptEditAttempt {
			work: "main".into(),
			thread: "source-native".into(),
			generation: None,
			review_token: "a".repeat(64),
			attempt_id: "review".into(),
			before_turn_id: "first".into(),
			item_id: "input".into(),
			turn_ids: vec!["first".into(), "second".into()],
			content: vec![
				serde_json::json!({"type":"text","text":"Original input"}),
				serde_json::json!({"type":"image","fileId":"native-file"}),
				serde_json::json!({"type":"skill","name":"review","path":"/skills/review (local)/SKILL.md"}),
			],
		},
		target_work: "branch".into(),
		boundary,
	}
}

async fn seed(store: &SqliteStore) {
	store
		.create_agent_work_item(AgentWorkItem {
			id: "main".into(),
			parent_goal_id: None,
			kind: AgentWorkKind::Goal,
			title: "Main".into(),
			instructions: "Original objective".into(),
			codex_thread_id: None,
			status: AgentWorkStatus::Open,
			next_check_at_micros: None,
			created_at_micros: 1,
			updated_at_micros: 1,
			active_turn_id: None,
			dispatch_state: AgentDispatchState::Idle,
		})
		.await
		.unwrap();
	store.bind_agent_thread("main".into(), "source-native".into()).await.unwrap();
}

async fn assert_fork_draft(
	store: &SqliteStore,
	boundary: AgentForkBoundary,
	a: &AgentForkAttempt,
	result: &AgentForkReceipt,
) {
	if boundary == AgentForkBoundary::BeforeInput {
		let edit = store
			.agent_prompt_edit_receipt("branch".into(), "fork-native".into())
			.await
			.unwrap()
			.unwrap();

		assert_eq!(edit.id, result.edit_receipt_id.unwrap());
		assert_eq!(edit.state, "applied");
		assert_eq!(edit.attempt.content, a.source.content);
		assert!(store.release_agent_prompt_edit_draft(edit.id, None).await.unwrap());
	} else {
		assert!(result.edit_receipt_id.is_none());
	}
}

#[tokio::test]
async fn fork_receipt_survives_restart_without_repeating_creation_or_changing_source() {
	for (boundary, before, expected) in [
		(AgentForkBoundary::BeforeInput, "first", vec![]),
		(AgentForkBoundary::AfterTurn, "first", vec!["first"]),
		(AgentForkBoundary::BeforeInput, "second", vec!["first"]),
		(AgentForkBoundary::AfterTurn, "second", vec!["first", "second"]),
		(AgentForkBoundary::AttachedAfterTurn, "second", vec![]),
	] {
		let expected: Vec<String> = expected.into_iter().map(String::from).collect();
		let directory = tempfile::tempdir().unwrap();
		let path = directory.path().join("fork.sqlite3");
		let store = SqliteStore::open_test(&path).unwrap();

		seed(&store).await;

		let original = store.get_agent_work_item("main".into()).await.unwrap();
		let mut a = attempt(boundary);

		a.source.before_turn_id = before.into();

		assert_eq!(a.expected_turns(), Some(expected.as_slice()));
		if boundary == AgentForkBoundary::AttachedAfterTurn {
			assert_eq!(a.source.turn_ids, ["first", "second"]);
			assert!(
				store
					.acknowledge_agent_fork(
						a.clone(),
						"fork-native".into(),
						vec!["first".into(), "second".into()]
					)
					.await
					.is_err()
			);
		}

		let saved = store.reserve_agent_fork(a.clone()).await.unwrap().unwrap();

		assert_eq!(saved.state, "reserved");
		assert!(
			store
				.agent_prompt_edit_receipt("main".into(), "source-native".into())
				.await
				.unwrap()
				.is_none()
		);

		drop(store);

		let store = SqliteStore::open_test(&path).unwrap();

		assert_eq!(
			store.agent_fork_receipt("main".into(), "a".repeat(64)).await.unwrap().unwrap(),
			saved
		);
		assert!(store.reserve_agent_fork(a.clone()).await.unwrap().is_none());
		assert_eq!(
			store.get_agent_work_item("branch".into()).await.unwrap().dispatch_state,
			AgentDispatchState::Dispatching
		);
		assert!(
			store
				.acknowledge_agent_fork(a.clone(), "source-native".into(), expected.clone())
				.await
				.is_err()
		);

		let acknowledged = store
			.record_agent_fork_identity(a.clone(), "fork-native".into())
			.await
			.unwrap()
			.unwrap();

		assert_eq!(acknowledged.state, "acknowledged");

		drop(store);

		let store = SqliteStore::open_test(&path).unwrap();

		assert_eq!(
			store
				.agent_fork_receipt("main".into(), a.source.review_token.clone())
				.await
				.unwrap()
				.unwrap(),
			acknowledged
		);
		assert!(
			store
				.record_agent_fork_identity(a.clone(), "different-fork".into())
				.await
				.unwrap()
				.is_none()
		);
		assert!(
			store
				.acknowledge_agent_fork(a.clone(), "fork-native".into(), vec!["wrong".into()])
				.await
				.is_err()
		);

		let result = store
			.acknowledge_agent_fork(a.clone(), "fork-native".into(), expected.clone())
			.await
			.unwrap()
			.unwrap();

		assert_eq!(result.state, "forked");
		assert_eq!(store.get_agent_work_item("main".into()).await.unwrap(), original);

		let branch = store.get_agent_work_item("branch".into()).await.unwrap();

		assert_eq!(branch.parent_goal_id.as_deref(), Some("main"));
		assert_eq!(branch.codex_thread_id.as_deref(), Some("fork-native"));
		assert_eq!(branch.dispatch_state, AgentDispatchState::Idle);
		assert!(store.agent_manager_ids().await.unwrap().contains(&"branch".into()));
		assert_eq!(
			store
				.acknowledge_agent_fork(a.clone(), "fork-native".into(), expected.clone())
				.await
				.unwrap()
				.unwrap(),
			result
		);

		assert_fork_draft(&store, boundary, &a, &result).await;
		assert_attachment_index(&store, boundary, &result).await;
	}
}

#[tokio::test]
async fn attached_branch_retains_its_exact_source_instead_of_becoming_a_sibling() {
	let directory = tempfile::tempdir().unwrap();
	let store = SqliteStore::open_test(&directory.path().join("attached.sqlite3")).unwrap();

	seed(&store).await;

	let mut source = store.get_agent_work_item("main".into()).await.unwrap();

	source.id = "source-agent".into();
	source.parent_goal_id = Some("main".into());
	source.codex_thread_id = None;

	store.create_agent_work_item(source).await.unwrap();
	store.bind_agent_thread("source-agent".into(), "source-agent-native".into()).await.unwrap();

	let mut a = attempt(AgentForkBoundary::AttachedAfterTurn);

	a.source.work = "source-agent".into();
	a.source.thread = "source-agent-native".into();

	store.reserve_agent_fork(a.clone()).await.unwrap().unwrap();

	assert_eq!(
		store.get_agent_work_item("branch".into()).await.unwrap().parent_goal_id.as_deref(),
		Some("source-agent")
	);
	assert!(store.agent_attached_forks("main".into()).await.unwrap().0.is_empty());

	let saved = store.agent_attached_forks("source-agent".into()).await.unwrap().0;

	assert_eq!(saved.len(), 1);
	assert_eq!(saved[0].state, "reserved");
	assert_eq!(saved[0].attempt, a);
}

#[tokio::test]
async fn definite_fork_refusal_resolves_only_the_new_unbound_work() {
	let directory = tempfile::tempdir().unwrap();
	let store = SqliteStore::open_test(&directory.path().join("fork.sqlite3")).unwrap();

	seed(&store).await;

	let original = store.get_agent_work_item("main".into()).await.unwrap();
	let a = attempt(AgentForkBoundary::BeforeInput);

	store.reserve_agent_fork(a.clone()).await.unwrap().unwrap();

	assert!(store.reject_agent_fork(a.clone()).await.unwrap());
	assert!(!store.reject_agent_fork(a.clone()).await.unwrap());
	assert_eq!(
		store
			.agent_fork_receipt("main".into(), a.source.review_token.clone())
			.await
			.unwrap()
			.unwrap()
			.state,
		"rejected"
	);
	assert_eq!(store.get_agent_work_item("main".into()).await.unwrap(), original);

	let branch = store.get_agent_work_item("branch".into()).await.unwrap();

	assert_eq!(branch.status, AgentWorkStatus::Resolved);
	assert!(branch.codex_thread_id.is_none());
	assert!(store.acknowledge_agent_fork(a, "late-native".into(), vec![]).await.unwrap().is_none());
}

async fn assert_attachment_index(
	store: &SqliteStore,
	boundary: AgentForkBoundary,
	result: &AgentForkReceipt,
) {
	let (attached, truncated) = store.agent_attached_forks("main".into()).await.unwrap();

	assert!(!truncated);
	assert_eq!(attached.len(), usize::from(boundary == AgentForkBoundary::AttachedAfterTurn));

	if let Some(attachment) = attached.first() {
		assert_eq!(attachment, result);
	}

	assert!(store.agent_attached_forks("branch".into()).await.unwrap().0.is_empty());
}
