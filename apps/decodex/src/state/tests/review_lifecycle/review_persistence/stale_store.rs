use tempfile::TempDir;

use crate::state::{ReviewHandoffMarker, ReviewOrchestrationMarker, StateStore};

#[test]
fn persistent_review_lifecycle_survives_stale_store_persist_and_is_visible() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let observer = StateStore::open(&state_path).expect("observer state store should open");
	let writer = StateStore::open(&state_path).expect("writer state store should open");
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

	writer
		.upsert_review_handoff_marker("pubfi", "PUB-101", &handoff)
		.expect("handoff projection should persist");
	writer
		.upsert_review_orchestration_marker("pubfi", "PUB-101", &orchestration)
		.expect("orchestration projection should persist");

	let observed_handoff = observer
		.review_handoff_marker("pubfi", "PUB-101", "x/decodex-pub-101")
		.expect("observer should read handoff projection")
		.expect("observer should see lifecycle written by another store");

	assert_eq!(observed_handoff, handoff);

	observer
		.record_run_attempt("run-2", "PUB-202", 1, "running")
		.expect("stale observer should persist unrelated runtime state");

	let reopened = StateStore::open(&state_path).expect("reopened state store should open");

	assert_eq!(
		reopened
			.review_handoff_marker("pubfi", "PUB-101", "x/decodex-pub-101")
			.expect("reopened store should read handoff projection"),
		Some(handoff.clone())
	);
	assert_eq!(
		reopened
			.review_orchestration_marker("pubfi", "PUB-101", &handoff)
			.expect("reopened store should read orchestration projection"),
		Some(orchestration)
	);
	assert!(
		reopened.run_attempt("run-2").expect("run attempt should read").is_some(),
		"unrelated stale-store persist should still keep its own update"
	);
}
