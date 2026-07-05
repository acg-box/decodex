use std::fs;

use crate::{
	orchestrator::{
		self, IssueDispatchMode, PrepareIssueRunContext,
		tests::{self, FakeTracker},
	},
	state::StateStore,
	worktree::WorktreeManager,
};

#[test]
fn prepare_issue_run_runs_after_create_workspace_hook() {
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
after_create_commands = ["printf '%s\n' \"$DECODEX_BRANCH\" > \"$DECODEX_REPO_ROOT/after-create.log\""]
before_remove_commands = []
timeout_seconds = 60

[context]
read_first = []
+++

Follow the repository policy.
	"#;
	let (_temp_dir, base_config, workflow) =
		tests::temp_project_layout_with_workflow_markdown(workflow_markdown);
	let config = tests::service_config_with_github_token_env_var(&base_config, "HOME");
	let issue = tests::sample_issue("Todo", &[]);
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let issue_run = orchestrator::prepare_issue_run(
		PrepareIssueRunContext {
			tracker: &tracker,
			project: &config,
			workflow: &workflow,
			state_store: &state_store,
			worktree_manager: &worktree_manager,
			dry_run: false,
			lease_preacquired: false,
			dispatch_mode: IssueDispatchMode::Normal,
			preferred_issue_state: None,
			preferred_initial_issue_state: None,
			preferred_run_identity: None,
			preferred_retry_budget_base: None,
		},
		issue,
	)
	.expect("issue preparation should succeed")
	.expect("startable issue should prepare");

	assert_eq!(
		fs::read_to_string(config.repo_root().join("after-create.log"))
			.expect("after-create hook log should exist"),
		format!("{}\n", issue_run.worktree.branch_name)
	);
}
