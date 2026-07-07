use std::fs;

use tempfile::TempDir;

use crate::orchestrator::{
	self, PostReviewLaneClassification, PostReviewLaneSnapshot, PullRequestReviewStateInspector,
	StateStore,
	tests::{self, TEST_SERVICE_ID},
};

pub(crate) const BRANCH_NAME: &str = "x/pubfi-pub-101";
pub(crate) const HEAD_OID: &str = "08a20f7dfb9526e7421a5f095b1c6adec84e52d6";
pub(crate) const PR_URL: &str = "https://github.com/hack-ink/decodex/pull/174";
pub(crate) const SERVICE_ID: &str = TEST_SERVICE_ID;

pub(crate) fn snapshot_for_issue_state(
	issue_state: &str,
	local_branch_name: &str,
) -> (TempDir, StateStore, PostReviewLaneSnapshot) {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue(issue_state, &[]);
	let worktree_path = temp_dir.path().join("lane");

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");

	state_store
		.upsert_worktree("pubfi", &issue.id, BRANCH_NAME, &worktree_path.display().to_string())
		.expect("worktree should record");

	let worktree = state_store
		.list_worktrees("pubfi")
		.expect("worktree list should succeed")
		.into_iter()
		.next()
		.expect("worktree should exist");
	let snapshot = PostReviewLaneSnapshot {
		issue,
		worktree,
		lifecycle_record: Some(tests::sample_review_lifecycle_record(
			BRANCH_NAME,
			PR_URL,
			HEAD_OID,
		)),
		local_branch_name: Some(String::from(local_branch_name)),
		local_head_oid: Some(String::from(HEAD_OID)),
	};

	(temp_dir, state_store, snapshot)
}

pub(crate) fn classify<I>(
	snapshot: &PostReviewLaneSnapshot,
	state_store: &StateStore,
	inspector: &I,
) -> PostReviewLaneClassification
where
	I: PullRequestReviewStateInspector,
{
	orchestrator::classify_post_review_lane(
		snapshot,
		state_store,
		&tests::sample_workflow(),
		inspector,
	)
	.expect("classification should succeed")
}

pub(crate) fn seed_review_marker(
	state_store: &StateStore,
	snapshot: &PostReviewLaneSnapshot,
	stage: &str,
	external_round_count: i64,
) {
	tests::seed_review_orchestration_marker(
		state_store,
		SERVICE_ID,
		&snapshot.issue.id,
		&tests::sample_review_orchestration_marker(
			BRANCH_NAME,
			PR_URL,
			HEAD_OID,
			stage,
			external_round_count,
		),
	);
}
