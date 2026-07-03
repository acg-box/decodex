use crate::{
	orchestrator::tests::operator::{StateStore, TEST_SERVICE_ID, TrackerComment, TrackerIssue},
	tracker::records::{self, LinearExecutionEventIdentity},
};

pub(in crate::orchestrator::tests::operator) fn successful_linear_execution_history_comments(
	issue: &TrackerIssue,
) -> Vec<TrackerComment> {
	vec![
		linear_execution_history_comment(
			issue,
			"run_started",
			"2026-04-29T10:00:00Z",
			"run-start",
			|record| {
				record.branch = Some(String::from("y/decodex-xy-355"));
				record.worktree_path = Some(String::from(".worktrees/XY-355"));
				record.commit_sha = Some(String::from("0000000000000000000000000000000000000000"));
				record.transport = Some(String::from("stdio://"));
				record.summary = Some(String::from("Started the Decodex lane."));
			},
		),
		linear_execution_history_comment(
			issue,
			"review_handoff",
			"2026-04-29T10:05:00Z",
			"review",
			|record| {
				record.branch = Some(String::from("y/decodex-xy-355"));
				record.worktree_path = Some(String::from(".worktrees/XY-355"));
				record.pr_url = Some(String::from("https://github.com/hack-ink/decodex/pull/355"));
				record.pr_head_sha = Some(String::from("1111111111111111111111111111111111111111"));
				record.pr_base_ref = Some(String::from("main"));
				record.commit_sha = Some(String::from("1111111111111111111111111111111111111111"));
				record.validation_result = Some(String::from("passed"));
				record.summary = Some(String::from("Opened a reviewable PR."));
				record.terminal_path = Some(String::from("review_handoff"));
			},
		),
		linear_execution_history_comment(
			issue,
			"landed",
			"2026-04-29T10:08:00Z",
			"landed",
			|record| {
				record.branch = Some(String::from("y/decodex-xy-355"));
				record.pr_url = Some(String::from("https://github.com/hack-ink/decodex/pull/355"));
				record.pr_head_sha = Some(String::from("1111111111111111111111111111111111111111"));
				record.pr_base_ref = Some(String::from("main"));
				record.commit_sha = Some(String::from("2222222222222222222222222222222222222222"));
				record.summary = Some(String::from("Merged the PR."));
			},
		),
		linear_execution_history_comment(
			issue,
			"needs_attention",
			"2026-04-29T10:06:00Z",
			"earlier-attention",
			|record| {
				record.summary = Some(String::from("Earlier attempt required operator attention."));
				record.error_class = Some(String::from("validation_failed"));
				record.next_action = Some(String::from("Re-run the repaired lane."));
				record.blockers = Some(Vec::new());
				record.evidence = Some(vec![String::from("cargo make test failed")]);
				record.terminal_path = Some(String::from("manual_attention"));
			},
		),
		linear_execution_history_comment(
			issue,
			"closeout",
			"2026-04-29T10:10:00Z",
			"closeout",
			|record| {
				record.branch = Some(String::from("y/decodex-xy-355"));
				record.pr_url = Some(String::from("https://github.com/hack-ink/decodex/pull/355"));
				record.commit_sha = Some(String::from("2222222222222222222222222222222222222222"));
				record.summary = Some(String::from("Completed retained closeout."));
				record.target_state = Some(String::from("Done"));
			},
		),
	]
}

pub(in crate::orchestrator::tests::operator) fn successful_linear_execution_history_comments_with_cleanup(
	issue: &TrackerIssue,
) -> Vec<TrackerComment> {
	let mut comments = successful_linear_execution_history_comments(issue);

	comments.push(linear_execution_history_comment(
		issue,
		"cleanup_complete",
		"2026-04-29T10:11:00Z",
		"cleanup",
		|record| {
			record.branch = Some(String::from("y/decodex-xy-355"));
			record.worktree_path = Some(String::from(".worktrees/XY-355"));
			record.pr_url = Some(String::from("https://github.com/hack-ink/decodex/pull/355"));
			record.commit_sha = Some(String::from("2222222222222222222222222222222222222222"));
			record.cleanup_status = Some(String::from("completed"));
			record.summary = Some(String::from("Cleaned up the retained lane."));
		},
	));

	comments
}

pub(in crate::orchestrator::tests::operator) fn retained_partial_progress_linear_execution_history_comments(
	issue: &TrackerIssue,
) -> Vec<TrackerComment> {
	vec![
		linear_execution_history_comment(
			issue,
			"run_started",
			"2026-06-11T09:00:00Z",
			"run-start",
			|record| {
				record.branch = Some(String::from("xy/profit-pilot-xy-922"));
				record.worktree_path = Some(String::from(".worktrees/XY-922"));
				record.commit_sha = Some(String::from("0000000000000000000000000000000000000000"));
				record.transport = Some(String::from("stdio://"));
				record.summary = Some(String::from("Started the Decodex lane."));
			},
		),
		linear_execution_history_comment(
			issue,
			"needs_attention",
			"2026-06-11T09:08:00Z",
			"retained-partial-progress",
			|record| {
				record.branch = Some(String::from("xy/profit-pilot-xy-922"));
				record.worktree_path = Some(String::from(".worktrees/XY-922"));
				record.summary = Some(String::from(
					"Decodex retained validation-ready partial progress for manual review.",
				));
				record.error_class = Some(String::from("partial_progress_retained"));
				record.next_action = Some(String::from(
					"review the retained worktree diff, then commit/push/PR or mark manual disposition",
				));
				record.blockers = Some(vec![String::from(
					"lane stopped before review handoff and terminal finalize",
				)]);
				record.evidence = Some(vec![
					String::from("cargo make check passed"),
					String::from("retained worktree has tracked changes"),
				]);
				record.terminal_path = Some(String::from("retained_partial_progress"));
			},
		),
	]
}

pub(in crate::orchestrator::tests::operator) fn seed_local_linear_execution_events(
	state_store: &StateStore,
	comments: &[TrackerComment],
) {
	for comment in comments {
		let record = records::parse_linear_execution_event_record(&comment.body)
			.expect("test comment should contain a valid Linear execution event");

		state_store
			.record_linear_execution_event(&record)
			.expect("local execution event should persist");
	}
}

pub(in crate::orchestrator::tests::operator) fn linear_execution_history_comment<F>(
	issue: &TrackerIssue,
	event_type: &str,
	event_timestamp: &str,
	stable_anchor: &str,
	configure: F,
) -> TrackerComment
where
	F: FnOnce(&mut records::LinearExecutionEventRecord),
{
	let mut record = records::LinearExecutionEventRecord::new(
		LinearExecutionEventIdentity {
			service_id: TEST_SERVICE_ID,
			issue_id: &issue.id,
			issue_identifier: &issue.identifier,
			run_id: "xy-355-attempt-1-1777527013",
			attempt_number: 1,
		},
		event_type,
		event_timestamp.to_owned(),
		stable_anchor,
	);

	configure(&mut record);

	records::validate_linear_execution_event_record(&record)
		.expect("test ledger record should be valid");

	TrackerComment {
		body: records::append_structured_comment_record(
			&records::render_linear_execution_event_comment_body(&record, None),
			&record,
		)
		.expect("structured comment should serialize"),
		created_at: event_timestamp.to_owned(),
	}
}
