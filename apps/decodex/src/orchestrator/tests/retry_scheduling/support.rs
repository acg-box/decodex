use std::{fs, path::Path, process::Command};

use crate::{
	orchestrator::{
		DaemonRunChild, IssueDispatchMode, PullRequestReviewState, tests, tests::TEST_SERVICE_ID,
	},
	tracker::{self, TrackerIssue},
	workflow::WorkflowDocument,
};

pub(super) const PUB_704_RETAINED_HEAD_SUBJECT: &str = r#"{"schema":"decodex/commit/2","change":"current retained handoff","authority":"PUB-704","impact":"compatible"}"#;
pub(super) const PUB_704_RETAINED_LANDED_SUBJECT: &str = r#"{"schema":"decodex/commit/2","change":"Land current retained handoff","authority":"PUB-704","impact":"compatible"}"#;

pub(super) fn sample_approved_clean_review_state(
	pr_url: &str,
	branch_name: &str,
	head_oid: &str,
) -> PullRequestReviewState {
	tests::sample_pull_request_review_state(
		pr_url,
		branch_name,
		head_oid,
		Some("APPROVED"),
		"MERGEABLE",
		"CLEAN",
		Some("SUCCESS"),
		0,
	)
}

pub(super) fn sample_service_owned_issue(state_name: &str) -> TrackerIssue {
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);

	tests::sample_issue(state_name, &[active_label.as_str()])
}

pub(super) fn sample_service_owned_issue_without_needs_attention_team_label(
	state_name: &str,
) -> TrackerIssue {
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);

	tests::sample_issue_without_needs_attention_team_label(state_name, &[active_label.as_str()])
}

pub(super) fn sample_service_owned_issue_with_project_slug_and_sort_fields(
	id: &str,
	identifier: &str,
	project_slug: &str,
	state_name: &str,
	sort_value: Option<i64>,
	updated_at: &str,
) -> TrackerIssue {
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);

	tests::sample_issue_with_project_slug_and_sort_fields(
		id,
		identifier,
		project_slug,
		state_name,
		&[active_label.as_str()],
		sort_value,
		updated_at,
	)
}

pub(super) fn assert_fake_admin_merge_invocation_present(
	invocation_log_path: &Path,
	head_oid: &str,
	merge_subject: &str,
	pr_url: &str,
) {
	let gh_invocation_log =
		fs::read_to_string(invocation_log_path).expect("fake gh invocation log should read");
	let expected_invocation = [
		"pr",
		"merge",
		"--admin",
		"--merge",
		"--match-head-commit",
		head_oid,
		"--subject",
		merge_subject,
		"--body",
		"",
		pr_url,
	]
	.join("\n");

	assert!(
		gh_invocation_log.contains(&expected_invocation),
		"fake gh invocation log should contain the admin merge command"
	);
}

pub(super) fn stop_daemon_children(active_children: &mut [DaemonRunChild]) {
	for daemon_child in active_children {
		let _ = daemon_child.child.kill();
		let _ = daemon_child.child.wait();
	}
}

pub(super) fn spawn_sleeping_daemon_child(
	active_issue: &TrackerIssue,
	workflow: &WorkflowDocument,
) -> DaemonRunChild {
	let child =
		Command::new("sh").args(["-c", "sleep 30"]).spawn().expect("child process should spawn");

	DaemonRunChild {
		child,
		issue_id: active_issue.id.clone(),
		run_id: String::from("leased-run"),
		attempt_number: 1,
		initial_issue_state: active_issue.state.name.clone(),
		retry_project_slug: active_issue
			.project_slug
			.clone()
			.expect("sample issue should carry a project slug"),
		dispatch_mode: IssueDispatchMode::Normal,
		from_retry_queue: false,
		workflow: workflow.clone(),
	}
}
