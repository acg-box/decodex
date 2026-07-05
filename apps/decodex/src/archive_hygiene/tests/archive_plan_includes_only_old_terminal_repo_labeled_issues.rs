use crate::{
	archive_hygiene::{
		plan,
		tests::{self, FakeArchiveTracker},
	},
	tracker::{self},
};

#[test]
fn archive_plan_includes_only_old_terminal_repo_labeled_issues() {
	let config = tests::service_config();
	let workflow = tests::workflow();
	let active = tracker::automation_active_label(config.service_id());
	let queued = tracker::automation_queue_label(config.service_id());
	let tracker = FakeArchiveTracker::default().with_label(
		"repo:decodex",
		vec![
			tests::issue("issue-old", "XY-1", "Done", &["repo:decodex"], "2026-03-01T00:00:00Z"),
			tests::issue(
				"issue-active",
				"XY-2",
				"Done",
				&["repo:decodex", &active],
				"2026-03-01T00:00:00Z",
			),
			tests::issue(
				"issue-queued",
				"XY-3",
				"Canceled",
				&["repo:decodex", &queued],
				"2026-03-01T00:00:00Z",
			),
			tests::issue(
				"issue-needs",
				"XY-4",
				"Duplicate",
				&["repo:decodex", "decodex:needs-attention"],
				"2026-03-01T00:00:00Z",
			),
			tests::issue(
				"issue-manual",
				"XY-5",
				"Done",
				&["repo:decodex", "decodex:manual-only"],
				"2026-03-01T00:00:00Z",
			),
			tests::issue("issue-todo", "XY-6", "Todo", &["repo:decodex"], "2026-03-01T00:00:00Z"),
			tests::issue("issue-new", "XY-7", "Done", &["repo:decodex"], "2026-04-20T00:00:00Z"),
			tests::issue("issue-equal", "XY-8", "Done", &["repo:decodex"], "2026-04-01T00:00:00Z"),
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

	assert_eq!(
		plan.candidates.iter().map(|candidate| candidate.identifier.as_str()).collect::<Vec<_>>(),
		vec!["XY-1"]
	);
	assert_eq!(plan.skipped.len(), 7);
	assert!(
		plan.skipped
			.iter()
			.any(|skipped| skipped.reason.contains("protected label `decodex:active:decodex`"))
	);
	assert!(
		plan.skipped
			.iter()
			.any(|skipped| skipped.reason.contains("protected label `decodex:queued:decodex`"))
	);
	assert!(
		plan.skipped
			.iter()
			.any(|skipped| skipped.reason.contains("protected label `decodex:needs-attention`"))
	);
	assert!(
		plan.skipped
			.iter()
			.any(|skipped| skipped.reason.contains("protected label `decodex:manual-only`"))
	);
}
