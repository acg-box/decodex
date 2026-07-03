use crate::{
	orchestrator::tests::{
		self,
		runtime_failure::{
			self, AgentGitCredentialsUnavailable, ChildRunRef, FakeTracker, IssueDispatchMode,
			IssueRunPlan, PrepareIssueRunContext, RUN_ACTIVITY_MARKER_FILE,
			RUN_OPERATION_RECONCILIATION, RUN_OPERATION_REPO_GATE, RepoGateFailureKind, Report,
			RetainedReviewRepairPushFailed, RunFailureWritebackDisposition, StateStore,
			TEST_SERVICE_ID, TempDir, TestEnvVarGuard, WorktreeManager, WorktreeSpec, fs,
			orchestrator, process, tracker,
		},
	},
	test_support,
};

#[test]
fn repo_gate_runtime_failures_require_manual_attention_without_retry_budget_wait() {
	let error = Report::new(orchestrator::RepoGateFailure::new(
		RepoGateFailureKind::CommandSpawnFailed,
		String::from(
			"Failed to spawn repo gate command `cargo make fmt` in `/tmp/repo` via `/bin/sh` `-c`: missing tool",
		),
	));
	let repo_gate_failure = error
		.downcast_ref::<orchestrator::RepoGateFailure>()
		.expect("repo gate failure should downcast");

	assert_eq!(
		repo_gate_failure.disposition(),
		orchestrator::RepoGateFailureDisposition::NeedsHumanAttention
	);
	assert_eq!(repo_gate_failure.error_class(), "repo_gate_command_spawn_failed");
}

#[test]
fn operation_marker_write_failures_do_not_abort_completion_flow() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let occupied_path = temp_dir.path().join("occupied");

	fs::write(&occupied_path, "not a directory").expect("blocking file should write");
	orchestrator::write_run_operation_marker_best_effort(
		&occupied_path,
		"run-1",
		1,
		RUN_OPERATION_REPO_GATE,
	);
	orchestrator::write_run_operation_marker_best_effort(
		&occupied_path,
		"run-1",
		1,
		RUN_OPERATION_RECONCILIATION,
	);

	assert!(occupied_path.is_file());
	assert!(!occupied_path.join(RUN_ACTIVITY_MARKER_FILE).exists());
}

#[test]
fn validate_review_handoff_runtime_requires_gh_and_github_token_authority() {
	let (_temp_dir, config, _workflow) = tests::temp_project_layout();

	{
		let _env_lock = TestEnvVarGuard::lock();
		let missing_env_var = format!("DECODEX_TEST_MISSING_GITHUB_TOKEN_ENV_{}", process::id());
		let config_missing_github =
			runtime_failure::service_config_with_github_token_env_var(&config, &missing_env_var);

		assert!(orchestrator::validate_review_handoff_runtime(&config, true).is_ok());
		assert!(orchestrator::validate_review_handoff_runtime(&config, false).is_ok());
		assert!(orchestrator::validate_daemon_runtime().is_ok());
		assert!(orchestrator::validate_command_available("git", None, "test preflight").is_ok());

		let error = orchestrator::validate_review_handoff_runtime(&config_missing_github, false)
			.expect_err("missing github token env-var should fail live preflight");

		assert!(error.to_string().contains("github.token_env_var"));
	}

	let env_var = format!("DECODEX_TEST_BLANK_GITHUB_TOKEN_ENV_{}", process::id());
	let _env_guard = TestEnvVarGuard::set(&env_var, "");
	let config_blank_github =
		runtime_failure::service_config_with_github_token_env_var(&config, &env_var);
	let error = orchestrator::validate_review_handoff_runtime(&config_blank_github, false)
		.expect_err("blank github token authority should fail live preflight");

	assert!(error.to_string().contains("must not be blank"));

	let error = orchestrator::validate_command_available(
		"__decodex_missing_command__",
		None,
		"PR-backed review handoff",
	)
	.expect_err("missing command should fail preflight");

	assert!(
		error.to_string().contains("Required command `__decodex_missing_command__` is unavailable")
	);
}

