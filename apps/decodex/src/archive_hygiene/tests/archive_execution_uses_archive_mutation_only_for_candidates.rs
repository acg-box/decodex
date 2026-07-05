use crate::archive_hygiene::{
	plan,
	tests::{self, FakeArchiveTracker},
};

#[test]
fn archive_execution_uses_archive_mutation_only_for_candidates() {
	let config = tests::service_config();
	let workflow = tests::workflow();
	let tracker = FakeArchiveTracker::default().with_label(
		"repo:decodex",
		vec![
			tests::issue("issue-old", "XY-1", "Done", &["repo:decodex"], "2026-03-01T00:00:00Z"),
			tests::issue("issue-new", "XY-2", "Done", &["repo:decodex"], "2026-04-20T00:00:00Z"),
		],
	);
	let plan = plan::build_archive_plan(
		&tracker,
		&config,
		&workflow,
		&[String::from("repo:decodex")],
		"2026-04-01T00:00:00Z",
	)
	.expect("archive plan should build");

	for candidate in &plan.candidates {
		tracker.archive_issue(&candidate.id);
	}

	assert_eq!(tracker.archived_issue_ids.borrow().as_slice(), ["issue-old"]);
}
