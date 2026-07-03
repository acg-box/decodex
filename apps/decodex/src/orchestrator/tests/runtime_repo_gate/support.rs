use crate::{
	orchestrator::{IssueDispatchMode, IssueRunPlan, ServiceConfig, StateStore, tests},
	tracker::TrackerIssue,
	worktree::WorktreeSpec,
};

pub(in crate::orchestrator::tests) fn record_phase_acceptance_progress_checkpoint(
	config: &ServiceConfig,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
	blockers: &[&str],
) {
	let head_sha = tests::git_output(config.repo_root(), &["rev-parse", "HEAD"]);
	let blockers = blockers.iter().map(|blocker| (*blocker).to_owned()).collect::<Vec<_>>();

	state_store
		.append_private_execution_event(
			config.service_id(),
			&issue_run.issue.id,
			&issue_run.run_id,
			issue_run.attempt_number,
			"progress_checkpoint",
			serde_json::json!({
				"phase": "verifying",
				"docs_impact": "none",
				"focus": "Validate phase-specific work before handoff.",
				"next_action": "Complete the active phase goal.",
				"blockers": blockers,
				"evidence": ["current worktree inspected"],
				"verification": ["repo gate will run after phase goal completion"],
				"head_sha": head_sha,
				"branch": issue_run.worktree.branch_name.as_str(),
				"worktree_path": issue_run.worktree.path.display().to_string(),
			}),
		)
		.expect("phase acceptance progress checkpoint should record");
}

pub(super) fn phase_goal_repo_gate_issue_run(
	config: &ServiceConfig,
	issue: &TrackerIssue,
) -> IssueRunPlan {
	IssueRunPlan {
		issue: issue.clone(),
		issue_state: String::from("In Progress"),
		initial_issue_state: String::from("Todo"),
		worktree: WorktreeSpec {
			branch_name: String::from("x/pubfi-pub-101"),
			issue_identifier: issue.identifier.clone(),
			path: config.repo_root().to_path_buf(),
			reused_existing: false,
		},
		retry_project_slug: String::from("pubfi"),
		dispatch_mode: IssueDispatchMode::Normal,
		attempt_number: 1,
		run_id: String::from("pub-101-attempt-1"),
		retry_budget_base: 0,
	}
}

pub(super) fn review_repair_phase_goal_issue_run(
	config: &ServiceConfig,
	issue: &TrackerIssue,
) -> IssueRunPlan {
	IssueRunPlan {
		issue: issue.clone(),
		issue_state: String::from("In Review"),
		initial_issue_state: String::from("In Review"),
		worktree: WorktreeSpec {
			branch_name: String::from("x/pubfi-pub-101"),
			issue_identifier: issue.identifier.clone(),
			path: config.repo_root().to_path_buf(),
			reused_existing: true,
		},
		retry_project_slug: String::from("pubfi"),
		dispatch_mode: IssueDispatchMode::ReviewRepair,
		attempt_number: 3,
		run_id: String::from("pub-101-attempt-3"),
		retry_budget_base: 0,
	}
}
