use crate::{
	orchestrator::tests::{
		Path, StateStore, TEST_EXTERNAL_REVIEW_AUTO_MERGE_ENABLED_AT,
		TEST_EXTERNAL_REVIEW_REQUEST_COMMENT_ID, TEST_EXTERNAL_REVIEW_REQUEST_CREATED_AT,
		WorktreeMapping,
	},
	state,
};

pub(super) fn sample_review_lifecycle_handoff_fixture(
	branch_name: &str,
	pr_url: &str,
	head_oid: &str,
) -> state::ReviewLifecycleHandoffFixture {
	state::ReviewLifecycleHandoffFixture::new(
		"run-1",
		1,
		branch_name,
		pr_url,
		"main",
		branch_name,
		head_oid,
	)
}

pub(super) fn sample_review_lifecycle_record(
	branch_name: &str,
	pr_url: &str,
	head_oid: &str,
) -> state::ReviewLifecycleRecord {
	state::ReviewLifecycleRecord::from_test_lifecycle_fixtures(
		&sample_review_lifecycle_handoff_fixture(branch_name, pr_url, head_oid),
		None,
	)
}

pub(super) fn seed_review_lifecycle_handoff_fixture(
	state_store: &StateStore,
	project_id: &str,
	issue_id: &str,
	branch_name: &str,
	pr_url: &str,
	head_oid: &str,
) {
	state_store
		.upsert_review_lifecycle_handoff_fixture(
			project_id,
			issue_id,
			&sample_review_lifecycle_handoff_fixture(branch_name, pr_url, head_oid),
		)
		.expect("review lifecycle handoff fixture should persist");
}

pub(super) fn seed_review_lifecycle_handoff_fixture_value(
	state_store: &StateStore,
	project_id: &str,
	issue_id: &str,
	marker: &state::ReviewLifecycleHandoffFixture,
) {
	state_store
		.upsert_review_lifecycle_handoff_fixture(project_id, issue_id, marker)
		.expect("review lifecycle handoff fixture should persist");
}

pub(super) fn seed_review_lifecycle_handoff_fixture_for_path(
	state_store: &StateStore,
	project_id: &str,
	worktree_path: &Path,
	marker: &state::ReviewLifecycleHandoffFixture,
) {
	let worktree = worktree_mapping_for_path(state_store, project_id, worktree_path);

	seed_review_lifecycle_handoff_fixture_value(
		state_store,
		project_id,
		worktree.issue_id(),
		marker,
	);
}

pub(super) fn seed_review_lifecycle_transition_fixture(
	state_store: &StateStore,
	project_id: &str,
	issue_id: &str,
	marker: &state::ReviewLifecycleTransitionFixture,
) {
	state_store
		.upsert_review_lifecycle_handoff_fixture(
			project_id,
			issue_id,
			&state::ReviewLifecycleHandoffFixture::new(
				marker.run_id().to_owned(),
				marker.attempt_number(),
				marker.branch_name().to_owned(),
				marker.pr_url().to_owned(),
				"main",
				marker.branch_name().to_owned(),
				marker.head_sha().to_owned(),
			),
		)
		.expect("review lifecycle handoff fixture should persist");
	state_store
		.upsert_review_lifecycle_transition_fixture(project_id, issue_id, marker)
		.expect("review lifecycle transition fixture should persist");
}

pub(super) fn seed_review_lifecycle_transition_fixture_for_path(
	state_store: &StateStore,
	project_id: &str,
	worktree_path: &Path,
	marker: &state::ReviewLifecycleTransitionFixture,
) {
	let worktree = worktree_mapping_for_path(state_store, project_id, worktree_path);

	seed_review_lifecycle_transition_fixture(state_store, project_id, worktree.issue_id(), marker);
}

pub(super) fn persisted_review_lifecycle_handoff_fixture(
	state_store: &StateStore,
	project_id: &str,
	issue_id: &str,
	branch_name: &str,
) -> state::ReviewLifecycleHandoffFixture {
	state_store
		.review_lifecycle_handoff_fixture(project_id, issue_id, branch_name)
		.expect("review lifecycle handoff fixture should read")
		.expect("review lifecycle handoff fixture should exist")
}

pub(super) fn persisted_review_lifecycle_transition_fixture(
	state_store: &StateStore,
	project_id: &str,
	issue_id: &str,
	branch_name: &str,
) -> state::ReviewLifecycleTransitionFixture {
	let handoff =
		persisted_review_lifecycle_handoff_fixture(state_store, project_id, issue_id, branch_name);

	state_store
		.review_lifecycle_transition_fixture(project_id, issue_id, &handoff)
		.expect("review lifecycle transition fixture should read")
		.expect("review lifecycle transition fixture should exist")
}

pub(super) fn persisted_review_lifecycle_transition_fixture_for_path(
	state_store: &StateStore,
	project_id: &str,
	worktree_path: &Path,
) -> state::ReviewLifecycleTransitionFixture {
	let worktree = worktree_mapping_for_path(state_store, project_id, worktree_path);

	persisted_review_lifecycle_transition_fixture(
		state_store,
		project_id,
		worktree.issue_id(),
		worktree.branch_name(),
	)
}

pub(super) fn worktree_mapping_for_path(
	state_store: &StateStore,
	project_id: &str,
	worktree_path: &Path,
) -> WorktreeMapping {
	state_store
		.list_worktrees(project_id)
		.expect("worktree list should read")
		.into_iter()
		.find(|worktree| worktree.worktree_path() == worktree_path)
		.expect("worktree mapping should exist for path")
}

pub(super) fn sample_review_lifecycle_transition_fixture(
	branch_name: &str,
	pr_url: &str,
	head_oid: &str,
	phase: &str,
	external_round_count: i64,
) -> state::ReviewLifecycleTransitionFixture {
	state::ReviewLifecycleTransitionFixture::new(
		"run-1",
		1,
		branch_name,
		pr_url,
		head_oid,
		phase,
		Some(TEST_EXTERNAL_REVIEW_REQUEST_COMMENT_ID),
		Some(TEST_EXTERNAL_REVIEW_REQUEST_CREATED_AT),
		Some(0),
		0,
		external_round_count,
		if phase == "waiting_for_merge" {
			Some(TEST_EXTERNAL_REVIEW_AUTO_MERGE_ENABLED_AT)
		} else {
			None
		},
	)
}
