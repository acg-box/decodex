use crate::{
	orchestrator::{
		self, PassiveRetainedAttentionRuntime, RetainedReviewRunIdentity,
		tests::{
			FakeTracker, TEST_SERVICE_ID, {self},
		},
	},
	state::StateStore,
	tracker::{self},
	worktree::WorktreeManager,
};

#[test]
fn rebound_lifecycle_authority_suppresses_stale_missing_handoff_attention_writeback() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let issue = tests::sample_issue("In Review", &[active_label.as_str()]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("retained worktree should exist");
	let head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let run_identity = RetainedReviewRunIdentity {
		run_id: String::from("pub-101-attempt-8-123"),
		attempt_number: 8,
	};

	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree mapping should record");
	state_store
		.upsert_review_lifecycle_handoff_fixture(
			config.service_id(),
			&issue.id,
			&tests::sample_review_lifecycle_handoff_fixture(
				&worktree.branch_name,
				"https://github.com/hack-ink/decodex/pull/101",
				&head_oid,
			),
		)
		.expect("rebound lifecycle authority should record");

	let worktree_mapping = state_store
		.worktree_for_issue(&issue.id)
		.expect("worktree mapping query should succeed")
		.expect("worktree mapping should exist");
	let runtime = PassiveRetainedAttentionRuntime {
		tracker: &tracker,
		project: &config,
		workflow: &workflow,
		state_store: &state_store,
	};

	orchestrator::apply_passive_retained_manual_attention_with_run_identity(
		runtime,
		&issue,
		&worktree_mapping,
		&run_identity,
		"missing_review_handoff_record",
	)
	.expect("stale passive retained attention should no-op after rebind");

	assert!(tracker.state_updates.borrow().is_empty());
	assert!(tracker.label_additions.borrow().is_empty());
	assert!(tracker.label_removals.borrow().is_empty());
	assert!(tracker.comments.borrow().is_empty());
	assert!(
		state_store
			.list_linear_execution_events(config.service_id(), &issue.id)
			.expect("linear execution events should list")
			.is_empty()
	);
}
