use std::{fs, path::Path};

use tempfile::TempDir;

use crate::{
	orchestrator::{
		IssueDispatchMode, RunSummary,
		tests::{self, TEST_SERVICE_ID, recovery_terminal_support},
	},
	test_support::TestEnvVarGuard,
	tracker::{self, TrackerIssue},
	worktree::WorktreeSpec,
};

pub(super) fn candidate_selection_service_owned_issue(state_name: &str) -> TrackerIssue {
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);

	tests::sample_issue(state_name, &[active_label.as_str()])
}

pub(super) fn install_merged_pr_response(
	temp_dir: &TempDir,
	worktree: &WorktreeSpec,
	pr_url: &str,
	head_oid: &str,
) -> TestEnvVarGuard {
	recovery_terminal_support::install_fake_merged_pr_gh_response(
		temp_dir, worktree, pr_url, head_oid,
	)
}

pub(super) fn sample_handoff_summary(issue: &TrackerIssue, worktree_path: &Path) -> RunSummary {
	RunSummary {
		project_id: String::from("pubfi"),
		issue_id: issue.id.clone(),
		issue_identifier: issue.identifier.clone(),
		issue_state: String::from("In Progress"),
		initial_issue_state: String::from("Todo"),
		retry_project_slug: String::from("pubfi"),
		dispatch_mode: IssueDispatchMode::Normal,
		branch_name: String::from("main"),
		worktree_path: worktree_path.to_path_buf(),
		attempt_number: 1,
		run_id: String::from("run-review-handoff"),
		continuation_pending: false,
		program_dispatch: None,
	}
}

pub(super) fn assert_admin_merge_invocation(
	invocation_log_path: &Path,
	head_oid: &str,
	landed_merge_subject: &str,
	pr_url: &str,
) {
	let gh_invocation = fs::read_to_string(invocation_log_path)
		.expect("fake gh invocation log should read")
		.lines()
		.map(str::to_owned)
		.collect::<Vec<_>>();
	let expected = vec![
		String::from("pr"),
		String::from("merge"),
		String::from("--admin"),
		String::from("--merge"),
		String::from("--match-head-commit"),
		String::from(head_oid),
		String::from("--subject"),
		String::from(landed_merge_subject),
		String::from("--body"),
		String::new(),
		String::from(pr_url),
		String::from("pr"),
		String::from("view"),
		String::from(pr_url),
		String::from("--json"),
		String::from("state,headRefOid,mergeCommit"),
	];

	assert!(gh_invocation.starts_with(&expected));

	for extra_view in gh_invocation[expected.len()..].chunks(5) {
		assert_eq!(
			extra_view,
			[
				String::from("pr"),
				String::from("view"),
				String::from(pr_url),
				String::from("--json"),
				String::from("state,headRefOid,mergeCommit"),
			]
		);
	}
}
