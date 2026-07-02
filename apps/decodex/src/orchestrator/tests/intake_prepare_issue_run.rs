use std::fs;
#[cfg(unix)] use std::os::fd::IntoRawFd;

use crate::{
	orchestrator::{
		self, IssueDispatchMode, PreferredRunIdentity, PrepareIssueRunContext,
		TargetIssueRunContext,
		tests::{self, FakeTracker, TEST_SERVICE_ID},
	},
	state::{self, PreacquiredLeaseGuards, StateStore},
	tracker::{self, records},
	worktree::WorktreeManager,
};

#[test]
fn prepare_issue_run_records_starting_attempt_before_execute() {
	let (_temp_dir, base_config, workflow) = tests::temp_project_layout();
	let config = tests::service_config_with_github_token_env_var(&base_config, "HOME");
	let issue = tests::sample_issue(
		"In Progress",
		&[tracker::automation_active_label(TEST_SERVICE_ID).as_str()],
	);
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
			dispatch_mode: IssueDispatchMode::Retry,
			preferred_issue_state: None,
			preferred_initial_issue_state: None,
			preferred_run_identity: None,
			preferred_retry_budget_base: None,
		},
		issue.clone(),
	)
	.expect("issue preparation should succeed")
	.expect("active retry issue should prepare");

	assert_eq!(
		state_store
			.run_attempt(&issue_run.run_id)
			.expect("run attempt lookup should succeed")
			.expect("run attempt should exist")
			.status(),
		"starting"
	);
	assert_eq!(
		state_store
			.lease_for_issue(&issue.id)
			.expect("lease lookup should succeed")
			.expect("lease should exist")
			.run_id(),
		issue_run.run_id
	);

	let event_types = tracker
		.comments
		.borrow()
		.iter()
		.filter_map(|comment| records::parse_linear_execution_event_record(comment))
		.map(|record| record.event_type)
		.collect::<Vec<_>>();

	assert_eq!(event_types, vec![String::from("run_started")]);
}

#[test]
fn prepare_issue_run_rejects_missing_read_first_before_lease_or_attempt() {
	let workflow_markdown = tests::sample_workflow_markdown(
		"pubfi",
		&["docs/guide/getting_started.md"],
		"Follow the repository policy.\n",
		1,
	);
	let (_temp_dir, base_config, workflow) =
		tests::temp_project_layout_with_workflow_markdown(&workflow_markdown);
	let config = tests::service_config_with_github_token_env_var(&base_config, "HOME");
	let issue = tests::sample_issue("Todo", &[]);
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let error = orchestrator::prepare_issue_run(
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
		issue.clone(),
	)
	.expect_err("missing read_first file should reject dispatch");
	let message = format!("{error:#}");

	assert!(message.contains("context.read_first"));
	assert!(message.contains("docs/guide/getting_started.md"));
	assert!(
		message.contains(config.workflow_path().to_str().expect("workflow path should be utf-8"))
	);
	assert!(
		state_store.lease_for_issue(&issue.id).expect("lease lookup should succeed").is_none(),
		"read_first preflight must reject before acquiring a lease"
	);
	assert!(
		state_store
			.list_run_attempts_for_issue(&issue.id)
			.expect("attempt lookup should succeed")
			.is_empty(),
		"read_first preflight must reject before recording an attempt"
	);
	assert!(
		state_store
			.worktree_for_issue(&issue.id)
			.expect("worktree lookup should succeed")
			.is_none(),
		"read_first preflight must reject before recording worktree ownership"
	);
}

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

#[test]
fn prepare_issue_run_starts_fresh_retry_budget_for_normal_queue_intake() {
	let (_temp_dir, base_config, workflow) = tests::temp_project_layout();
	let config = tests::service_config_with_github_token_env_var(&base_config, "HOME");
	let issue = tests::sample_issue("Todo", &[]);
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree =
		worktree_manager.ensure_worktree(&issue.identifier, false).expect("worktree should exist");

	state::write_run_retry_budget_attempt_count(&worktree.path, "older-run", 4, 2)
		.expect("retry budget marker should write");

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
		issue_run.retry_budget_base, 0,
		"normal queue intake starts a new automatic retry episode instead of inheriting old marker attempts"
	);
}

