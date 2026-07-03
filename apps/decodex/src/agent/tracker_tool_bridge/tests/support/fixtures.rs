use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::{
	agent::tracker_tool_bridge::{
		LocalRepoDetails, PullRequestDetails, ReviewExecutionMode, ReviewHandoffContext,
		tests::{TEST_SERVICE_ID, support::fakes::FakeTracker},
	},
	config::ReviewLevel,
	tracker::{TrackerIssue, TrackerLabel, TrackerState, TrackerTeam},
	workflow::WorkflowDocument,
};

pub(crate) fn sample_issue() -> TrackerIssue {
	TrackerIssue {
		id: String::from("issue-1"),
		identifier: String::from("DEC-1"),
		#[cfg(test)]
		project_slug: Some(String::from("decodex")),
		title: String::from("Sample"),
		author: None,
		description: String::from("Body"),
		priority: Some(3),
		created_at: String::from("2026-03-13T04:16:17.133Z"),
		updated_at: String::from("2026-03-13T04:16:17.133Z"),
		state: TrackerState { id: String::from("state-todo"), name: String::from("Todo") },
		team: TrackerTeam {
			id: String::from("team-1"),
			name: String::from("Decodex"),
			states: vec![
				TrackerState { id: String::from("state-todo"), name: String::from("Todo") },
				TrackerState {
					id: String::from("state-progress"),
					name: String::from("In Progress"),
				},
				TrackerState { id: String::from("state-review"), name: String::from("In Review") },
			],
			labels: vec![
				TrackerLabel {
					id: String::from("label-queued"),
					name: crate::tracker::automation_queue_label(TEST_SERVICE_ID),
				},
				TrackerLabel {
					id: String::from("label-active"),
					name: crate::tracker::automation_active_label(TEST_SERVICE_ID),
				},
				TrackerLabel {
					id: String::from("label-manual"),
					name: String::from("decodex:manual-only"),
				},
				TrackerLabel {
					id: String::from("label-needs"),
					name: String::from("decodex:needs-attention"),
				},
			],
		},
		labels_complete: true,
		labels: Vec::new(),
		blockers: Vec::new(),
	}
}

pub(crate) fn sample_in_progress_issue() -> TrackerIssue {
	let mut issue = sample_issue();

	issue.state =
		TrackerState { id: String::from("state-progress"), name: String::from("In Progress") };

	issue
}

pub(crate) fn sample_review_issue() -> TrackerIssue {
	let mut issue = sample_issue();

	issue.state =
		TrackerState { id: String::from("state-review"), name: String::from("In Review") };

	issue
}

pub(crate) fn tracker_with_current_issue_snapshot(issue: &TrackerIssue) -> FakeTracker {
	FakeTracker::with_refresh_snapshots(vec![vec![issue.clone()]])
}

pub(crate) fn sample_workflow() -> WorkflowDocument {
	sample_workflow_with_tracker_states(&["Todo"], "In Progress", "In Review", "Todo")
}

pub(crate) fn sample_workflow_with_startable_states(startable_states: &[&str]) -> WorkflowDocument {
	sample_workflow_with_tracker_states(startable_states, "In Progress", "In Review", "Todo")
}

pub(crate) fn sample_workflow_with_tracker_states(
	startable_states: &[&str],
	in_progress_state: &str,
	success_state: &str,
	failure_state: &str,
) -> WorkflowDocument {
	let startable_states =
		startable_states.iter().map(|state| format!("\"{state}\"")).collect::<Vec<_>>().join(", ");

	WorkflowDocument::parse_markdown(&format!(
		r#"
+++
version = 1

[tracker]
provider = "linear"
startable_states = [{startable_states}]
terminal_states = ["Done", "Canceled", "Duplicate"]
in_progress_state = "{in_progress_state}"
success_state = "{success_state}"
completed_state = "Done"
failure_state = "{failure_state}"
opt_out_label = "decodex:manual-only"
needs_attention_label = "decodex:needs-attention"

[agent]
transport = "stdio://"

[execution]
max_attempts = 3
max_turns = 1
max_retry_backoff_ms = 300000
gate_profiles = {{}}
canonicalize_commands = []
verify_commands = []

[execution.workspace_hooks]
after_create_commands = []
before_remove_commands = []
timeout_seconds = 60

[context]
read_first = []
+++

Use the tracker tools.
"#,
	))
	.expect("workflow should parse")
}

