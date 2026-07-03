use std::path::PathBuf;

use crate::{
	pull_request::PullRequestLandingState,
	tracker::{TrackerIssue, TrackerLabel, TrackerState, TrackerTeam},
	workflow::WorkflowDocument,
};

pub(in crate::manual::tests) struct MergedManualLandBranch {
	pub(in crate::manual::tests) branch_name: String,
	pub(in crate::manual::tests) head_oid: String,
	pub(in crate::manual::tests) merge_commit: String,
	pub(in crate::manual::tests) worktree_path: PathBuf,
}

pub(in crate::manual::tests) fn merged_manual_land_state(
	branch_name: &str,
	head_oid: &str,
) -> PullRequestLandingState {
	let mut landing_state = sample_landing_state();

	landing_state.state = String::from("MERGED");
	landing_state.base_ref_name = String::from("main");
	landing_state.head_ref_name = branch_name.to_owned();
	landing_state.head_ref_oid = head_oid.to_owned();

	landing_state
}

pub(in crate::manual::tests) fn sample_workflow() -> WorkflowDocument {
	WorkflowDocument::parse_markdown(
		r#"
+++
version = 1

[tracker]
provider = "linear"
startable_states = ["Todo"]
terminal_states = ["Done", "Canceled", "Duplicate"]
in_progress_state = "In Progress"
success_state = "In Review"
completed_state = "Done"
failure_state = "Todo"
opt_out_label = "decodex:manual-only"
needs_attention_label = "decodex:needs-attention"

[agent]
transport = "stdio://"

[execution]
max_attempts = 3
max_turns = 8
max_retry_backoff_ms = 300000
gate_profiles = {}
canonicalize_commands = []
verify_commands = []

[execution.workspace_hooks]
after_create_commands = []
before_remove_commands = []
timeout_seconds = 60

[context]
read_first = []
+++

Test workflow.
"#,
	)
	.expect("sample workflow should parse")
}

pub(in crate::manual::tests) fn sample_landing_state() -> PullRequestLandingState {
	PullRequestLandingState {
		url: String::from("https://github.com/hack-ink/decodex/pull/64"),
		state: String::from("OPEN"),
		is_draft: false,
		review_decision: Some(String::from("APPROVED")),
		base_ref_name: String::from("release/1.x"),
		pending_review_requests: 0,
		mergeable: String::from("MERGEABLE"),
		merge_state_status: String::from("CLEAN"),
		head_ref_name: String::from("XY-225"),
		head_ref_oid: String::from("deadbeef"),
		status_check_rollup_state: Some(String::from("SUCCESS")),
		unresolved_review_threads: 0,
	}
}

pub(in crate::manual::tests) fn sample_issue(
	id: &str,
	identifier: &str,
	labels_complete: bool,
	labels: &[&str],
) -> TrackerIssue {
	TrackerIssue {
		id: id.to_owned(),
		identifier: identifier.to_owned(),
		#[cfg(test)]
		project_slug: None,
		title: String::from("Sample issue"),
		author: None,
		description: String::new(),
		priority: None,
		created_at: String::from("2026-04-13T00:00:00Z"),
		updated_at: String::from("2026-04-13T00:00:00Z"),
		state: TrackerState { id: String::from("state-1"), name: String::from("In Review") },
		team: TrackerTeam {
			id: String::from("team-1"),
			name: String::from("Core"),
			states: vec![TrackerState {
				id: String::from("state-1"),
				name: String::from("In Review"),
			}],
			labels: labels
				.iter()
				.enumerate()
				.map(|(index, label)| TrackerLabel {
					id: format!("team-label-{index}"),
					name: (*label).to_owned(),
				})
				.collect(),
		},
		labels_complete,
		labels: labels
			.iter()
			.enumerate()
			.map(|(index, label)| TrackerLabel {
				id: format!("issue-label-{index}"),
				name: (*label).to_owned(),
			})
			.collect(),
		blockers: Vec::new(),
	}
}
