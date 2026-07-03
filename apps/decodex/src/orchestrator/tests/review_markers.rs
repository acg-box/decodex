use crate::{
	orchestrator::tests::{
		Path, StateStore, TEST_EXTERNAL_REVIEW_AUTO_MERGE_ENABLED_AT,
		TEST_EXTERNAL_REVIEW_REQUEST_COMMENT_ID, TEST_EXTERNAL_REVIEW_REQUEST_CREATED_AT,
		WorktreeMapping,
	},
	state,
};

pub(super) fn sample_review_handoff_marker(
	branch_name: &str,
	pr_url: &str,
	head_oid: &str,
) -> state::ReviewHandoffMarker {
	state::ReviewHandoffMarker::new("run-1", 1, branch_name, pr_url, "main", branch_name, head_oid)
}

pub(super) fn seed_review_handoff_marker(
	state_store: &StateStore,
	project_id: &str,
	issue_id: &str,
	branch_name: &str,
	pr_url: &str,
	head_oid: &str,
) {
	state_store
		.upsert_review_handoff_marker(
			project_id,
			issue_id,
			&sample_review_handoff_marker(branch_name, pr_url, head_oid),
		)
		.expect("review handoff marker should persist");
}

pub(super) fn seed_review_handoff_marker_value(
	state_store: &StateStore,
	project_id: &str,
	issue_id: &str,
	marker: &state::ReviewHandoffMarker,
) {
	state_store
		.upsert_review_handoff_marker(project_id, issue_id, marker)
		.expect("review handoff marker should persist");
}

pub(super) fn seed_review_handoff_marker_for_path(
	state_store: &StateStore,
	project_id: &str,
	worktree_path: &Path,
	marker: &state::ReviewHandoffMarker,
) {
	let worktree = worktree_mapping_for_path(state_store, project_id, worktree_path);

	seed_review_handoff_marker_value(state_store, project_id, worktree.issue_id(), marker);
}

pub(super) fn seed_review_orchestration_marker(
	state_store: &StateStore,
	project_id: &str,
	issue_id: &str,
	marker: &state::ReviewOrchestrationMarker,
) {
	state_store
		.upsert_review_handoff_marker(
			project_id,
			issue_id,
			&state::ReviewHandoffMarker::new(
				marker.run_id().to_owned(),
				marker.attempt_number(),
				marker.branch_name().to_owned(),
				marker.pr_url().to_owned(),
				"main",
				marker.branch_name().to_owned(),
				marker.head_sha().to_owned(),
			),
		)
		.expect("review handoff marker should persist");
	state_store
		.upsert_review_orchestration_marker(project_id, issue_id, marker)
		.expect("review orchestration marker should persist");
}

pub(super) fn seed_review_orchestration_marker_for_path(
	state_store: &StateStore,
	project_id: &str,
	worktree_path: &Path,
	marker: &state::ReviewOrchestrationMarker,
) {
	let worktree = worktree_mapping_for_path(state_store, project_id, worktree_path);

	seed_review_orchestration_marker(state_store, project_id, worktree.issue_id(), marker);
}

pub(super) fn persisted_review_handoff_marker(
	state_store: &StateStore,
	project_id: &str,
	issue_id: &str,
	branch_name: &str,
) -> state::ReviewHandoffMarker {
	state_store
		.review_handoff_marker(project_id, issue_id, branch_name)
		.expect("review handoff marker should read")
		.expect("review handoff marker should exist")
}

pub(super) fn persisted_review_orchestration_marker(
	state_store: &StateStore,
	project_id: &str,
	issue_id: &str,
	branch_name: &str,
) -> state::ReviewOrchestrationMarker {
	let handoff = persisted_review_handoff_marker(state_store, project_id, issue_id, branch_name);

	state_store
		.review_orchestration_marker(project_id, issue_id, &handoff)
		.expect("review orchestration marker should read")
		.expect("review orchestration marker should exist")
}

pub(super) fn persisted_review_orchestration_marker_for_path(
	state_store: &StateStore,
	project_id: &str,
	worktree_path: &Path,
) -> state::ReviewOrchestrationMarker {
	let worktree = worktree_mapping_for_path(state_store, project_id, worktree_path);

	persisted_review_orchestration_marker(
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

pub(super) fn sample_review_orchestration_marker(
	branch_name: &str,
	pr_url: &str,
	head_oid: &str,
	phase: &str,
	external_round_count: i64,
) -> state::ReviewOrchestrationMarker {
	state::ReviewOrchestrationMarker::new(
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