pub(crate) fn sample_review_context() -> ReviewHandoffContext {
	ReviewHandoffContext {
		attempt_number: 2,
		branch_name: String::from("x/decodex-pub-618"),
		run_id: String::from("pub-618-attempt-2-123"),
		service_id: String::from(TEST_SERVICE_ID),
		worktree_path: String::from(".worktrees/PUB-618"),
		cwd: PathBuf::from("/tmp/PUB-618"),
		github_token_env_var: Some(String::from("HOME")),
		github_command_path: None,
		review_level: ReviewLevel::Standard,
		mode: ReviewExecutionMode::Handoff,
		recorded_pr_url: None,
	}
}

pub(crate) fn manual_attention_comment_args() -> Value {
	serde_json::json!({
		"kind": "manual_attention",
		"error_class": "operator_decision_required",
		"next_action": "resolve the blocker manually, clear the needs-attention label, then restart automation if desired",
		"blockers": ["operator decision is required before automation can continue"],
		"evidence": ["agent selected the manual-attention path for this run"],
		"failed_command": "cargo make test",
		"raw_error": "repo gate failed with public test output"
	})
}

pub(crate) fn sample_review_context_in(cwd: &Path) -> ReviewHandoffContext {
	ReviewHandoffContext {
		attempt_number: 2,
		branch_name: String::from("x/decodex-pub-618"),
		run_id: String::from("pub-618-attempt-2-123"),
		service_id: String::from(TEST_SERVICE_ID),
		worktree_path: String::from(".worktrees/PUB-618"),
		cwd: cwd.to_path_buf(),
		github_token_env_var: Some(String::from("HOME")),
		github_command_path: None,
		review_level: ReviewLevel::Standard,
		mode: ReviewExecutionMode::Handoff,
		recorded_pr_url: None,
	}
}

pub(crate) fn sample_review_repair_context_in(cwd: &Path, pr_url: &str) -> ReviewHandoffContext {
	ReviewHandoffContext {
		attempt_number: 3,
		branch_name: String::from("x/decodex-pub-618"),
		run_id: String::from("pub-618-attempt-3-123"),
		service_id: String::from(TEST_SERVICE_ID),
		worktree_path: String::from(".worktrees/PUB-618"),
		cwd: cwd.to_path_buf(),
		github_token_env_var: Some(String::from("HOME")),
		github_command_path: None,
		review_level: ReviewLevel::Standard,
		mode: ReviewExecutionMode::Repair,
		recorded_pr_url: Some(pr_url.to_owned()),
	}
}

pub(crate) fn sample_closeout_context_in(cwd: &Path, pr_url: &str) -> ReviewHandoffContext {
	ReviewHandoffContext {
		attempt_number: 4,
		branch_name: String::from("x/decodex-pub-618"),
		run_id: String::from("pub-618-attempt-4-123"),
		service_id: String::from(TEST_SERVICE_ID),
		worktree_path: String::from(".worktrees/PUB-618"),
		cwd: cwd.to_path_buf(),
		github_token_env_var: Some(String::from("HOME")),
		github_command_path: None,
		review_level: ReviewLevel::Standard,
		mode: ReviewExecutionMode::Closeout,
		recorded_pr_url: Some(pr_url.to_owned()),
	}
}

pub(crate) fn sample_pull_request() -> PullRequestDetails {
	PullRequestDetails {
		head_ref_name: String::from("x/decodex-pub-618"),
		head_ref_oid: String::from("08a20f7dfb9526e7421a5f095b1c6adec84e52d6"),
		head_repository_name: String::from("decodex"),
		head_repository_owner: String::from("hack-ink"),
		is_draft: false,
		state: String::from("OPEN"),
		base_ref_name: String::from("main"),
		url: String::from("https://github.com/hack-ink/decodex/pull/48"),
	}
}

pub(crate) fn sample_local_repo() -> LocalRepoDetails {
	LocalRepoDetails {
		default_branch: String::from("main"),
		head_oid: String::from("08a20f7dfb9526e7421a5f095b1c6adec84e52d6"),
		head_tree_oid: String::from("f8a20f7dfb9526e7421a5f095b1c6adec84e52d6"),
		repository_name: String::from("decodex"),
		repository_owner: String::from("hack-ink"),
		review_blocking_changes: Vec::new(),
	}
}