#[test]
fn prepare_issue_run_uses_persisted_retry_budget_marker_for_recovered_retry() {
	let (_temp_dir, base_config, workflow) = tests::temp_project_layout();
	let config = tests::service_config_with_github_token_env_var(&base_config, "HOME");
	let issue = tests::sample_issue(
		"In Progress",
		&[tracker::automation_active_label(TEST_SERVICE_ID).as_str()],
	);
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree =
		worktree_manager.ensure_worktree(&issue.identifier, false).expect("worktree should exist");

	state::write_run_retry_budget_attempt_count(&worktree.path, "older-run", 4, 2)
		.expect("retry budget marker should write");

	let issue_run = orchestrator::prepare_issue_run(
		PrepareIssueRunContext {
			tracker: &tracker,
			project: &config,
			workflow: &workflow,
			state_store: &state_store,
			worktree_manager: &worktree_manager,
			dry_run: false,
			lease_preacquired: false,
			dispatch_mode: IssueDispatchMode::Retry,
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
		issue_run.retry_budget_base, 2,
		"recovered retry dispatch should preserve retry budget from the retained worktree marker"
	);
}

#[test]
fn prepare_issue_run_keeps_persisted_retry_budget_when_preferred_retry_base_is_stale() {
	let (_temp_dir, base_config, workflow) = tests::temp_project_layout();
	let config = tests::service_config_with_github_token_env_var(&base_config, "HOME");
	let issue = tests::sample_issue(
		"In Progress",
		&[tracker::automation_active_label(TEST_SERVICE_ID).as_str()],
	);
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree =
		worktree_manager.ensure_worktree(&issue.identifier, false).expect("worktree should exist");

	state::write_run_retry_budget_attempt_count(&worktree.path, "older-run", 4, 2)
		.expect("retry budget marker should write");

	let issue_run = orchestrator::prepare_issue_run(
		PrepareIssueRunContext {
			tracker: &tracker,
			project: &config,
			workflow: &workflow,
			state_store: &state_store,
			worktree_manager: &worktree_manager,
			dry_run: false,
			lease_preacquired: false,
			dispatch_mode: IssueDispatchMode::Retry,
			preferred_issue_state: None,
			preferred_initial_issue_state: None,
			preferred_run_identity: None,
			preferred_retry_budget_base: Some(0),
		},
		issue,
	)
	.expect("issue preparation should succeed")
	.expect("recovered retry issue should prepare");

	assert_eq!(
		issue_run.retry_budget_base, 2,
		"preferred retry-budget base should not hide retained retry episode state"
	);
}

#[test]
fn prepare_issue_run_honors_preferred_identity_when_attempt_is_current() {
	let (_temp_dir, base_config, workflow) = tests::temp_project_layout();
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
			preferred_run_identity: Some(PreferredRunIdentity {
				run_id: "planned-run",
				attempt_number: 1,
			}),
			preferred_retry_budget_base: None,
		},
		issue.clone(),
	)
	.expect("issue preparation should succeed")
	.expect("targeted issue should prepare");

	assert_eq!(issue_run.run_id, "planned-run");
	assert_eq!(issue_run.attempt_number, 1);
	assert_eq!(
		state_store
			.lease_for_issue(&issue.id)
			.expect("lease lookup should succeed")
			.expect("lease should exist")
			.run_id(),
		"planned-run"
	);
}