#[test]
fn retained_review_repair_completion_pushes_repaired_head_to_pr_branch() {
	let (temp_dir, base_config, _workflow) = tests::temp_project_layout();
	let env_var = format!("DECODEX_TEST_REPAIR_PUSH_GITHUB_TOKEN_ENV_{}", process::id());
	let _env_guard = TestEnvVarGuard::set(&env_var, "secret-token-value");
	let config = runtime_failure::service_config_with_github_token_env_var(&base_config, &env_var);
	let remote_root = temp_dir.path().join("origin.git");
	let issue = tests::sample_issue("In Review", &[]);
	let issue_run = runtime_failure::loop_guardrail_issue_run(&config, &issue, 2);

	runtime_failure::add_origin_remote(config.repo_root(), &remote_root);
	runtime_failure::checkout_new_branch(config.repo_root(), &issue_run.worktree.branch_name);

	let local_head = runtime_failure::commit_worktree_change(
		config.repo_root(),
		"repair.txt",
		"repair\n",
		r#"{"schema":"decodex/commit/1","summary":"Retain review repair","authority":"XY-1115"}"#,
	);

	orchestrator::push_retained_review_repair_head(
		&config,
		&issue_run,
		Some("https://github.com/hack-ink/decodex/pull/502"),
	)
	.expect("retained review-repair completion should push the repaired head");

	let output = test_support::hermetic_git_command()
		.arg("--git-dir")
		.arg(&remote_root)
		.args(["rev-parse", &format!("refs/heads/{}", issue_run.worktree.branch_name)])
		.output()
		.expect("remote head probe should run");

	assert!(
		output.status.success(),
		"remote retained repair branch should exist: {}",
		String::from_utf8_lossy(&output.stderr)
	);
	assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), local_head);
}

#[test]
fn retained_review_repair_push_failures_are_structured_terminal_attention() {
	let _env_lock = TestEnvVarGuard::lock();
	let (_temp_dir, base_config, _workflow) = tests::temp_project_layout();
	let missing_env_var = format!("DECODEX_TEST_MISSING_REPAIR_PUSH_TOKEN_ENV_{}", process::id());
	let config =
		runtime_failure::service_config_with_github_token_env_var(&base_config, &missing_env_var);
	let issue = tests::sample_issue("In Review", &[]);
	let issue_run = runtime_failure::loop_guardrail_issue_run(&config, &issue, 2);
	let error = orchestrator::push_retained_review_repair_head(
		&config,
		&issue_run,
		Some("https://github.com/hack-ink/decodex/pull/502"),
	)
	.expect_err("missing GitHub token should produce a typed push failure");
	let push_failure = error
		.downcast_ref::<RetainedReviewRepairPushFailed>()
		.expect("missing push authority should preserve typed error");
	let (error_class, next_action) = orchestrator::terminal_failure_comment_details(
		false,
		&error,
		"clear label `decodex:needs-attention`",
	);

	assert_eq!(push_failure.error_class(), "retained_review_repair_push_auth_failed");
	assert_eq!(error_class, "retained_review_repair_push_auth_failed");
	assert!(next_action.contains("repair GitHub authentication"));
	assert!(next_action.contains(&issue_run.worktree.branch_name));
	assert_eq!(
		orchestrator::run_failure_writeback_disposition(&error),
		RunFailureWritebackDisposition::TerminalAttention
	);
}

