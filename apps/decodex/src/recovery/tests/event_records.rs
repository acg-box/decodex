use crate::{
	recovery::{
		LEGACY_MANUAL_CLOSEOUT_EVENT, REVIEW_HANDOFF_ADOPT_EVENT, REVIEW_HANDOFF_REBIND_EVENT,
	},
	tracker::records::{self, LinearExecutionEventIdentity, LinearExecutionEventRecord},
};

#[test]
fn review_handoff_rebind_event_validation_accepts_required_fields() {
	let mut record = LinearExecutionEventRecord::new(
		LinearExecutionEventIdentity {
			service_id: "pubfi",
			issue_id: "issue-id",
			issue_identifier: "PUB-718",
			run_id: "pub-718-attempt-1",
			attempt_number: 1,
		},
		REVIEW_HANDOFF_REBIND_EVENT,
		super::current_timestamp(),
		"anchor",
	);

	record.branch = Some(String::from("x/pubfi-pub-718"));
	record.worktree_path = Some(String::from(".worktrees/PUB-718"));
	record.pr_url = Some(String::from("https://github.com/hack-ink/pubfi-mono-v2/pull/14"));
	record.pr_head_sha = Some(String::from("0123456789abcdef0123456789abcdef01234567"));
	record.pr_base_ref = Some(String::from("main"));
	record.commit_sha = Some(String::from("0123456789abcdef0123456789abcdef01234567"));
	record.validation_result = Some(String::from("passed"));
	record.summary = Some(String::from("Explicit operator rebind restored lifecycle record."));
	record.evidence = Some(vec![String::from("existing_review_lifecycle_record=absent")]);

	records::validate_linear_execution_event_record(&record).expect("rebind event should validate");
}

#[test]
fn review_handoff_adopt_event_validation_accepts_required_fields() {
	let mut record = LinearExecutionEventRecord::new(
		LinearExecutionEventIdentity {
			service_id: "decodex",
			issue_id: "issue-id",
			issue_identifier: "XY-944",
			run_id: "xy-944-manual-adopt-1",
			attempt_number: 1,
		},
		REVIEW_HANDOFF_ADOPT_EVENT,
		super::current_timestamp(),
		"anchor",
	);

	record.branch = Some(String::from("xy/xy-944-manual-takeover-adopt"));
	record.worktree_path = Some(String::from(".worktrees/XY-944"));
	record.pr_url = Some(String::from("https://github.com/hack-ink/decodex/pull/344"));
	record.pr_head_sha = Some(String::from("0123456789abcdef0123456789abcdef01234567"));
	record.pr_base_ref = Some(String::from("main"));
	record.commit_sha = Some(String::from("0123456789abcdef0123456789abcdef01234567"));
	record.validation_result = Some(String::from("passed"));
	record.summary =
		Some(String::from("Explicit operator manual takeover adopted review handoff."));
	record.evidence = Some(vec![String::from("manual_takeover_adopt=true")]);

	records::validate_linear_execution_event_record(&record).expect("adopt event should validate");
}

#[test]
fn merged_closeout_recovery_events_validate() {
	let mut closeout = LinearExecutionEventRecord::new(
		LinearExecutionEventIdentity {
			service_id: "pubfi-mono",
			issue_id: "issue-id",
			issue_identifier: "PUB-1549",
			run_id: "pub-1549-attempt-1-1781240781",
			attempt_number: 1,
		},
		LEGACY_MANUAL_CLOSEOUT_EVENT,
		super::current_timestamp(),
		"anchor-closeout",
	);

	closeout.branch = Some(String::from("x/pubfi-mono-pub-1549"));
	closeout.worktree_path = Some(String::from(".worktrees/PUB-1549"));
	closeout.pr_url = Some(String::from("https://github.com/helixbox/pubfi-mono/pull/309"));
	closeout.pr_head_sha = Some(String::from("0123456789abcdef0123456789abcdef01234567"));
	closeout.pr_base_ref = Some(String::from("main"));
	closeout.commit_sha = Some(String::from("1123456789abcdef0123456789abcdef01234567"));
	closeout.validation_result = Some(String::from("passed"));
	closeout.target_state = Some(String::from("Done"));
	closeout.summary = Some(String::from("Merged closeout recovery recorded."));

	records::validate_linear_execution_event_record(&closeout)
		.expect("merged closeout event should validate");

	let mut cleanup = LinearExecutionEventRecord::new(
		LinearExecutionEventIdentity {
			service_id: "pubfi-mono",
			issue_id: "issue-id",
			issue_identifier: "PUB-1549",
			run_id: "pub-1549-attempt-1-1781240781",
			attempt_number: 1,
		},
		"cleanup_complete",
		super::timestamp_after_seconds(1),
		"anchor-cleanup",
	);

	cleanup.branch = Some(String::from("x/pubfi-mono-pub-1549"));
	cleanup.worktree_path = Some(String::from(".worktrees/PUB-1549"));
	cleanup.pr_url = Some(String::from("https://github.com/helixbox/pubfi-mono/pull/309"));
	cleanup.pr_head_sha = Some(String::from("0123456789abcdef0123456789abcdef01234567"));
	cleanup.pr_base_ref = Some(String::from("main"));
	cleanup.commit_sha = Some(String::from("1123456789abcdef0123456789abcdef01234567"));
	cleanup.cleanup_status = Some(String::from("merged_closeout_reconciled"));
	cleanup.target_state = Some(String::from("Done"));
	cleanup.summary = Some(String::from("Merged closeout recovery marked cleanup complete."));

	records::validate_linear_execution_event_record(&cleanup)
		.expect("merged closeout cleanup event should validate");
}