#[cfg(unix)]
#[test]
fn prepare_issue_run_allows_preacquired_cross_process_slot() {
	let (_temp_dir, base_config, workflow) = tests::temp_project_layout();
	let config = tests::service_config_with_github_token_env_var(&base_config, "HOME");
	let issue = tests::sample_issue("Todo", &[]);
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let parent_store = StateStore::open_in_memory().expect("parent state store should open");
	let child_store = StateStore::open_in_memory().expect("child state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());

	parent_store
		.configure_dispatch_slot_root(config.service_id(), config.worktree_root())
		.expect("parent dispatch-slot root should configure");
	child_store
		.configure_dispatch_slot_root(config.service_id(), config.worktree_root())
		.expect("child dispatch-slot root should configure");

	assert!(
		parent_store
			.try_acquire_lease(config.service_id(), &issue.id, "planned-run", "In Progress")
			.expect("parent should acquire the shared dispatch slot")
	);

	let child_issue_claim = parent_store
		.clone_issue_claim_for_child(&issue.id)
		.expect("child should inherit the shared issue-claim fd");
	let (child_guard, child_slot_index) = parent_store
		.clone_dispatch_slot_for_child(&issue.id)
		.expect("child should inherit the shared dispatch-slot fd");

	child_store
		.adopt_preacquired_lease(
			config.service_id(),
			&issue.id,
			"planned-run",
			"In Progress",
			PreacquiredLeaseGuards {
				issue_claim_fd: child_issue_claim.into_raw_fd(),
				dispatch_slot_fd: child_guard.into_raw_fd(),
				dispatch_slot_index: child_slot_index,
			},
		)
		.expect("child should adopt the inherited lease guard");

	let issue_run = orchestrator::prepare_issue_run(
		PrepareIssueRunContext {
			tracker: &tracker,
			project: &config,
			workflow: &workflow,
			state_store: &child_store,
			worktree_manager: &worktree_manager,
			dry_run: false,
			lease_preacquired: true,
			dispatch_mode: IssueDispatchMode::Normal,
			preferred_issue_state: None,
			preferred_initial_issue_state: None,
			preferred_run_identity: Some(PreferredRunIdentity {
				run_id: "planned-run",
				attempt_number: 1,
			}),
			preferred_retry_budget_base: None,
		},
		issue.clone(),
	)
	.expect("preacquired issue preparation should succeed")
	.expect("targeted issue should prepare under the preacquired lease");

	assert_eq!(issue_run.run_id, "planned-run");
	assert_eq!(
		child_store
			.lease_for_issue(&issue.id)
			.expect("child lease lookup should succeed")
			.expect("child should retain the adopted local lease")
			.run_id(),
		"planned-run",
		"preacquired child runs should keep the adopted local lease so cleanup can release the handoff guard"
	);
}

#[cfg(unix)]
#[test]
fn prepare_issue_run_allows_preacquired_recovered_retry_attempt() {
	let (_temp_dir, base_config, workflow) = tests::temp_project_layout();
	let config = tests::service_config_with_github_token_env_var(&base_config, "HOME");
	let issue = tests::sample_issue(
		"In Progress",
		&[tracker::automation_active_label(TEST_SERVICE_ID).as_str()],
	);
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let parent_store = StateStore::open_in_memory().expect("parent state store should open");
	let child_store = StateStore::open_in_memory().expect("child state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());

	parent_store
		.configure_dispatch_slot_root(config.service_id(), config.worktree_root())
		.expect("parent dispatch-slot root should configure");
	child_store
		.configure_dispatch_slot_root(config.service_id(), config.worktree_root())
		.expect("child dispatch-slot root should configure");

	assert!(
		parent_store
			.try_acquire_lease(config.service_id(), &issue.id, "planned-run", "In Progress")
			.expect("parent should acquire the shared dispatch slot")
	);

	let child_issue_claim = parent_store
		.clone_issue_claim_for_child(&issue.id)
		.expect("child should inherit the shared issue-claim fd");
	let (child_guard, child_slot_index) = parent_store
		.clone_dispatch_slot_for_child(&issue.id)
		.expect("child should inherit the shared dispatch-slot fd");

	child_store
		.adopt_preacquired_lease(
			config.service_id(),
			&issue.id,
			"planned-run",
			"In Progress",
			PreacquiredLeaseGuards {
				issue_claim_fd: child_issue_claim.into_raw_fd(),
				dispatch_slot_fd: child_guard.into_raw_fd(),
				dispatch_slot_index: child_slot_index,
			},
		)
		.expect("child should adopt the inherited lease guard");
	child_store
		.record_run_attempt("planned-run", &issue.id, 1, "running")
		.expect("recovered attempt should record before targeted execution");

	let issue_run = orchestrator::prepare_issue_run(
		PrepareIssueRunContext {
			tracker: &tracker,
			project: &config,
			workflow: &workflow,
			state_store: &child_store,
			worktree_manager: &worktree_manager,
			dry_run: false,
			lease_preacquired: true,
			dispatch_mode: IssueDispatchMode::Retry,
			preferred_issue_state: None,
			preferred_initial_issue_state: None,
			preferred_run_identity: Some(PreferredRunIdentity {
				run_id: "planned-run",
				attempt_number: 1,
			}),
			preferred_retry_budget_base: None,
		},
		issue.clone(),
	)
	.expect("recovered retry preparation should succeed")
	.expect("planned retry attempt should still execute");

	assert_eq!(issue_run.run_id, "planned-run");
	assert_eq!(issue_run.attempt_number, 1);
	assert_eq!(
		child_store
			.lease_for_issue(&issue.id)
			.expect("child lease lookup should succeed")
			.expect("child should retain the adopted local lease")
			.run_id(),
		"planned-run"
	);
}

#[cfg(unix)]
#[test]
fn prepare_issue_run_allows_preacquired_cross_process_slot_without_github_token_authority() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue("Todo", &[]);
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let parent_store = StateStore::open_in_memory().expect("parent state store should open");
	let child_store = StateStore::open_in_memory().expect("child state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("retained worktree should exist");
	let sentinel_path = worktree.path.join("dirty.txt");

	fs::write(&sentinel_path, "uncommitted repair work\n")
		.expect("retained worktree should keep local edits");

	parent_store
		.configure_dispatch_slot_root(config.service_id(), config.worktree_root())
		.expect("parent dispatch-slot root should configure");
	child_store
		.configure_dispatch_slot_root(config.service_id(), config.worktree_root())
		.expect("child dispatch-slot root should configure");

	assert!(
		parent_store
			.try_acquire_lease(config.service_id(), &issue.id, "planned-run", "In Progress")
			.expect("parent should acquire the shared dispatch slot")
	);

	let child_issue_claim = parent_store
		.clone_issue_claim_for_child(&issue.id)
		.expect("child should inherit the shared issue-claim fd");
	let (child_guard, child_slot_index) = parent_store
		.clone_dispatch_slot_for_child(&issue.id)
		.expect("child should inherit the shared dispatch-slot fd");

	child_store
		.adopt_preacquired_lease(
			config.service_id(),
			&issue.id,
			"planned-run",
			"In Progress",
			PreacquiredLeaseGuards {
				issue_claim_fd: child_issue_claim.into_raw_fd(),
				dispatch_slot_fd: child_guard.into_raw_fd(),
				dispatch_slot_index: child_slot_index,
			},
		)
		.expect("child should adopt the inherited lease guard");

	let issue_run = orchestrator::prepare_issue_run(
		PrepareIssueRunContext {
			tracker: &tracker,
			project: &config,
			workflow: &workflow,
			state_store: &child_store,
			worktree_manager: &worktree_manager,
			dry_run: false,
			lease_preacquired: true,
			dispatch_mode: IssueDispatchMode::Normal,
			preferred_issue_state: None,
			preferred_initial_issue_state: None,
			preferred_run_identity: Some(PreferredRunIdentity {
				run_id: "planned-run",
				attempt_number: 1,
			}),
			preferred_retry_budget_base: None,
		},
		issue.clone(),
	)
	.expect("preacquired live runs should not require github token authority before review handoff")
	.expect("preacquired live runs should still prepare a run");

	assert_eq!(issue_run.run_id, "planned-run");
	assert_eq!(issue_run.attempt_number, 1);
	assert!(
		worktree.path.exists(),
		"reused retained worktrees should remain available for preacquired child runs"
	);
	assert!(
		sentinel_path.exists(),
		"prepare path must not discard retained local work for preacquired child runs"
	);
	assert!(
		child_store
			.lease_for_issue(&issue.id)
			.expect("child lease lookup should succeed")
			.expect("preacquired child lease should remain adopted")
			.run_id() == "planned-run",
		"preacquired child lease should remain adopted after planning"
	);
	assert!(
		child_store
			.latest_run_attempt_for_issue(&issue.id)
			.expect("run attempt lookup should work")
			.expect("starting attempt should record")
			.status() == "starting",
		"preacquired child planning should record a starting attempt"
	);
}

#[cfg(unix)]
#[test]
fn run_target_issue_once_skips_reconciliation_for_preacquired_child_runs() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue("Todo", &[]);
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![Vec::new(), Vec::new()]);
	let parent_store = StateStore::open_in_memory().expect("parent state store should open");
	let child_store = StateStore::open_in_memory().expect("child state store should open");

	parent_store
		.configure_dispatch_slot_root(config.service_id(), config.worktree_root())
		.expect("parent dispatch-slot root should configure");
	child_store
		.configure_dispatch_slot_root(config.service_id(), config.worktree_root())
		.expect("child dispatch-slot root should configure");

	assert!(
		parent_store
			.try_acquire_lease(config.service_id(), &issue.id, "planned-run", "In Progress")
			.expect("parent should acquire the shared dispatch slot")
	);

	let child_issue_claim = parent_store
		.clone_issue_claim_for_child(&issue.id)
		.expect("child should inherit the shared issue-claim fd");
	let (child_guard, child_slot_index) = parent_store
		.clone_dispatch_slot_for_child(&issue.id)
		.expect("child should inherit the shared dispatch-slot fd");

	child_store
		.record_run_attempt("planned-run", &issue.id, 1, "running")
		.expect("adopted run attempt should record");

	let summary = orchestrator::run_target_issue_once(TargetIssueRunContext {
		tracker: &tracker,
		project: &config,
		workflow: &workflow,
		state_store: &child_store,
		issue_id: &issue.id,
		preferred_issue_state: Some("In Progress"),
		preferred_initial_issue_state: None,
		dry_run: false,
		lease_preacquired: true,
		preferred_issue_claim_fd: Some(child_issue_claim.into_raw_fd()),
		preferred_dispatch_slot_fd: Some(child_guard.into_raw_fd()),
		preferred_dispatch_slot_index: Some(child_slot_index),
		dispatch_mode: IssueDispatchMode::Normal,
		preferred_run_identity: Some(PreferredRunIdentity {
			run_id: "planned-run",
			attempt_number: 1,
		}),
		preferred_retry_budget_base: None,
	})
	.expect("targeted child run should not error before refresh lookup");

	assert!(summary.is_none(), "missing refreshed issue should stop before execution");
	assert_eq!(
		child_store
			.lease_for_issue(&issue.id)
			.expect("child lease lookup should succeed")
			.expect("preacquired child lease should remain adopted")
			.run_id(),
		"planned-run"
	);
	assert_eq!(
		child_store
			.run_attempt("planned-run")
			.expect("run lookup should succeed")
			.expect("planned attempt should remain recorded")
			.status(),
		"running"
	);
}

#[test]
fn prepare_issue_run_rejects_stale_preferred_identity_after_attempt_advance() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue("Todo", &[]);
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());

	state_store
		.record_run_attempt("other-run", &issue.id, 1, "succeeded")
		.expect("existing run attempt should record");

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
			preferred_run_identity: Some(PreferredRunIdentity {
				run_id: "planned-run",
				attempt_number: 1,
			}),
			preferred_retry_budget_base: None,
		},
		issue.clone(),
	)
	.expect("stale targeted issue preparation should not error");

	assert!(issue_run.is_none(), "stale preferred identity should be rejected");
	assert!(state_store.lease_for_issue(&issue.id).expect("lease lookup should succeed").is_none());
}
