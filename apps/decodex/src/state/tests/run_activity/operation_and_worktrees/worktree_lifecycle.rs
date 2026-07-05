use tempfile::TempDir;

use crate::state::{
	ReviewHandoffMarker, ReviewOrchestrationMarker, ReviewPolicyCheckpointInput, StateStore, tests,
};

#[test]
fn persistent_clear_worktree_deletes_review_lifecycle() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let store = StateStore::open(&state_path).expect("state store should open");
	let handoff = ReviewHandoffMarker::new(
		"run-1",
		1,
		"x/decodex-pub-101",
		"https://github.com/hack-ink/decodex/pull/101",
		"main",
		"x/decodex-pub-101",
		"08a20f7dfb9526e7421a5f095b1c6adec84e52d6",
	);
	let orchestration = ReviewOrchestrationMarker::new(
		"run-1",
		1,
		"x/decodex-pub-101",
		"https://github.com/hack-ink/decodex/pull/101",
		"08a20f7dfb9526e7421a5f095b1c6adec84e52d6",
		"request_pending",
		None,
		None,
		None,
		0,
		0,
		None,
	);

	store
		.upsert_worktree("pubfi", "PUB-101", "x/decodex-pub-101", "/tmp/worktrees/pub-101")
		.expect("worktree mapping should be recorded");
	store
		.upsert_review_handoff_marker("pubfi", "PUB-101", &handoff)
		.expect("handoff projection should persist");
	store
		.upsert_review_orchestration_marker("pubfi", "PUB-101", &orchestration)
		.expect("orchestration projection should persist");
	store.clear_worktree("PUB-101").expect("worktree cleanup should persist");

	let reopened = StateStore::open(&state_path).expect("reopened store should open");

	assert!(
		reopened.worktree_for_issue("PUB-101").expect("worktree lookup should succeed").is_none()
	);
	assert!(
		reopened
			.review_handoff_marker("pubfi", "PUB-101", "x/decodex-pub-101")
			.expect("handoff lookup should succeed")
			.is_none()
	);
	assert!(
		reopened
			.review_orchestration_marker("pubfi", "PUB-101", &handoff)
			.expect("orchestration lookup should succeed")
			.is_none()
	);
}

#[test]
fn persistent_clear_worktree_mapping_preserves_review_lifecycle() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let store = StateStore::open(&state_path).expect("state store should open");
	let handoff = tests::sample_pub_101_review_handoff();

	store
		.upsert_worktree("pubfi", "PUB-101", "x/decodex-pub-101", "/tmp/worktrees/pub-101")
		.expect("worktree mapping should be recorded");
	store
		.upsert_review_handoff_marker("pubfi", "PUB-101", &handoff)
		.expect("handoff projection should persist");
	store
		.upsert_review_policy_checkpoint(ReviewPolicyCheckpointInput {
			project_id: "pubfi",
			issue_id: "PUB-101",
			run_id: "run-1",
			attempt_number: 1,
			phase: "handoff",
			review_level: "normal",
			status: "clean",
			head_sha: "08a20f7dfb9526e7421a5f095b1c6adec84e52d6",
			nonclean_rounds: 0,
			details_json: "{}",
		})
		.expect("review checkpoint should persist");
	store.clear_worktree_mapping("PUB-101").expect("worktree mapping cleanup should persist");

	let reopened = StateStore::open(&state_path).expect("reopened store should open");

	assert!(
		reopened.worktree_for_issue("PUB-101").expect("worktree lookup should succeed").is_none()
	);
	assert!(
		reopened
			.review_handoff_marker("pubfi", "PUB-101", "x/decodex-pub-101")
			.expect("handoff lookup should succeed")
			.is_some()
	);
	assert!(
		reopened
			.review_policy_checkpoint("pubfi", "PUB-101", "run-1", 1, "handoff")
			.expect("review checkpoint lookup should succeed")
			.is_some()
	);
}