#[test]
fn agent_git_credentials_use_runtime_env_without_persisting_the_token() {
	let (_temp_dir, config, _workflow) = tests::temp_project_layout();
	let env_var = format!("DECODEX_TEST_AGENT_GITHUB_TOKEN_ENV_{}", process::id());
	let _env_guard = TestEnvVarGuard::set(&env_var, "secret-token-value");
	let config = runtime_failure::service_config_with_github_token_env_var(&config, &env_var);
	let credentials =
		orchestrator::prepare_agent_git_credentials(&config, "run/with spaces", config.repo_root())
			.expect("agent Git credentials should prepare");

	assert!(
		fs::read_dir(config.worktree_root())
			.expect("worktree root should list")
			.filter_map(std::result::Result::ok)
			.all(|entry| !entry.file_name().to_string_lossy().starts_with(".decodex-git-askpass-")),
		"agent Git credentials should not materialize askpass helper files"
	);

	let inherited_signing_key =
		runtime_failure::git_config_value(config.repo_root(), "user.signingkey", None);
	let agent_signing_key = runtime_failure::git_config_value(
		config.repo_root(),
		"user.signingkey",
		Some(&credentials),
	);

	assert_eq!(
		agent_signing_key, inherited_signing_key,
		"agent git environment should preserve inherited signing keys when the repo has no local key"
	);
	assert_eq!(
		runtime_failure::git_config_value(config.repo_root(), "commit.gpgsign", Some(&credentials))
			.as_deref(),
		Some("false")
	);

	let inherited_git_config_keys = runtime_failure::injected_git_config_keys(&credentials);

	assert!(
		!inherited_git_config_keys.iter().any(|key| key == "commit.gpgsign"),
		"agent git environment should not disable inherited commit signing"
	);
	assert!(
		!inherited_git_config_keys.iter().any(|key| key == "tag.gpgsign"),
		"agent git environment should not disable inherited tag signing"
	);
	assert!(
		!inherited_git_config_keys.iter().any(|key| key == "user.signingkey"),
		"agent git environment should not mask inherited signing keys"
	);

	let injected_git_config_values = runtime_failure::injected_git_config_values(&credentials);

	assert!(
		injected_git_config_values
			.iter()
			.any(|value| value.contains("github.com") && value.contains("x-access-token")),
		"agent git environment should inject an inline GitHub credential helper"
	);
	assert!(
		!injected_git_config_values.iter().any(|value| value.contains("secret-token-value")),
		"agent git config should not persist the GitHub token"
	);
}

#[test]
fn agent_git_credentials_pin_repo_local_signing_key_when_configured() {
	let (_temp_dir, config, _workflow) = tests::temp_project_layout();
	let env_var = format!("DECODEX_TEST_AGENT_SIGNING_GITHUB_TOKEN_ENV_{}", process::id());
	let _env_guard = TestEnvVarGuard::set(&env_var, "secret-token-value");
	let config = runtime_failure::service_config_with_github_token_env_var(&config, &env_var);

	runtime_failure::git_status_success(
		config.repo_root(),
		&["config", "user.signingkey", "route-y-signing-key"],
	);

	let credentials = orchestrator::prepare_agent_git_credentials(
		&config,
		"run-with-signing",
		config.repo_root(),
	)
	.expect("agent Git credentials should prepare");
	let mut signing_key_probe = test_support::hermetic_git_command();

	signing_key_probe.arg("-C").arg(config.repo_root()).args([
		"config",
		"--get",
		"user.signingkey",
	]);
	credentials.process_env().apply_to(&mut signing_key_probe).expect("agent env should apply");

	let output = signing_key_probe.output().expect("git signing key probe should run");

	assert!(output.status.success());
	assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "route-y-signing-key");
}

#[test]
fn missing_agent_git_credentials_stop_without_retry() {
	let _env_lock = TestEnvVarGuard::lock();
	let (_temp_dir, config, _workflow) = tests::temp_project_layout();
	let missing_env_var = format!("DECODEX_TEST_MISSING_AGENT_GITHUB_TOKEN_ENV_{}", process::id());
	let config =
		runtime_failure::service_config_with_github_token_env_var(&config, &missing_env_var);
	let error = match orchestrator::prepare_agent_git_credentials(
		&config,
		"run-missing-token",
		config.repo_root(),
	) {
		Ok(_) => panic!("missing github token should fail before app-server launch"),
		Err(error) => error,
	};
	let credentials_error = error
		.downcast_ref::<AgentGitCredentialsUnavailable>()
		.expect("credential preflight failure should be typed");
	let (error_class, next_action) = orchestrator::terminal_failure_comment_details(
		false,
		&error,
		"clear label `decodex:needs-attention`",
	);

	assert_eq!(credentials_error.token_env_var, missing_env_var);
	assert_eq!(error_class, "github_credentials_unavailable");
	assert!(next_action.contains("repair GitHub authentication"));
	assert!(!next_action.contains(&missing_env_var));
}

#[test]
fn live_run_without_candidate_does_not_require_github_token_authority() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let tracker = FakeTracker::with_refresh_snapshots_and_project(vec![], vec![vec![]], true);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let summary = orchestrator::run_project_once(&tracker, &config, &workflow, &state_store, false)
		.expect("empty backlog should not require github token authority");

	assert!(summary.is_none());
}

