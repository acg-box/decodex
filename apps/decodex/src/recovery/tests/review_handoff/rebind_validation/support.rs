use crate::tracker::{
	TrackerIssue,
	records::{LinearExecutionEventIdentity, LinearExecutionEventRecord},
};

pub(crate) fn terminal_writeback_failure_event(
	service_id: &str,
	issue: &TrackerIssue,
	run_id: &str,
	attempt_number: i64,
	branch_name: &str,
	pr_url: &str,
) -> LinearExecutionEventRecord {
	let mut event = LinearExecutionEventRecord::new(
		LinearExecutionEventIdentity {
			service_id,
			issue_id: &issue.id,
			issue_identifier: &issue.identifier,
			run_id,
			attempt_number,
		},
		"terminal_failure",
		String::from("2026-07-04T00:00:00Z"),
		"review-handoff-writeback-failed",
	);

	event.branch = Some(branch_name.to_owned());
	event.worktree_path = Some(format!(".worktrees/{}", issue.identifier));
	event.pr_url = Some(pr_url.to_owned());
	event.error_class = Some(String::from("review_handoff_writeback_failed"));
	event.next_action = Some(String::from("recover review handoff"));
	event.blockers = Some(vec![String::from("review handoff writeback failed")]);
	event.evidence = Some(vec![String::from("retained PR lane evidence present")]);

	event
}
