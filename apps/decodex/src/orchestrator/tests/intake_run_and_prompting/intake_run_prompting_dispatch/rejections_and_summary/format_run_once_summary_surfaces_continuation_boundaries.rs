use std::path::PathBuf;

use crate::orchestrator::{self, IssueDispatchMode, RunSummary};

#[test]
fn format_run_once_summary_surfaces_continuation_boundaries() {
	let summary = RunSummary {
		project_id: String::from("pubfi"),
		issue_id: String::from("issue-1"),
		issue_identifier: String::from("PUB-101"),
		issue_state: String::from("In Progress"),
		initial_issue_state: String::from("In Progress"),
		retry_project_slug: String::from("pubfi"),
		dispatch_mode: IssueDispatchMode::Normal,
		branch_name: String::from("x/pubfi-pub-101"),
		worktree_path: PathBuf::from(".worktrees/PUB-101"),
		attempt_number: 1,
		run_id: String::from("pub-101-attempt-1"),
		continuation_pending: true,
		program_dispatch: None,
	};
	let message = orchestrator::format_run_once_summary(&summary, false);

	assert!(message.contains("run paused at continuation boundary"));
	assert!(message.contains("next_action=rerun_or_use_daemon"));
	assert!(!message.contains("run complete"));
}