#[test]
fn prepare_issue_run_with_candidate_does_not_require_github_token_authority_before_agent_execution()
{
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let listed_issue = tests::sample_issue("Todo", &[]);
	let tracker = FakeTracker::with_refresh_snapshots(
		vec![listed_issue.clone()],
		vec![vec![listed_issue.clone()]],
	);
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
		listed_issue.clone(),
	)
	.expect("candidate dispatch should prepare without github token authority")
	.expect("candidate issue should plan a run");

	assert_eq!(issue_run.issue.id, listed_issue.id);
	assert_eq!(issue_run.issue_state, "In Progress");
	assert!(
		state_store.lease_for_issue(&listed_issue.id).expect("lease lookup should work").is_some()
	);
	assert!(
		state_store
			.worktree_for_issue(&listed_issue.id)
			.expect("worktree lookup should work")
			.is_some()
	);
	assert_eq!(
		state_store
			.latest_run_attempt_for_issue(&listed_issue.id)
			.expect("run attempt lookup should work")
			.expect("starting attempt should record")
			.status(),
		"starting"
	);
}

#[test]
fn execute_issue_run_clears_lease_when_active_label_setup_fails() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let mut listed_issue = tests::sample_issue("Todo", &[]);
	let mut refreshed_issue = listed_issue.clone();
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let worktree_path = config.worktree_root().join(&listed_issue.identifier);

	listed_issue.team.labels.retain(|label| label.name != active_label);
	refreshed_issue.team.labels.retain(|label| label.name != active_label);

	let tracker = FakeTracker::with_refresh_snapshots(
		vec![listed_issue.clone()],
		vec![vec![refreshed_issue]],
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue_run = IssueRunPlan {
		issue: listed_issue.clone(),
		issue_state: String::from("In Progress"),
		initial_issue_state: listed_issue.state.name.clone(),
		worktree: WorktreeSpec {
			branch_name: String::from("x/pubfi-pub-101"),
			issue_identifier: listed_issue.identifier.clone(),
			path: worktree_path.clone(),
			reused_existing: false,
		},
		retry_project_slug: listed_issue
			.project_slug
			.clone()
			.expect("sample issue should carry a project slug"),
		dispatch_mode: IssueDispatchMode::Normal,
		attempt_number: 1,
		run_id: String::from("pub-101-attempt-1-123"),
		retry_budget_base: 0,
	};

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");

	state_store
		.record_run_attempt(
			&issue_run.run_id,
			&listed_issue.id,
			issue_run.attempt_number,
			"starting",
		)
		.expect("run attempt should record");
	state_store
		.upsert_lease(config.service_id(), &listed_issue.id, &issue_run.run_id, "In Progress")
		.expect("lease should record");

	let error = orchestrator::execute_issue_run(
		&tracker,
		&config,
		&workflow,
		&state_store,
		issue_run.clone(),
	)
	.expect_err("active-label setup failure should abort execution");

	assert!(error.to_string().contains("required label"));
	assert!(
		state_store.lease_for_issue(&listed_issue.id).expect("lease lookup should work").is_none(),
		"active-label setup failures should still release the lease"
	);
	assert_eq!(
		state_store
			.run_attempt(&issue_run.run_id)
			.expect("run attempt lookup should work")
			.expect("run attempt should exist")
			.status(),
		"failed",
		"active-label setup failures should mark the run failed before returning"
	);
}

