use crate::{
	orchestrator::tests::{self, TEST_SERVICE_ID},
	tracker::{self, TrackerIssue, records},
};

pub(super) fn reconciliation_sample_service_owned_issue(state_name: &str) -> TrackerIssue {
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);

	tests::sample_issue(state_name, &[active_label.as_str()])
}

pub(super) fn assert_dirty_stalled_retained_progress_comments(comments: &[String]) {
	assert!(comments.iter().any(|comment| {
		comment.contains("decodex retained partial progress and needs attention")
			&& comment.contains("partial_progress_retained")
			&& comment.contains("finish validation and PR handoff or reset the patch manually")
			&& comment.contains(".worktrees/PUB-102")
	}));
	assert!(
		comments.iter().all(|comment| !comment.contains("- error_class: `stalled_run_detected`"))
	);
	assert!(
		comments.iter().all(|comment| !comment.contains("decodex run failed and needs attention"))
	);

	let ledger_event = comments
		.iter()
		.find_map(|comment| records::parse_linear_execution_event_record(comment))
		.expect("retained partial progress should write a Linear execution event");

	assert_eq!(ledger_event.event_type, "needs_attention");
	assert_eq!(ledger_event.error_class.as_deref(), Some("partial_progress_retained"));
	assert_eq!(ledger_event.terminal_path.as_deref(), Some("retained_partial_progress"));
	assert_eq!(
		ledger_event.summary.as_deref(),
		Some("Decodex retained partial progress and needs attention.")
	);
	assert_eq!(
		ledger_event.blockers.as_deref(),
		Some(
			[String::from("Retained tracked worktree changes require operator recovery.")]
				.as_slice()
		)
	);
	assert!(
		ledger_event.evidence.as_deref().is_some_and(|evidence| evidence
			.iter()
			.any(|item| item.contains("tracked worktree changes retained"))),
		"retained partial progress evidence should mention retained tracked changes"
	);
	assert!(
		ledger_event.evidence.as_deref().is_some_and(|evidence| evidence
			.iter()
			.any(|item| item.contains("Source failure class `stalled_run_detected`"))),
		"retained partial progress evidence should preserve the stalled source class"
	);
}
