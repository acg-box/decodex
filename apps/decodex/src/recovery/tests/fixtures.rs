use std::{fs, path::Path};

use tempfile::TempDir;

use crate::{
	config::ServiceConfig,
	pull_request::PullRequestLandingState,
	recovery::{RecoveryContext, RecoveryRuntimeMutationPolicy},
	state::{StateStore, WorktreeMapping},
	tracker::{TrackerIssue, TrackerLabel, TrackerState, TrackerTeam, linear::LinearClient},
	workflow::WorkflowDocument,
};

pub(in crate::recovery::tests) fn sample_worktree(branch_name: &str) -> WorktreeMapping {
	sample_worktree_at(branch_name, Path::new("/tmp/PUB-718"))
}

pub(in crate::recovery::tests) fn sample_worktree_at(
	branch_name: &str,
	worktree_path: &Path,
) -> WorktreeMapping {
	let store = StateStore::open_in_memory().expect("state store should open");
	let worktree_path = worktree_path.to_string_lossy();

	store
		.upsert_worktree("pubfi", "issue-id", branch_name, &worktree_path)
		.expect("worktree should persist");

	store
		.worktree_for_issue("issue-id")
		.expect("worktree should read")
		.expect("worktree should exist")
}

pub(in crate::recovery::tests) fn sample_landing_state(
	pr_url: &str,
	branch_name: &str,
	head_oid: &str,
) -> PullRequestLandingState {
	PullRequestLandingState {
		url: pr_url.to_owned(),
		state: String::from("OPEN"),
		is_draft: false,
		review_decision: Some(String::from("APPROVED")),
		base_ref_name: String::from("main"),
		base_ref_oid: Some(String::from("base-sha")),
		pending_review_requests: 0,
		mergeable: String::from("MERGEABLE"),
		merge_state_status: String::from("CLEAN"),
		head_ref_name: branch_name.to_owned(),
		head_ref_oid: head_oid.to_owned(),
		status_check_rollup_state: Some(String::from("SUCCESS")),
		required_status_contexts: Vec::new(),
		unresolved_review_threads: 0,
	}
}

pub(in crate::recovery::tests) fn sample_workflow() -> WorkflowDocument {
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

pub(in crate::recovery::tests) fn sample_recovery_context(
	temp_dir: &TempDir,
	runtime_mutation_policy: RecoveryRuntimeMutationPolicy,
) -> RecoveryContext {
	let repo_root = temp_dir.path().join("repo");
	let config_path = temp_dir.path().join("project.toml");

	fs::create_dir_all(&repo_root).expect("repo root should exist");
	fs::write(
		&config_path,
		r#"
service_id = "pubfi"

[paths]
repo_root = "repo"

[tracker]
api_key_env_var = "HOME"
team_id = "team-test"

[github]
token_env_var = "HOME"
owner = "test-owner"
repository = "test-repository"
"#,
	)
	.expect("config should write");

	RecoveryContext {
		config: ServiceConfig::from_path(&config_path).expect("config should load"),
		workflow: sample_workflow(),
		state_store: StateStore::open_in_memory().expect("state store should open"),
		tracker: LinearClient::new(String::from("test-token")).expect("linear client should build"),
		runtime_mutation_policy,
	}
}

pub(in crate::recovery::tests) fn sample_issue(state_name: &str) -> TrackerIssue {
	let states = vec![
		TrackerState { id: String::from("state-todo"), name: String::from("Todo") },
		TrackerState { id: String::from("state-progress"), name: String::from("In Progress") },
		TrackerState { id: String::from("state-review"), name: String::from("In Review") },
		TrackerState { id: String::from("state-done"), name: String::from("Done") },
	];
	let state = states
		.iter()
		.find(|state| state.name == state_name)
		.expect("sample state should exist")
		.clone();

	TrackerIssue {
		id: String::from("issue-id"),
		identifier: String::from("PUB-718"),
		#[cfg(test)]
		project_slug: None,
		title: String::from("Sample issue"),
		author: None,
		description: String::new(),
		priority: None,
		created_at: String::from("2026-06-09T00:00:00Z"),
		updated_at: String::from("2026-06-09T00:00:00Z"),
		state,
		team: TrackerTeam {
			id: String::from("team-id"),
			name: String::from("XY"),
			states,
			labels: Vec::new(),
		},
		labels_complete: true,
		labels: Vec::new(),
		blockers: Vec::new(),
	}
}

pub(in crate::recovery::tests) fn sample_issue_with_labels(
	state_name: &str,
	labels: &[String],
) -> TrackerIssue {
	let mut issue = sample_issue(state_name);

	for label in labels {
		let tracker_label =
			TrackerLabel { id: format!("label-{}", label.replace(':', "-")), name: label.clone() };

		issue.team.labels.push(tracker_label.clone());
		issue.labels.push(tracker_label);
	}

	issue
}
