use std::fs;

use crate::{
	orchestrator::{
		self,
		tests::{self, FakeTracker, recovery_terminal_support},
	},
	state::{self, StateStore},
	worktree::WorktreeManager,
};

#[test]
fn cleanup_terminal_worktree_runs_before_remove_workspace_hook() {
	let workflow_markdown = r#"
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
max_turns = 1
max_retry_backoff_ms = 300000
gate_profiles = {}
canonicalize_commands = []
verify_commands = []

[execution.workspace_hooks]
after_create_commands = []
before_remove_commands = ["printf '%s:%s\n' \"$DECODEX_ISSUE_ID\" \"$DECODEX_BRANCH\" > \"$DECODEX_REPO_ROOT/before-remove.log\""]
timeout_seconds = 60

[context]
read_first = []
+++

Follow the repository policy.
	"#;
	let (_temp_dir, config, workflow) =
		tests::temp_project_layout_with_workflow_markdown(workflow_markdown);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree("PUB-101", false)
		.expect("worktree should exist before cleanup");

	orchestrator::cleanup_terminal_worktree(
		&state_store,
		&worktree_manager,
		&workflow,
		"issue-1",
		"PUB-101",
		&worktree.branch_name,
		&worktree.path,
	)
	.expect("cleanup should succeed");

	assert_eq!(
		fs::read_to_string(config.repo_root().join("before-remove.log"))
			.expect("before-remove hook log should exist"),
		"PUB-101:x/pubfi-pub-101\n"
	);
	assert!(!worktree.path.exists(), "cleanup should still remove the worktree");
	assert!(
		!tests::git_output(config.repo_root(), &["branch", "--list", &worktree.branch_name])
			.is_empty(),
		"generic terminal cleanup should preserve the retained local branch ref"
	);
}

#[test]
fn materialize_daemon_spawn_state_starts_fresh_budget_for_normal_queue_intake() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue("Todo", &[]);
	let tracker = FakeTracker::with_refresh_snapshots(
		vec![issue.clone()],
		vec![vec![issue.clone()], vec![issue.clone()]],
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let retained_worktree = worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("retained worktree should exist");

	state::write_run_retry_budget_attempt_count(&retained_worktree.path, "older-run", 4, 2)
		.expect("retry budget marker should write");

	let summary = orchestrator::run_project_once(&tracker, &config, &workflow, &state_store, true)
		.expect("dry-run planning should succeed")
		.expect("retained lane should still be selected");
	let daemon_spawn_state =
		orchestrator::materialize_daemon_spawn_state(&config, &workflow, &state_store, &summary)
			.expect("daemon parent should materialize worktree and retry budget together");

	assert_eq!(daemon_spawn_state.worktree.path, summary.worktree_path);
	assert_eq!(
		daemon_spawn_state.retry_budget_base, 0,
		"normal daemon queue intake should not inherit retry attempts from an old marker"
	);
}

#[test]
fn uses_retained_retry_budget_marker_for_recovery() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = recovery_terminal_support::sample_active_issue("In Progress");
	let tracker = FakeTracker::with_refresh_snapshots(
		vec![issue.clone()],
		vec![vec![issue.clone()], vec![issue.clone()]],
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let retained_worktree = worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("retained worktree should exist");

	state::write_run_retry_budget_attempt_count(&retained_worktree.path, "older-run", 4, 2)
		.expect("retry budget marker should write");

	let summary = orchestrator::run_project_once(&tracker, &config, &workflow, &state_store, true)
		.expect("dry-run planning should succeed")
		.expect("retained lane should still be selected");
	let daemon_spawn_state =
		orchestrator::materialize_daemon_spawn_state(&config, &workflow, &state_store, &summary)
			.expect("daemon parent should materialize worktree and retry budget together");

	assert_eq!(summary.dispatch_mode, orchestrator::IssueDispatchMode::Retry);
	assert_eq!(daemon_spawn_state.worktree.path, summary.worktree_path);
	assert_eq!(
		daemon_spawn_state.retry_budget_base, 2,
		"recovered retry handoff should preserve retry budget from the retained worktree marker"
	);
}