#[test]
fn reconciliation_clears_stale_leases_and_terminal_worktrees() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let issue = tests::sample_issue("Done", &[active_label.as_str()]);
	let queue_label = tracker::automation_queue_label(TEST_SERVICE_ID);
	let tracker =
		FakeTracker::new(vec![issue.clone()]).with_label_lookup_issues(&queue_label, vec![]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_path = config.worktree_root().join("PUB-101");

	state_store
		.record_run_attempt("run-1", &issue.id, 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease("pubfi", &issue.id, "run-1", "In Progress")
		.expect("lease should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree mapping should record");

	let summary = orchestrator::run_project_once(&tracker, &config, &workflow, &state_store, false)
		.expect("reconciliation should succeed");

	assert!(summary.is_none());
	assert!(state_store.lease_for_issue(&issue.id).expect("lease lookup should work").is_none());
	assert!(
		state_store.worktree_for_issue(&issue.id).expect("worktree lookup should work").is_none()
	);
	assert_eq!(
		state_store
			.run_attempt("run-1")
			.expect("run attempt lookup should work")
			.expect("run attempt should exist")
			.status(),
		"terminated"
	);
	assert_eq!(
		tracker.label_removals.borrow().as_slice(),
		[
			(issue.id.clone(), vec![String::from("label-active")]),
			(issue.id.clone(), vec![String::from("label-queued")]),
		]
	);
}

#[test]
fn reconciliation_runs_without_project_validation() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let issue = tests::sample_issue("Done", &[active_label.as_str()]);
	let queue_label = tracker::automation_queue_label(TEST_SERVICE_ID);
	let tracker = FakeTracker::with_refresh_snapshots_and_project(
		vec![issue.clone()],
		vec![vec![issue.clone()]],
		false,
	)
	.with_label_lookup_issues(&queue_label, vec![]);
	let state_store = StateStore::open_in_memory().expect("state store should open");

	state_store
		.record_run_attempt("run-1", &issue.id, 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease("pubfi", &issue.id, "run-1", "In Progress")
		.expect("lease should record");

	let summary = orchestrator::run_project_once(&tracker, &config, &workflow, &state_store, false)
		.expect("reconciliation should still succeed without any project validation");

	assert!(summary.is_none(), "reconciliation-only startup should not dispatch a new lane here");
	assert!(state_store.lease_for_issue(&issue.id).expect("lease lookup should work").is_none());
	assert_eq!(
		state_store
			.run_attempt("run-1")
			.expect("run attempt lookup should work")
			.expect("run attempt should exist")
			.status(),
		"terminated"
	);
	assert_eq!(
		tracker.label_removals.borrow().as_slice(),
		[
			(issue.id.clone(), vec![String::from("label-active")]),
			(issue.id.clone(), vec![String::from("label-queued")]),
		]
	);
}

#[test]
fn exited_child_cleanup_updates_status_and_retry_budget_by_interrupt_flag() {
	for (case_name, mark_interrupted, expected_status, expected_retry_budget) in
		[("clean exit", false, "running", 0), ("interrupted exit", true, "interrupted", 1)]
	{
		let state_store = StateStore::open_in_memory().expect("state store should open");
		let issue = tests::sample_issue("In Progress", &[]);

		state_store
			.record_run_attempt("run-1", &issue.id, 1, "running")
			.expect("run attempt should record");
		state_store
			.upsert_lease("pubfi", &issue.id, "run-1", "In Progress")
			.expect("lease should record");

		orchestrator::clear_orphaned_daemon_child_state(
			&state_store,
			ChildRunRef { issue_id: &issue.id, run_id: "run-1", attempt_number: 1 },
			mark_interrupted,
		)
		.expect(case_name);

		assert!(
			state_store.lease_for_issue(&issue.id).expect("lease lookup should succeed").is_none()
		);
		assert_eq!(
			state_store
				.run_attempt("run-1")
				.expect("run attempt lookup should succeed")
				.expect("run attempt should exist")
				.status(),
			expected_status,
			"{case_name}"
		);
		assert_eq!(
			state_store
				.retry_budget_attempt_count(&issue.id)
				.expect("retry budget count should succeed"),
			expected_retry_budget,
			"{case_name}"
		);
	}
}

#[test]
fn exited_child_cleanup_handles_worktree_mapping_ownership() {
	{
		let temp_dir = TempDir::new().expect("tempdir should create");
		let state_store = StateStore::open_in_memory().expect("state store should open");
		let issue = tests::sample_issue("Done", &[]);
		let removed_worktree_path = temp_dir.path().join("removed-lane");

		state_store
			.record_run_attempt("run-1", &issue.id, 1, "running")
			.expect("run attempt should record");
		state_store.update_run_status("run-1", "succeeded").expect("run status should update");
		state_store
			.upsert_lease("pubfi", &issue.id, "run-1", "In Progress")
			.expect("lease should record");
		state_store
			.upsert_worktree(
				"pubfi",
				&issue.id,
				"x/pubfi-pub-101",
				&removed_worktree_path.display().to_string(),
			)
			.expect("worktree mapping should record");

		orchestrator::clear_orphaned_daemon_child_state(
			&state_store,
			ChildRunRef { issue_id: &issue.id, run_id: "run-1", attempt_number: 1 },
			false,
		)
		.expect("removed worktree cleanup should succeed");

		assert!(
			state_store.lease_for_issue(&issue.id).expect("lease lookup should succeed").is_none()
		);
		assert!(
			state_store
				.worktree_for_issue(&issue.id)
				.expect("worktree lookup should succeed")
				.is_none()
		);
		assert_eq!(
			state_store
				.run_attempt("run-1")
				.expect("run attempt lookup should succeed")
				.expect("run attempt should exist")
				.status(),
			"succeeded"
		);
	}
	{
		let temp_dir = TempDir::new().expect("tempdir should create");
		let state_store = StateStore::open_in_memory().expect("state store should open");
		let issue = tests::sample_issue("In Review", &[]);
		let existing_worktree_path = temp_dir.path().join("retained-lane");

		fs::create_dir_all(&existing_worktree_path).expect("worktree path should exist");

		state_store
			.record_run_attempt("run-1", &issue.id, 1, "running")
			.expect("run attempt should record");
		state_store
			.upsert_lease("pubfi", &issue.id, "run-1", "In Progress")
			.expect("lease should record");
		state_store
			.upsert_worktree(
				"pubfi",
				&issue.id,
				"x/pubfi-pub-101",
				&existing_worktree_path.display().to_string(),
			)
			.expect("worktree mapping should record");

		orchestrator::clear_orphaned_daemon_child_state(
			&state_store,
			ChildRunRef { issue_id: &issue.id, run_id: "run-1", attempt_number: 1 },
			false,
		)
		.expect("existing worktree cleanup should succeed");

		assert_eq!(
			state_store
				.worktree_for_issue(&issue.id)
				.expect("worktree lookup should succeed")
				.expect("worktree mapping should remain")
				.worktree_path(),
			existing_worktree_path.as_path()
		);
	}
}

#[test]
fn exited_child_cleanup_requires_exact_run_id() {
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue("In Progress", &[]);

	state_store
		.record_run_attempt("other-run", &issue.id, 1, "running")
		.expect("other run attempt should record");
	state_store
		.upsert_lease("pubfi", &issue.id, "other-run", "In Progress")
		.expect("lease should record");

	orchestrator::clear_orphaned_daemon_child_state(
		&state_store,
		ChildRunRef { issue_id: &issue.id, run_id: "planned-run", attempt_number: 1 },
		true,
	)
	.expect("orphaned child cleanup should succeed");

	assert_eq!(
		state_store
			.lease_for_issue(&issue.id)
			.expect("lease lookup should succeed")
			.expect("lease should remain attached to the other run")
			.run_id(),
		"other-run"
	);
	assert_eq!(
		state_store
			.run_attempt("other-run")
			.expect("run attempt lookup should succeed")
			.expect("run attempt should exist")
			.status(),
		"running"
	);
}

#[test]
fn exited_child_cleanup_keeps_other_run_lease_and_worktree_mapping() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue("In Progress", &[]);
	let removed_worktree_path = temp_dir.path().join("removed-lane");

	state_store
		.record_run_attempt("run-1", &issue.id, 1, "running")
		.expect("run attempt should record");
	state_store
		.record_run_attempt("other-run", &issue.id, 2, "running")
		.expect("other run attempt should record");
	state_store
		.upsert_lease("pubfi", &issue.id, "other-run", "In Progress")
		.expect("lease should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&removed_worktree_path.display().to_string(),
		)
		.expect("worktree mapping should record");

	orchestrator::clear_orphaned_daemon_child_state(
		&state_store,
		ChildRunRef { issue_id: &issue.id, run_id: "run-1", attempt_number: 1 },
		false,
	)
	.expect("orphaned child cleanup should succeed");

	assert_eq!(
		state_store
			.lease_for_issue(&issue.id)
			.expect("lease lookup should succeed")
			.expect("lease should remain attached to the other run")
			.run_id(),
		"other-run"
	);
	assert_eq!(
		state_store
			.worktree_for_issue(&issue.id)
			.expect("worktree lookup should succeed")
			.expect("worktree mapping should remain")
			.worktree_path(),
		removed_worktree_path.as_path()
	);
}
