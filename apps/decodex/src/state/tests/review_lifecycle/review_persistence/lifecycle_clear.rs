use tempfile::TempDir;

use crate::state::{
	ReviewLifecycleHandoffFixture, ReviewLifecycleTransitionFixture, StateStore, tests,
};

#[test]
fn clear_review_lifecycle_for_handoff_preserves_other_branches() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let store = StateStore::open(&state_path).expect("state store should open");
	let removed_handoff = tests::sample_pub_101_review_handoff();
	let removed_orchestration = tests::sample_pub_101_review_orchestration();
	let kept_handoff = ReviewLifecycleHandoffFixture::new(
		"run-2",
		1,
		"x/decodex-pub-101-review",
		"https://github.com/hack-ink/decodex/pull/102",
		"main",
		"x/decodex-pub-101-review",
		"18a20f7dfb9526e7421a5f095b1c6adec84e52d6",
	);
	let kept_orchestration = ReviewLifecycleTransitionFixture::new(
		"run-2",
		1,
		"x/decodex-pub-101-review",
		"https://github.com/hack-ink/decodex/pull/102",
		"18a20f7dfb9526e7421a5f095b1c6adec84e52d6",
		"request_pending",
		None,
		None,
		None,
		0,
		0,
		None,
	);

	store
		.upsert_review_lifecycle_handoff_fixture("pubfi", "PUB-101", &removed_handoff)
		.expect("removed handoff projection should persist");
	store
		.upsert_review_lifecycle_transition_fixture("pubfi", "PUB-101", &removed_orchestration)
		.expect("removed orchestration projection should persist");

	tests::upsert_handoff_review_policy_checkpoint(
		&store,
		"PUB-101",
		"run-1",
		"findings",
		"08a20f7dfb9526e7421a5f095b1c6adec84e52d6",
		2,
	);

	store
		.upsert_review_lifecycle_handoff_fixture("pubfi", "PUB-101", &kept_handoff)
		.expect("kept handoff projection should persist");
	store
		.upsert_review_lifecycle_transition_fixture("pubfi", "PUB-101", &kept_orchestration)
		.expect("kept orchestration projection should persist");

	tests::upsert_handoff_review_policy_checkpoint(
		&store,
		"PUB-101",
		"run-2",
		"clean",
		"18a20f7dfb9526e7421a5f095b1c6adec84e52d6",
		0,
	);

	store
		.clear_review_lifecycle_for_handoff(
			"pubfi",
			"PUB-101",
			&removed_handoff,
			&removed_orchestration,
		)
		.expect("exact review lifecycle should clear");

	let reopened = StateStore::open(&state_path).expect("reopened state store should open");

	assert!(
		reopened
			.review_lifecycle_handoff_fixture("pubfi", "PUB-101", "x/decodex-pub-101")
			.expect("removed handoff projection should read")
			.is_none()
	);
	assert_eq!(
		reopened
			.review_lifecycle_handoff_fixture("pubfi", "PUB-101", "x/decodex-pub-101-review")
			.expect("kept handoff projection should read"),
		Some(kept_handoff.clone())
	);
	assert_eq!(
		reopened
			.review_lifecycle_transition_fixture("pubfi", "PUB-101", &kept_handoff)
			.expect("kept orchestration projection should read"),
		Some(kept_orchestration)
	);
	assert!(
		reopened
			.review_policy_checkpoint("pubfi", "PUB-101", "run-1", 1, "handoff")
			.expect("removed review policy checkpoint should read")
			.is_none()
	);

	let kept_checkpoint = reopened
		.review_policy_checkpoint("pubfi", "PUB-101", "run-2", 1, "handoff")
		.expect("kept review policy checkpoint should read")
		.expect("kept review policy checkpoint should exist");

	assert_eq!(kept_checkpoint.status(), "clean");
	assert_eq!(kept_checkpoint.head_sha(), "18a20f7dfb9526e7421a5f095b1c6adec84e52d6");
}

#[test]
fn missing_review_lifecycle_projections_return_absent() {
	let store = StateStore::open_in_memory().expect("state store should open");
	let handoff = ReviewLifecycleHandoffFixture::new(
		"run-1",
		2,
		"x/decodex-pub-101",
		"https://github.com/hack-ink/decodex/pull/101",
		"main",
		"x/decodex-pub-101",
		"08a20f7dfb9526e7421a5f095b1c6adec84e52d6",
	);

	assert!(
		store
			.review_lifecycle_handoff_fixture("pubfi", "PUB-101", "x/decodex-pub-101")
			.expect("review handoff projection should read")
			.is_none()
	);
	assert!(
		store
			.review_lifecycle_transition_fixture("pubfi", "PUB-101", &handoff)
			.expect("review orchestration projection should read")
			.is_none()
	);
}