#[test]
fn superseded_closeout_recovery_events_validate() {
	let mut closeout = LinearExecutionEventRecord::new(
		LinearExecutionEventIdentity {
			service_id: "pubfi-mono",
			issue_id: "issue-id",
			issue_identifier: "PUB-1704",
			run_id: "pub-1704-attempt-1",
			attempt_number: 1,
		},
		LEGACY_MANUAL_CLOSEOUT_EVENT,
		super::current_timestamp(),
		"anchor-closeout",
	);

	closeout.branch = Some(String::from("y/pubfi-pub-1704"));
	closeout.worktree_path = Some(String::from(".worktrees/PUB-1704"));
	closeout.pr_url = Some(String::from("https://github.com/helixbox/pubfi-mono/pull/826"));
	closeout.pr_head_sha = Some(String::from("0123456789abcdef0123456789abcdef01234567"));
	closeout.pr_base_ref = Some(String::from("main"));
	closeout.commit_sha = Some(String::from("1123456789abcdef0123456789abcdef01234567"));
	closeout.validation_result = Some(String::from("passed"));
	closeout.target_state = Some(String::from("Done"));
	closeout.summary = Some(String::from("Superseded closeout recovery recorded."));

	records::validate_linear_execution_event_record(&closeout)
		.expect("superseded closeout event should validate");

	let mut cleanup = LinearExecutionEventRecord::new(
		LinearExecutionEventIdentity {
			service_id: "pubfi-mono",
			issue_id: "issue-id",
			issue_identifier: "PUB-1704",
			run_id: "pub-1704-attempt-1",
			attempt_number: 1,
		},
		"cleanup_complete",
		super::timestamp_after_seconds(1),
		"anchor-cleanup",
	);

	cleanup.branch = Some(String::from("y/pubfi-pub-1704"));
	cleanup.worktree_path = Some(String::from(".worktrees/PUB-1704"));
	cleanup.pr_url = Some(String::from("https://github.com/helixbox/pubfi-mono/pull/826"));
	cleanup.pr_head_sha = Some(String::from("0123456789abcdef0123456789abcdef01234567"));
	cleanup.pr_base_ref = Some(String::from("main"));
	cleanup.commit_sha = Some(String::from("1123456789abcdef0123456789abcdef01234567"));
	cleanup.cleanup_status = Some(String::from("superseded_closeout_reconciled"));
	cleanup.target_state = Some(String::from("Done"));
	cleanup.summary = Some(String::from("Superseded closeout recovery marked cleanup complete."));

	records::validate_linear_execution_event_record(&cleanup)
		.expect("superseded closeout cleanup event should validate");
}

#[test]
fn review_handoff_rebind_event_requires_evidence() {
	let mut record = LinearExecutionEventRecord::new(
		LinearExecutionEventIdentity {
			service_id: "pubfi",
			issue_id: "issue-id",
			issue_identifier: "PUB-718",
			run_id: "pub-718-attempt-1",
			attempt_number: 1,
		},
		REVIEW_HANDOFF_REBIND_EVENT,
		super::current_timestamp(),
		"anchor",
	);

	record.branch = Some(String::from("x/pubfi-pub-718"));
	record.pr_url = Some(String::from("https://github.com/hack-ink/pubfi-mono-v2/pull/14"));
	record.pr_head_sha = Some(String::from("0123456789abcdef0123456789abcdef01234567"));
	record.pr_base_ref = Some(String::from("main"));
	record.commit_sha = Some(String::from("0123456789abcdef0123456789abcdef01234567"));
	record.validation_result = Some(String::from("passed"));
	record.summary = Some(String::from("Explicit operator rebind restored lifecycle record."));

	let error = records::validate_linear_execution_event_record(&record)
		.expect_err("rebind event without evidence should fail");

	assert!(error.contains("evidence"));
}
