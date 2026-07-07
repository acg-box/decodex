mod targeted_program_dispatch_tests {
	#[cfg(unix)]
	use std::os::unix::fs::PermissionsExt;
	use std::{env, fs};

	use crate::{
		execution_program::{
			ExecutionConflictDomain, ExecutionConflictDomainKind, ExecutionLinearIssueMapping,
			ExecutionProgram, ExecutionProgramNode, ExecutionProgramNodeStage,
			ExecutionQueueIntent,
		},
		orchestrator::{self, IssueDispatchMode, TargetIssueRunContext, tests, tests::FakeTracker},
		state::StateStore,
		test_support::TestEnvVarGuard,
	};

	#[test]
	fn targeted_identifier_dispatch_accepts_status_ready_program_node() {
		let (_temp_dir, config, workflow) = tests::temp_project_layout();
		let state_store = StateStore::open_in_memory().expect("state store should open");
		let issue = tests::sample_issue_with_sort_fields(
			"issue-program-ready",
			"PUB-1094",
			"Todo",
			&[],
			Some(1),
			"2026-06-23T04:16:17.133Z",
		);
		let mapping =
			ExecutionLinearIssueMapping::new(&issue.id, &issue.identifier, &issue.state.name)
				.expect("program issue mapping should build");
		let node = ExecutionProgramNode::new(
			"node-program-ready",
			ExecutionProgramNodeStage::Runtime,
			"Resolve a dispatchable Program Intake node.",
			ExecutionQueueIntent::ReadyToQueue,
		)
		.expect("program node should build")
		.with_acceptance_expectations(["Program node maps to a normal Linear issue."])
		.expect("acceptance should attach")
		.with_validation_expectations(["Run focused Program dispatch validation."])
		.expect("validation should attach")
		.with_linear_issue(mapping)
		.expect("issue mapping should attach");
		let program = ExecutionProgram::from_issue_batch_intake(
			"program-targeted-run",
			config.service_id(),
			"program-targeted-run-fingerprint",
			"Targeted Program run bridge.",
			vec![node],
		)
		.expect("program should build");

		state_store
			.upsert_execution_program(config.service_id(), program)
			.expect("program should persist");

		let tracker = FakeTracker::new(vec![issue.clone()]);
		let snapshot = orchestrator::build_live_operator_status_snapshot(
			&tracker,
			&config,
			&workflow,
			&state_store,
			10,
		)
		.expect("status snapshot should build");
		let program = snapshot
			.execution_programs
			.iter()
			.find(|program| program.program_id == "program-targeted-run")
			.expect("program should appear in status");

		assert_eq!(program.dispatchable_count, 1);
		assert_eq!(program.node_readbacks[0].dispatch_action.as_deref(), Some("dispatch"));

		let summary =
			orchestrator::run_target_issue_once_with_inferred_dispatch(TargetIssueRunContext {
				tracker: &tracker,
				project: &config,
				workflow: &workflow,
				state_store: &state_store,
				issue_id: &issue.identifier,
				preferred_issue_state: None,
				preferred_initial_issue_state: None,
				dry_run: true,
				lease_preacquired: false,
				preferred_issue_claim_fd: None,
				preferred_dispatch_slot_fd: None,
				preferred_dispatch_slot_index: None,
				dispatch_mode: IssueDispatchMode::Normal,
				preferred_run_identity: None,
				preferred_retry_budget_base: None,
			})
			.expect("targeted identifier run should succeed")
			.expect("status-ready program issue should dispatch by identifier");

		assert_eq!(summary.issue_id, issue.id);
		assert_eq!(summary.issue_identifier, issue.identifier);
		assert_eq!(summary.dispatch_mode, IssueDispatchMode::Program);
	}

	#[test]
	fn targeted_program_dispatch_records_selection_before_execution_failure() {
		let (temp_dir, config, workflow) = tests::temp_project_layout();
		let fake_bin_dir = temp_dir.path().join("fake-empty-bin");

		fs::create_dir_all(&fake_bin_dir).expect("fake bin dir should exist");

		let fake_codex_path = fake_bin_dir.join("codex");

		fs::write(&fake_codex_path, "#!/bin/sh\nexit 42\n").expect("fake codex should write");

		#[cfg(unix)]
		{
			let mut permissions = fs::metadata(&fake_codex_path)
				.expect("fake codex metadata should read")
				.permissions();

			permissions.set_mode(0o755);

			fs::set_permissions(&fake_codex_path, permissions)
				.expect("fake codex should become executable");
		}

		let path_env = env::var("PATH").unwrap_or_default();
		let _path_guard =
			TestEnvVarGuard::set("PATH", &format!("{}:{path_env}", fake_bin_dir.display()));
		let state_store = StateStore::open_in_memory().expect("state store should open");
		let issue = tests::sample_issue_with_sort_fields(
			"issue-program-event-before-failure",
			"PUB-1096",
			"Todo",
			&[],
			Some(1),
			"2026-06-23T04:16:17.133Z",
		);
		let mapping =
			ExecutionLinearIssueMapping::new(&issue.id, &issue.identifier, &issue.state.name)
				.expect("program issue mapping should build");
		let node = ExecutionProgramNode::new(
			"node-program-event-before-failure",
			ExecutionProgramNodeStage::Runtime,
			"Resolve a Program Intake node before execution starts.",
			ExecutionQueueIntent::ReadyToQueue,
		)
		.expect("program node should build")
		.with_acceptance_expectations(["Program node maps to a normal Linear issue."])
		.expect("acceptance should attach")
		.with_validation_expectations(["Run focused Program dispatch validation."])
		.expect("validation should attach")
		.with_linear_issue(mapping)
		.expect("issue mapping should attach");
		let program = ExecutionProgram::from_issue_batch_intake(
			"program-targeted-event-before-failure",
			config.service_id(),
			"program-targeted-event-before-failure-fingerprint",
			"Targeted Program event provenance.",
			vec![node],
		)
		.expect("program should build");

		state_store
			.upsert_execution_program(config.service_id(), program)
			.expect("program should persist");

		let tracker = FakeTracker::new(vec![issue.clone()]);
		let result =
			orchestrator::run_target_issue_once_with_inferred_dispatch(TargetIssueRunContext {
				tracker: &tracker,
				project: &config,
				workflow: &workflow,
				state_store: &state_store,
				issue_id: &issue.identifier,
				preferred_issue_state: None,
				preferred_initial_issue_state: None,
				dry_run: false,
				lease_preacquired: false,
				preferred_issue_claim_fd: None,
				preferred_dispatch_slot_fd: None,
				preferred_dispatch_slot_index: None,
				dispatch_mode: IssueDispatchMode::Normal,
				preferred_run_identity: None,
				preferred_retry_budget_base: None,
			});

		assert!(result.is_err());

		let events = state_store
			.list_private_execution_events_for_issue(config.service_id(), &issue.id)
			.expect("private events should be readable");
		let event = events
			.iter()
			.find(|event| event.event_type() == "program_dispatch_selected")
			.expect("program dispatch selection should be recorded before execution failure");

		assert_eq!(event.attempt_number(), 1);
		assert_eq!(event.payload()["issue"]["identifier"], "PUB-1096");
		assert_eq!(event.payload()["run"]["dispatch_mode"], "program");
		assert_eq!(
			event.payload()["execution_program"]["program_id"],
			"program-targeted-event-before-failure"
		);
		assert_eq!(
			event.payload()["execution_program"]["node_id"],
			"node-program-event-before-failure"
		);
	}

	#[test]
	fn targeted_program_selection_reconciles_stale_worktree_mapping_before_dispatch() {
		let (_temp_dir, config, workflow) = tests::temp_project_layout();
		let state_store = StateStore::open_in_memory().expect("state store should open");
		let issue = tests::sample_issue_with_sort_fields(
			"issue-program-stale-mapping",
			"PUB-1095",
			"Todo",
			&[],
			Some(1),
			"2026-06-23T04:16:17.133Z",
		);
		let missing_worktree_path = config.worktree_root().join(&issue.identifier);
		let conflict = ExecutionConflictDomain::new(ExecutionConflictDomainKind::Module, "runtime")
			.expect("conflict should build");
		let mapping =
			ExecutionLinearIssueMapping::new(&issue.id, &issue.identifier, &issue.state.name)
				.expect("program issue mapping should build");
		let node = ExecutionProgramNode::new(
			"node-program-stale-mapping",
			ExecutionProgramNodeStage::Runtime,
			"Resolve a dispatchable Program node after stale worktree cleanup.",
			ExecutionQueueIntent::ReadyToQueue,
		)
		.expect("program node should build")
		.with_acceptance_expectations(["Program node maps to a normal Linear issue."])
		.expect("acceptance should attach")
		.with_validation_expectations(["Run focused Program dispatch validation."])
		.expect("validation should attach")
		.with_conflict_domains([conflict])
		.expect("conflict should attach")
		.with_linear_issue(mapping)
		.expect("issue mapping should attach");
		let program = ExecutionProgram::from_issue_batch_intake(
			"program-targeted-stale-mapping",
			config.service_id(),
			"program-targeted-stale-mapping-fingerprint",
			"Targeted Program run bridge with stale mapping.",
			vec![node],
		)
		.expect("program should build");

		state_store
			.upsert_worktree(
				config.service_id(),
				&issue.id,
				"x/pubfi-pub-1095",
				&missing_worktree_path.display().to_string(),
			)
			.expect("stale worktree mapping should persist");
		state_store
			.upsert_execution_program(config.service_id(), program)
			.expect("program should persist");

		let tracker = FakeTracker::new(vec![issue.clone()]);
		let blocked = orchestrator::select_execution_program_run_candidate_with_summary(
			&tracker,
			&config,
			&workflow,
			&state_store,
			&[],
		)
		.expect("stale mapping should evaluate");

		assert!(blocked.selected.is_none());
		assert_eq!(blocked.summary.dispatchable_nodes, 0);

		let candidate =
			orchestrator::select_target_status_visible_program_candidate(&TargetIssueRunContext {
				tracker: &tracker,
				project: &config,
				workflow: &workflow,
				state_store: &state_store,
				issue_id: &issue.identifier,
				preferred_issue_state: None,
				preferred_initial_issue_state: None,
				dry_run: false,
				lease_preacquired: false,
				preferred_issue_claim_fd: None,
				preferred_dispatch_slot_fd: None,
				preferred_dispatch_slot_index: None,
				dispatch_mode: IssueDispatchMode::Program,
				preferred_run_identity: None,
				preferred_retry_budget_base: None,
			})
			.expect("targeted Program selection should reconcile")
			.expect("targeted Program issue should select after stale mapping cleanup");

		assert_eq!(candidate.issue.id, issue.id);
		assert_eq!(candidate.dispatch_mode, IssueDispatchMode::Program);
		assert!(
			state_store
				.worktree_for_issue(&issue.id)
				.expect("worktree lookup should succeed")
				.is_none()
		);
	}
}
