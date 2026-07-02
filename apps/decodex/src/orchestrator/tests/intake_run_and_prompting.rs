mod targeted_program_dispatch_tests {
	use crate::{
		execution_program::{
			ExecutionConflictDomain, ExecutionConflictDomainKind, ExecutionLinearIssueMapping,
			ExecutionProgram, ExecutionProgramNode, ExecutionProgramNodeStage,
			ExecutionQueueIntent,
		},
		orchestrator::{self, IssueDispatchMode, TargetIssueRunContext, tests, tests::FakeTracker},
		state::StateStore,
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

use std::path::{Path, PathBuf};

use crate::{
	agent::{DynamicToolHandler, ReviewExecutionMode, ReviewHandoffContext, TrackerToolBridge},
	config::{ReviewLevel, ServiceConfig},
	orchestrator::{
		self, ISSUE_DELIVERY_CLOSEOUT_COMPLETE_TOOL_NAME, ISSUE_LABEL_ADD_TOOL_NAME,
		ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME, ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
		ISSUE_REVIEW_HANDOFF_TOOL_NAME, ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME,
		ISSUE_TERMINAL_FINALIZE_TOOL_NAME, ISSUE_TRANSITION_TOOL_NAME, IssueDispatchMode,
		IssueTurnContinuationGuard, ReviewHandoffMarker, ReviewOrchestrationMarker, RunSummary,
		TargetIssueRunContext, TurnContinuationGuard,
		tests::{
			self, FakePullRequestReviewStateInspector, FakeTracker, TEST_SERVICE_ID,
			intake_workflow_reload, recovery_terminal_support,
		},
	},
	state::{self, StateStore},
	tracker::{self, TrackerIssue},
	workflow::WorkflowDocument,
	worktree::{WorktreeManager, WorktreeSpec},
};

struct PromptSurfaces {
	developer_instructions: String,
	user_input: String,
	continuation_input: String,
}
impl PromptSurfaces {
	fn all(&self) -> [&str; 3] {
		[
			self.developer_instructions.as_str(),
			self.user_input.as_str(),
			self.continuation_input.as_str(),
		]
	}
}

fn assert_manual_attention_prompt_guidance(prompt: &str, expects_handoff_guard: bool) {
	assert!(prompt.contains(&format!(
		"request label `decodex:needs-attention` with `{ISSUE_LABEL_ADD_TOOL_NAME}`"
	)));
	assert!(prompt.contains("records manual-attention label intent only"));
	assert!(prompt.contains(
		"Decodex applies the actual label only after that manual_attention comment validates"
	));
	assert!(prompt.contains("do not use runtime-owned retry/repair classes"));
	assert!(prompt.contains("app-server timeout, transport, turn, dynamic-tool, or usage-limit"));
	assert!(prompt.contains("stalled-run detection"));
	assert!(prompt.contains("phase-goal terminal-path misses"));
	assert!(prompt.contains(
		"repo-gate canonicalize, verify, baseline, tracked-rewrite, or git-lock failures"
	));
	assert!(prompt.contains("generic retryable execution failures"));
	assert!(!prompt.contains("add label `decodex:needs-attention`"));
	assert!(!prompt.contains("add the needs-attention label"));

	if expects_handoff_guard {
		assert!(
			prompt
				.contains(&format!("Do not call `{ISSUE_REVIEW_HANDOFF_TOOL_NAME}` in that case"))
		);
	}
}

fn assert_review_route_prompt_guidance(prompt: &str) {
	assert!(prompt.contains("Adjudicate every reviewer signal into `finding_routes`"));
	assert!(prompt.contains("must route to `current_blocker`"));
	assert!(prompt.contains("structured route evidence before any repair loop uses the signal"));
	assert!(prompt.contains("Non-current `finding_routes`"));
	assert!(prompt.contains("must not drive repair churn"));
}

fn assert_review_repair_developer_prompt(prompt: &str) {
	assert!(prompt.contains(ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME));
	assert!(prompt.contains(ISSUE_REVIEW_CHECKPOINT_TOOL_NAME));
	assert!(prompt.contains("Do not move the issue back to `In Progress`"));
	assert!(prompt.contains("do not call `issue_review_handoff`"));
	assert!(prompt.contains(
		"Decodex Review: request an independent fresh-context read-only verification pass"
	));
	assert!(prompt.contains("review_type = \"repair_verification\""));
	assert!(prompt.contains("registered project workflow policy"));
	assert!(prompt.contains("structured accepted/rejected findings"));

	assert_review_route_prompt_guidance(prompt);

	assert!(prompt.contains(
		"including non-thread review summaries, validate the claim against the codebase, tests, and requirements"
	));
	assert!(prompt.contains(
		"After the repaired head is pushed, reply in-thread for every addressed comment"
	));
	assert!(prompt.contains("retained landing fallback"));
	assert!(prompt.contains("Do not merge or land the PR yourself"));
}

fn assert_review_repair_user_prompt(prompt: &str, pr_url: &str) {
	assert!(prompt.contains(pr_url));
	assert!(prompt.contains(ISSUE_REVIEW_CHECKPOINT_TOOL_NAME));
	assert!(prompt.contains(
		"Decodex Review: request an independent fresh-context read-only verification pass"
	));
	assert!(prompt.contains("review_contract"));
	assert!(prompt.contains("structured accepted/rejected findings"));

	assert_review_route_prompt_guidance(prompt);

	assert!(prompt.contains(
		"Read the current review feedback on `https://github.com/hack-ink/decodex/pull/77`, including non-thread review summaries"
	));
	assert!(
		prompt.contains(
			"validate each actionable claim against the codebase, tests, and requirements"
		)
	);
	assert!(prompt.contains("Leave pushback or clarification threads open"));
	assert!(prompt.contains("because retained landing was not a deterministic clean path"));
	assert!(prompt.contains("Do not merge or land the PR yourself"));
	assert!(prompt.contains(
		"resolve only the GitHub review threads whose fixes landed and verified on the repaired head"
	));
}

fn assert_review_repair_continuation_prompt(prompt: &str) {
	assert!(prompt.contains(ISSUE_REVIEW_CHECKPOINT_TOOL_NAME));
	assert!(prompt.contains("Resume by committing any review-blocking repair edits"));
	assert!(prompt.contains("review_type = \"repair_verification\""));
	assert!(prompt.contains("structured accepted/rejected findings"));

	assert_review_route_prompt_guidance(prompt);

	assert!(prompt.contains(
		"Validate each actionable review claim against the codebase, tests, and requirements before changing code"
	));
	assert!(
		prompt.contains(
			"keep pushback or clarification threads open until the repaired head is ready"
		)
	);
	assert!(prompt.contains("retained landing fallback"));
	assert!(prompt.contains("do not merge or land the PR yourself"));
	assert!(prompt.contains("Do not request GitHub Review yourself"));
	assert!(prompt.contains("In Review"));
	assert!(prompt.contains("review_repair"));
}

fn run_and_prompting_service_owned_issue(state_name: &str) -> TrackerIssue {
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);

	tests::sample_issue(state_name, &[active_label.as_str()])
}

fn run_and_prompting_target_context<'a, T>(
	tracker: &'a T,
	config: &'a ServiceConfig,
	workflow: &'a WorkflowDocument,
	state_store: &'a StateStore,
	issue_identifier: &'a str,
	dispatch_mode: IssueDispatchMode,
) -> TargetIssueRunContext<'a, T> {
	TargetIssueRunContext {
		tracker,
		project: config,
		workflow,
		state_store,
		issue_id: issue_identifier,
		preferred_issue_state: None,
		preferred_initial_issue_state: None,
		dry_run: true,
		lease_preacquired: false,
		preferred_issue_claim_fd: None,
		preferred_dispatch_slot_fd: None,
		preferred_dispatch_slot_index: None,
		dispatch_mode,
		preferred_run_identity: None,
		preferred_retry_budget_base: None,
	}
}

fn assert_prompt_orders_thread_replies_after_push(prompt: &str, push_requirement: &str) {
	let push_index =
		prompt.find(push_requirement).expect("prompt should require push before thread resolution");
	let reply_index = prompt
		.find("After the repaired head is pushed, reply in-thread for every addressed comment")
		.expect("prompt should place thread replies after push");

	assert!(push_index < reply_index);
}

fn build_normal_prompt_surfaces(
	config: &ServiceConfig,
	workflow: &WorkflowDocument,
) -> PromptSurfaces {
	let issue = tests::sample_issue("Todo", &[]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let issue_run = orchestrator::IssueRunPlan {
		issue: issue.clone(),
		issue_state: String::from("In Progress"),
		initial_issue_state: String::from("Todo"),
		worktree: WorktreeSpec {
			branch_name: String::from("x/pubfi-pub-101"),
			issue_identifier: String::from("PUB-101"),
			path: config.worktree_root().join("PUB-101"),
			reused_existing: false,
		},
		retry_project_slug: String::from("pubfi"),
		dispatch_mode: IssueDispatchMode::Normal,
		attempt_number: 1,
		run_id: String::from("pub-101-attempt-1-123"),
		retry_budget_base: 0,
	};
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let developer_instructions = orchestrator::build_developer_instructions(
		&tracker,
		config,
		workflow,
		&issue_run,
		&state_store,
		None,
	)
	.expect("developer instructions should build");
	let user_input = orchestrator::build_user_input(
		&tracker,
		config,
		&issue,
		workflow,
		&issue_run,
		&state_store,
		None,
	);
	let continuation_input = orchestrator::build_continuation_user_input(
		&issue,
		workflow,
		IssueDispatchMode::Normal,
		None,
		workflow.frontmatter().tracker().success_state(),
		config.codex().review_level(),
	);

	PromptSurfaces { developer_instructions, user_input, continuation_input }
}

#[test]
fn dry_run_selects_one_issue_and_plans_worktree() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let tracker = FakeTracker::new(vec![tests::sample_issue("Todo", &[])]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let summary = orchestrator::run_project_once(&tracker, &config, &workflow, &state_store, true)
		.expect("run once should succeed")
		.expect("one issue should be selected");

	assert_eq!(
		summary,
		RunSummary {
			project_id: String::from("pubfi"),
			issue_id: String::from("issue-1"),
			issue_identifier: String::from("PUB-101"),
			issue_state: String::from("In Progress"),
			initial_issue_state: String::from("Todo"),
			retry_project_slug: String::new(),
			dispatch_mode: orchestrator::IssueDispatchMode::Normal,
			branch_name: String::from("x/pubfi-pub-101"),
			worktree_path: Path::new(&config.worktree_root().join("PUB-101")).to_path_buf(),
			attempt_number: 1,
			run_id: summary.run_id.clone(),
			continuation_pending: false,
		}
	);
	assert!(tracker.comments.borrow().is_empty());
}

#[test]
fn targeted_identifier_dispatch_accepts_status_ready_queued_issue() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue_with_sort_fields(
		"issue-ready",
		"PUB-101",
		"Todo",
		&[],
		Some(1),
		"2026-03-13T04:16:17.133Z",
	);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("status snapshot should build");
	let candidate = snapshot
		.queued_candidates
		.iter()
		.find(|candidate| candidate.issue_identifier == issue.identifier)
		.expect("queued issue should appear in status");

	assert_eq!(candidate.classification, "ready");
	assert_eq!(candidate.reason, "eligible_for_dispatch");

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
		.expect("status-ready queued issue should dispatch by identifier");

	assert_eq!(summary.issue_id, issue.id);
	assert_eq!(summary.issue_identifier, issue.identifier);
	assert_eq!(summary.dispatch_mode, IssueDispatchMode::Normal);
}

#[test]
fn targeted_inferred_dispatch_keeps_retry_for_active_issue() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = run_and_prompting_service_owned_issue("In Progress");
	let tracker = FakeTracker::new(vec![issue.clone()]);
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
		.expect("targeted active identifier run should succeed")
		.expect("active target should fall back to retry dispatch");

	assert_eq!(summary.issue_id, issue.id);
	assert_eq!(summary.issue_identifier, issue.identifier);
	assert_eq!(summary.dispatch_mode, IssueDispatchMode::Retry);
}

#[test]
fn targeted_identifier_dispatch_accepts_status_visible_retained_closeout_lane() {
	let (temp_dir, base_config, workflow) = tests::temp_project_layout();
	let config = tests::service_config_with_github_token_env_var(&base_config, "HOME");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = run_and_prompting_service_owned_issue("In Review");
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree =
		worktree_manager.ensure_worktree(&issue.identifier, false).expect("worktree should exist");
	let head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/181";

	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree should record");

	tests::seed_review_handoff_marker_for_path(
		&state_store,
		config.service_id(),
		&worktree.path,
		&tests::sample_review_handoff_marker(&worktree.branch_name, pr_url, &head_oid),
	);

	let _path_guard = recovery_terminal_support::install_fake_merged_pr_gh_response(
		&temp_dir, &worktree, pr_url, &head_oid,
	);
	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("status snapshot should build");
	let lane = snapshot
		.post_review_lanes
		.iter()
		.find(|lane| lane.issue_identifier == issue.identifier)
		.expect("retained closeout lane should appear in status");

	assert_eq!(lane.classification, "continue");
	assert_eq!(lane.reason, "pull_request_merged_closeout_pending");
	assert_eq!(lane.pr_state.as_deref(), Some("MERGED"));

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
		.expect("targeted retained closeout identifier run should succeed")
		.expect("status-visible retained closeout lane should dispatch by identifier");

	assert_eq!(summary.issue_id, issue.id);
	assert_eq!(summary.issue_identifier, issue.identifier);
	assert_eq!(summary.dispatch_mode, IssueDispatchMode::Closeout);
	assert_eq!(summary.run_id, "run-1");
	assert_eq!(summary.attempt_number, 1);
}

#[test]
fn targeted_identifier_dispatch_accepts_status_visible_review_repair_lane() {
	let (temp_dir, base_config, workflow) = tests::temp_project_layout();
	let config = tests::service_config_with_github_token_env_var(&base_config, "HOME");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = run_and_prompting_service_owned_issue("In Review");
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree =
		worktree_manager.ensure_worktree(&issue.identifier, false).expect("worktree should exist");
	let head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/184";

	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree should record");

	tests::seed_review_handoff_marker_for_path(
		&state_store,
		config.service_id(),
		&worktree.path,
		&tests::sample_review_handoff_marker(&worktree.branch_name, pr_url, &head_oid),
	);

	let _path_guard = recovery_terminal_support::install_fake_conflicting_pr_gh_response(
		&temp_dir, &worktree, pr_url, &head_oid,
	);
	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("status snapshot should build");
	let lane = snapshot
		.post_review_lanes
		.iter()
		.find(|lane| lane.issue_identifier == issue.identifier)
		.expect("retained repair lane should appear in status");

	assert_eq!(lane.classification, "needs_review_repair");
	assert_eq!(lane.reason, "pull_request_merge_conflict");

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
		.expect("targeted retained repair identifier run should succeed")
		.expect("status-visible retained repair lane should dispatch by identifier");

	assert_eq!(summary.issue_id, issue.id);
	assert_eq!(summary.issue_identifier, issue.identifier);
	assert_eq!(summary.dispatch_mode, IssueDispatchMode::ReviewRepair);
	assert_eq!(summary.issue_state, "In Review");
}

#[test]
fn targeted_identifier_dispatch_accepts_stopped_active_closeout_lease() {
	let (temp_dir, base_config, workflow) = tests::temp_project_layout();
	let config = tests::service_config_with_github_token_env_var(&base_config, "HOME");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = run_and_prompting_service_owned_issue("Done");
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree =
		worktree_manager.ensure_worktree(&issue.identifier, false).expect("worktree should exist");
	let head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/183";

	state_store
		.record_run_attempt("run-1", &issue.id, 1, "running")
		.expect("stopped running run attempt should record");
	state_store
		.upsert_lease(config.service_id(), &issue.id, "run-1", "Done")
		.expect("stopped closeout lease should record");
	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree should record");

	state::write_run_activity_marker_for_process(&worktree.path, "run-1", 1, u32::MAX)
		.expect("stopped closeout activity marker should write");
	tests::seed_review_handoff_marker_for_path(
		&state_store,
		config.service_id(),
		&worktree.path,
		&tests::sample_review_handoff_marker(&worktree.branch_name, pr_url, &head_oid),
	);

	let _path_guard = recovery_terminal_support::install_fake_merged_pr_gh_response(
		&temp_dir, &worktree, pr_url, &head_oid,
	);
	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("status snapshot should build");
	let lane = snapshot
		.post_review_lanes
		.iter()
		.find(|lane| lane.issue_identifier == issue.identifier)
		.expect("retained closeout lane should appear in status");

	assert_eq!(lane.classification, "continue");
	assert_eq!(lane.reason, "pull_request_merged_closeout_pending");
	assert_eq!(lane.pr_state.as_deref(), Some("MERGED"));
	assert_eq!(snapshot.current_lanes.len(), 1);
	assert_eq!(snapshot.current_lanes[0].process_alive, Some(false));
	assert!(
		orchestrator::issue_passes_closeout_dispatch_policy(
			&tracker,
			&issue,
			&config,
			&workflow,
			&state_store,
		)
		.expect("closeout dispatch policy should evaluate"),
		"stopped active closeout lease should still satisfy closeout policy"
	);
	assert!(
		!orchestrator::target_issue_active_claim_blocks_dispatch(
			&run_and_prompting_target_context(
				&tracker,
				&config,
				&workflow,
				&state_store,
				&issue.identifier,
				IssueDispatchMode::Closeout,
			),
			&issue.id,
			&issue,
		)
		.expect("active closeout claim guard should evaluate"),
		"stopped active closeout lease should not block closeout dispatch"
	);

	let explicit_summary = orchestrator::run_target_issue_once(run_and_prompting_target_context(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&issue.identifier,
		IssueDispatchMode::Closeout,
	))
	.expect("explicit retained closeout identifier run should succeed")
	.expect("explicit closeout should accept the stopped active closeout lease");

	assert_eq!(explicit_summary.dispatch_mode, IssueDispatchMode::Closeout);

	let summary = orchestrator::run_target_issue_once_with_inferred_dispatch(
		run_and_prompting_target_context(
			&tracker,
			&config,
			&workflow,
			&state_store,
			&issue.identifier,
			IssueDispatchMode::Normal,
		),
	)
	.expect("targeted retained closeout identifier run should succeed")
	.expect("stopped active closeout lease should not hide the closeout candidate");

	assert_eq!(summary.issue_id, issue.id);
	assert_eq!(summary.issue_identifier, issue.identifier);
	assert_eq!(summary.dispatch_mode, IssueDispatchMode::Closeout);
	assert_eq!(summary.run_id, "run-1");
	assert_eq!(summary.attempt_number, 1);
}

#[test]
fn targeted_identifier_dispatch_rejects_different_status_visible_closeout_lane() {
	let (temp_dir, base_config, workflow) = tests::temp_project_layout();
	let config = tests::service_config_with_github_token_env_var(&base_config, "HOME");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let closeout_issue = tests::sample_issue_with_sort_fields(
		"issue-closeout",
		"PUB-101",
		"In Review",
		&[active_label.as_str()],
		Some(1),
		"2026-03-13T04:16:17.133Z",
	);
	let requested_issue = tests::sample_issue_with_sort_fields(
		"issue-requested",
		"PUB-102",
		"In Review",
		&[active_label.as_str()],
		Some(2),
		"2026-03-13T04:17:17.133Z",
	);
	let tracker = FakeTracker::new(vec![closeout_issue.clone(), requested_issue.clone()]);
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&closeout_issue.identifier, false)
		.expect("worktree should exist");
	let head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/182";

	state_store
		.upsert_worktree(
			config.service_id(),
			&closeout_issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree should record");

	tests::seed_review_handoff_marker_for_path(
		&state_store,
		config.service_id(),
		&worktree.path,
		&tests::sample_review_handoff_marker(&worktree.branch_name, pr_url, &head_oid),
	);

	let _path_guard = recovery_terminal_support::install_fake_merged_pr_gh_response(
		&temp_dir, &worktree, pr_url, &head_oid,
	);
	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("status snapshot should build");

	assert_eq!(snapshot.post_review_lanes.len(), 1);
	assert_eq!(snapshot.post_review_lanes[0].issue_identifier, closeout_issue.identifier);
	assert_eq!(snapshot.post_review_lanes[0].classification, "continue");
	assert_eq!(snapshot.post_review_lanes[0].reason, "pull_request_merged_closeout_pending",);

	let error = orchestrator::run_target_issue_once_with_inferred_dispatch(TargetIssueRunContext {
		tracker: &tracker,
		project: &config,
		workflow: &workflow,
		state_store: &state_store,
		issue_id: &requested_issue.identifier,
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
	.expect_err("targeted closeout inference should reject a different visible lane");
	let message = error.to_string();

	assert!(message.contains("targeted retained closeout mismatch"));
	assert!(message.contains(&requested_issue.identifier));
	assert!(message.contains(&closeout_issue.identifier));
}

#[test]
fn targeted_identifier_dispatch_rejects_different_status_visible_review_repair_lane() {
	let (temp_dir, base_config, workflow) = tests::temp_project_layout();
	let config = tests::service_config_with_github_token_env_var(&base_config, "HOME");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let repair_issue = tests::sample_issue_with_sort_fields(
		"issue-repair",
		"PUB-201",
		"In Review",
		&[active_label.as_str()],
		Some(1),
		"2026-03-13T04:16:17.133Z",
	);
	let requested_issue = tests::sample_issue_with_sort_fields(
		"issue-requested",
		"PUB-202",
		"In Review",
		&[active_label.as_str()],
		Some(2),
		"2026-03-13T04:17:17.133Z",
	);
	let tracker = FakeTracker::new(vec![repair_issue.clone(), requested_issue.clone()]);
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&repair_issue.identifier, false)
		.expect("worktree should exist");
	let head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/185";

	state_store
		.upsert_worktree(
			config.service_id(),
			&repair_issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree should record");

	tests::seed_review_handoff_marker_for_path(
		&state_store,
		config.service_id(),
		&worktree.path,
		&tests::sample_review_handoff_marker(&worktree.branch_name, pr_url, &head_oid),
	);

	let _path_guard = recovery_terminal_support::install_fake_conflicting_pr_gh_response(
		&temp_dir, &worktree, pr_url, &head_oid,
	);
	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("status snapshot should build");

	assert_eq!(snapshot.post_review_lanes.len(), 1);
	assert_eq!(snapshot.post_review_lanes[0].issue_identifier, repair_issue.identifier);
	assert_eq!(snapshot.post_review_lanes[0].classification, "needs_review_repair");
	assert_eq!(snapshot.post_review_lanes[0].reason, "pull_request_merge_conflict");

	let error = orchestrator::run_target_issue_once_with_inferred_dispatch(TargetIssueRunContext {
		tracker: &tracker,
		project: &config,
		workflow: &workflow,
		state_store: &state_store,
		issue_id: &requested_issue.identifier,
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
	.expect_err("targeted review repair inference should reject a different visible lane");
	let message = error.to_string();

	assert!(message.contains("targeted retained review repair mismatch"));
	assert!(message.contains(&requested_issue.identifier));
	assert!(message.contains(&repair_issue.identifier));
}

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
	};
	let message = orchestrator::format_run_once_summary(&summary, false);

	assert!(message.contains("run paused at continuation boundary"));
	assert!(message.contains("next_action=rerun_or_use_daemon"));
	assert!(!message.contains("run complete"));
}

#[test]
fn dry_run_returns_none_when_intake_has_no_service_owned_candidate() {
	{
		let (_temp_dir, config, workflow) = tests::temp_project_layout();
		let tracker = FakeTracker::with_refresh_snapshots_and_project(vec![], vec![vec![]], false);
		let state_store = StateStore::open_in_memory().expect("state store should open");
		let summary =
			orchestrator::run_project_once(&tracker, &config, &workflow, &state_store, true)
				.expect("dry run without queued issues should succeed");

		assert!(summary.is_none(), "empty intake should simply produce no dry-run selection");
	}
	{
		let (_temp_dir, config, workflow) = tests::temp_project_layout();
		let issue = tests::sample_issue_with_project_slug_and_sort_fields(
			"issue-1",
			"PUB-101",
			"other-service",
			"Todo",
			&[],
			Some(3),
			"2026-03-13T04:16:17.133Z",
		);
		let tracker = FakeTracker::new(vec![issue]);
		let state_store = StateStore::open_in_memory().expect("state store should open");
		let summary =
			orchestrator::run_project_once(&tracker, &config, &workflow, &state_store, true)
				.expect("dry run should succeed");

		assert!(summary.is_none(), "service-scoped queue labels should isolate intake");
	}
}

#[test]
fn no_eligible_issue_message_includes_operator_hint() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let message = orchestrator::format_no_eligible_issue_message(&config, &workflow);

	assert!(message.contains("No eligible issue found for the configured project."));
	assert!(message.contains("`Todo`"));
	assert!(message.contains("`decodex:queued:<service-id>`"));
	assert!(message.contains("`decodex:queued:pubfi`"));
	assert!(message.contains("`decodex:manual-only`/`decodex:needs-attention`"));
	assert!(message.contains("non-terminal state"));
	assert!(message.contains("dependency blockers"));
	assert!(message.contains("no active issue claim"));
}

#[test]
fn dry_run_falls_back_to_normal_issue_when_retained_retry_loses_ownership() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let normal_issue = tests::sample_issue_with_sort_fields(
		"issue-2",
		"PUB-102",
		"Todo",
		&[],
		Some(1),
		"2026-03-13T04:17:17.133Z",
	);
	let retry_issue = run_and_prompting_service_owned_issue("In Progress");
	let retry_issue_without_ownership = tests::sample_issue("In Progress", &[]);
	let tracker = FakeTracker::with_refresh_snapshots(
		vec![normal_issue.clone(), retry_issue.clone()],
		vec![
			vec![retry_issue.clone()],
			vec![retry_issue.clone()],
			vec![retry_issue_without_ownership.clone()],
			vec![retry_issue_without_ownership],
			vec![tests::sample_issue("In Progress", &[])],
			vec![normal_issue.clone()],
		],
	);
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&retry_issue.identifier, false)
		.expect("retained retry worktree should exist");

	state_store
		.upsert_worktree(
			config.service_id(),
			&retry_issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree mapping should record");

	let summary = orchestrator::run_project_once(&tracker, &config, &workflow, &state_store, true)
		.expect("dry run should succeed")
		.expect("normal queued issue should be selected after retained retry is excluded");

	assert_eq!(summary.issue_identifier, normal_issue.identifier);
	assert_eq!(summary.dispatch_mode, IssueDispatchMode::Normal);
}

#[test]
fn developer_instructions_trim_workflow_body_and_preserve_required_guidance() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue("Todo", &[]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let issue_run = orchestrator::IssueRunPlan {
		issue,
		issue_state: String::from("In Progress"),
		initial_issue_state: String::from("Todo"),
		worktree: WorktreeSpec {
			branch_name: String::from("x/pubfi-pub-101"),
			issue_identifier: String::from("PUB-101"),
			path: config.worktree_root().join("PUB-101"),
			reused_existing: false,
		},
		retry_project_slug: String::from("pubfi"),
		dispatch_mode: IssueDispatchMode::Normal,
		attempt_number: 1,
		run_id: String::from("pub-101-attempt-1-123"),
		retry_budget_base: 0,
	};
	let instructions = orchestrator::build_developer_instructions(
		&tracker,
		&config,
		&workflow,
		&issue_run,
		&StateStore::open_in_memory().expect("state store should open"),
		None,
	)
	.expect("developer instructions should build");

	assert!(instructions.contains("Workflow policy\nFollow the repository policy.\n"));
	assert!(instructions.contains("Keep pre-edit discovery bounded"));
	assert!(instructions.contains("Do not browse upstream references"));
	assert!(instructions.contains("Docs impact contract"));
	assert!(instructions.contains(
		"classify docs impact as `none`, `update_required`, `research_required`, or `drift_required`"
	));
	assert!(
		instructions
			.contains("record it in a current-HEAD `issue_progress_checkpoint` as `docs_impact`")
	);
	assert!(instructions.contains("Tracker tool contract"));
	assert!(instructions.contains("Linear tracker text is public/team-visible"));
	assert!(instructions.contains("You own issue-scoped tracker writes for `PUB-101`."));
	assert!(instructions.contains("Decodex already records the run-start Linear ledger"));
	assert!(!instructions.contains("started work on run"));
	assert!(
		instructions.contains("Do not speculate about capabilities you did not directly verify.")
	);
	assert!(instructions.contains(ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME));
	assert!(instructions.contains(ISSUE_REVIEW_CHECKPOINT_TOOL_NAME));
	assert!(instructions.contains(ISSUE_REVIEW_HANDOFF_TOOL_NAME));
	assert!(instructions.contains(ISSUE_TERMINAL_FINALIZE_TOOL_NAME));
	assert!(instructions.contains("Phase goal runtime contract"));
	assert!(
		instructions.contains("Treat the active phase goal as the authoritative current contract")
	);
	assert!(instructions.contains(
		"explicitly complete the active phase goal with the Codex goal completion mechanism"
	));
	assert!(
		instructions.contains(
			"Do not use `issue_progress_checkpoint`, final chat text, or an \"await next phase\" statement as a substitute"
		)
	);
	assert!(instructions.contains("treat `issue_progress_checkpoint` as terminal completion"));
	assert!(!instructions.contains("you may end the turn without"));
	assert!(!instructions.contains("WORKFLOW.md\n"));
}

#[test]
fn normal_prompts_record_manual_attention_label_intent_before_label_application() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let surfaces = build_normal_prompt_surfaces(&config, &workflow);

	for prompt in surfaces.all() {
		assert_manual_attention_prompt_guidance(prompt, true);
	}
}

#[test]
fn normal_prompts_require_review_signal_routes_before_repair() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let surfaces = build_normal_prompt_surfaces(&config, &workflow);

	for prompt in surfaces.all() {
		assert_review_route_prompt_guidance(prompt);

		assert!(prompt.contains(ISSUE_REVIEW_CHECKPOINT_TOOL_NAME));
	}
}

#[test]
fn review_pull_request_title_normalizes_issue_prefix() {
	for title in [
		"Ensure Decodex-created PR titles include issue authority prefix",
		"xy-381: Ensure Decodex-created PR titles include issue authority prefix",
	] {
		let mut issue = tests::sample_issue("Todo", &[]);

		issue.identifier = String::from("XY-381");
		issue.title = String::from(title);

		assert_eq!(
			orchestrator::review_pull_request_title(&issue),
			"XY-381: Ensure Decodex-created PR titles include issue authority prefix"
		);
	}
}

#[test]
fn normal_prompts_require_issue_prefixed_pull_request_title() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let mut issue = tests::sample_issue("Todo", &[]);

	issue.identifier = String::from("XY-381");
	issue.title = String::from("Ensure Decodex-created PR titles include issue authority prefix");

	let tracker = FakeTracker::new(vec![issue.clone()]);
	let issue_run = orchestrator::IssueRunPlan {
		issue: issue.clone(),
		issue_state: String::from("In Progress"),
		initial_issue_state: String::from("Todo"),
		worktree: WorktreeSpec {
			branch_name: String::from("y/decodex-xy-381"),
			issue_identifier: String::from("XY-381"),
			path: config.worktree_root().join("XY-381"),
			reused_existing: false,
		},
		retry_project_slug: String::from("decodex"),
		dispatch_mode: IssueDispatchMode::Normal,
		attempt_number: 1,
		run_id: String::from("xy-381-attempt-1-123"),
		retry_budget_base: 0,
	};
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let developer_instructions = orchestrator::build_developer_instructions(
		&tracker,
		&config,
		&workflow,
		&issue_run,
		&state_store,
		None,
	)
	.expect("developer instructions should build");
	let user_input = orchestrator::build_user_input(
		&tracker,
		&config,
		&issue,
		&workflow,
		&issue_run,
		&state_store,
		None,
	);
	let continuation_input = orchestrator::build_continuation_user_input(
		&issue,
		&workflow,
		IssueDispatchMode::Normal,
		None,
		workflow.frontmatter().tracker().success_state(),
		config.codex().review_level(),
	);
	let expected_title = "XY-381: Ensure Decodex-created PR titles include issue authority prefix";
	let create_or_update_instruction =
		format!("create or update a non-draft PR titled `{expected_title}`");

	assert!(developer_instructions.contains(&create_or_update_instruction));
	assert!(user_input.contains(&create_or_update_instruction));
	assert!(
		continuation_input
			.contains(&format!("ensure the non-draft PR title is `{expected_title}`"))
	);
	assert!(developer_instructions.contains("single-line `decodex/commit/1` JSON commit message"));
}

#[test]
fn retry_prompts_include_recovery_context() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue("In Progress", &[]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let issue_run = orchestrator::IssueRunPlan {
		issue: issue.clone(),
		issue_state: String::from("In Progress"),
		initial_issue_state: String::from("In Progress"),
		worktree: WorktreeSpec {
			branch_name: String::from("x/pubfi-pub-101"),
			issue_identifier: String::from("PUB-101"),
			path: config.worktree_root().join("PUB-101"),
			reused_existing: true,
		},
		retry_project_slug: String::from("pubfi"),
		dispatch_mode: IssueDispatchMode::Retry,
		attempt_number: 2,
		run_id: String::from("pub-101-attempt-2-123"),
		retry_budget_base: 1,
	};
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let developer_instructions = orchestrator::build_developer_instructions(
		&tracker,
		&config,
		&workflow,
		&issue_run,
		&state_store,
		None,
	)
	.expect("developer instructions should build");
	let user_input = orchestrator::build_user_input(
		&tracker,
		&config,
		&issue,
		&workflow,
		&issue_run,
		&state_store,
		None,
	);

	for prompt in [&developer_instructions, &user_input] {
		assert!(prompt.contains("Recovery context"));
		assert!(prompt.contains("Treat the current worktree"));
		assert!(prompt.contains("Do not assume in-memory model output or tool results survived"));
	}
}

#[test]
fn architecture_recovery_prompt_uses_only_latest_active_recovery_start() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue("In Progress", &[]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let issue_run = orchestrator::IssueRunPlan {
		issue: issue.clone(),
		issue_state: String::from("In Progress"),
		initial_issue_state: String::from("In Progress"),
		worktree: WorktreeSpec {
			branch_name: String::from("x/pubfi-pub-101"),
			issue_identifier: String::from("PUB-101"),
			path: config.worktree_root().join("PUB-101"),
			reused_existing: true,
		},
		retry_project_slug: String::from("pubfi"),
		dispatch_mode: IssueDispatchMode::Retry,
		attempt_number: 3,
		run_id: String::from("pub-101-attempt-3-123"),
		retry_budget_base: 2,
	};
	let state_store = StateStore::open_in_memory().expect("state store should open");

	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			&issue.id,
			"pub-101-attempt-2-123",
			2,
			"architecture_recovery_started",
			serde_json::json!({
				"schema": "decodex.architecture_recovery_started/1",
				"reason_code": "architecture_recovery_started",
				"guardrail_reason": "review_churn",
				"recovery_budget": {
					"attempt": 1,
					"max_attempts": 1,
				},
			}),
		)
		.expect("architecture recovery start event should record");

	let developer_instructions = orchestrator::build_developer_instructions(
		&tracker,
		&config,
		&workflow,
		&issue_run,
		&state_store,
		None,
	)
	.expect("developer instructions should build");

	assert!(developer_instructions.contains("Architecture recovery context"));
	assert!(developer_instructions.contains("guardrail `review_churn`"));

	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			&issue.id,
			"pub-101-attempt-2-123",
			2,
			"architecture_recovery_terminal",
			serde_json::json!({
				"schema": "decodex.architecture_recovery_terminal/1",
				"reason_code": "architecture_recovery_exhausted",
				"guardrail_reason": "review_churn",
				"recovery_budget": {
					"attempt": 2,
					"max_attempts": 1,
				},
			}),
		)
		.expect("architecture recovery terminal event should record");

	let developer_instructions = orchestrator::build_developer_instructions(
		&tracker,
		&config,
		&workflow,
		&issue_run,
		&state_store,
		None,
	)
	.expect("developer instructions should build");

	assert!(!developer_instructions.contains("Architecture recovery context"));
	assert!(!developer_instructions.contains("guardrail `review_churn`"));
}

#[test]
fn normal_prompts_respect_non_standard_review_levels() {
	for (mode, expected, forbidden_checkpoint) in [
		(ReviewLevel::Off, "[codex].review = \"off\"", None),
		(
			ReviewLevel::Basic,
			"Self Check: Review your work repeatedly and fix any logic bugs until no new issues are found.",
			Some(ISSUE_REVIEW_CHECKPOINT_TOOL_NAME),
		),
	] {
		let (_temp_dir, config, workflow) = tests::temp_project_layout();
		let config = tests::service_config_with_review_level(&config, mode);
		let prompts = build_normal_prompt_surfaces(&config, &workflow);

		for prompt in prompts.all() {
			assert!(prompt.contains(expected), "{mode:?} prompt should contain `{expected}`");
			assert!(!prompt.contains("Follow the repo-native bounded review method"));

			if let Some(forbidden_checkpoint) = forbidden_checkpoint {
				assert!(!prompt.contains(forbidden_checkpoint));
			}

			assert!(!prompt.contains("only after the latest `issue_review_checkpoint`"));
		}

		assert!(
			prompts
				.developer_instructions
				.contains("Call `issue_review_handoff` after the branch is pushed")
		);
		assert!(prompts.user_input.contains("required validation has passed"));
		assert!(prompts.continuation_input.contains("after required validation has passed"));
	}
}

#[test]
fn multi_turn_prompts_allow_nonterminal_yield_boundary() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout_with_max_turns(4);
	let issue = tests::sample_issue("Todo", &[]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let issue_run = orchestrator::IssueRunPlan {
		issue: issue.clone(),
		issue_state: String::from("In Progress"),
		initial_issue_state: String::from("Todo"),
		worktree: WorktreeSpec {
			branch_name: String::from("x/pubfi-pub-101"),
			issue_identifier: String::from("PUB-101"),
			path: config.worktree_root().join("PUB-101"),
			reused_existing: false,
		},
		retry_project_slug: String::from("pubfi"),
		dispatch_mode: IssueDispatchMode::Normal,
		attempt_number: 1,
		run_id: String::from("pub-101-attempt-1-123"),
		retry_budget_base: 0,
	};
	let user_input = orchestrator::build_user_input(
		&tracker,
		&config,
		&issue,
		&workflow,
		&issue_run,
		&StateStore::open_in_memory().expect("state store should open"),
		None,
	);
	let continuation_input = orchestrator::build_continuation_user_input(
		&issue,
		&workflow,
		IssueDispatchMode::Normal,
		None,
		workflow.frontmatter().tracker().success_state(),
		config.codex().review_level(),
	);

	assert!(user_input.contains("you may end the turn without"));
	assert!(continuation_input.contains("you may end the turn without terminal finalization"));
	assert!(!user_input.contains("Do not end the turn"));
}

#[test]
fn closeout_prompts_forbid_clean_continuation_boundaries() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout_with_max_turns(4);
	let issue = tests::sample_issue("In Review", &[]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let issue_run = orchestrator::IssueRunPlan {
		issue: issue.clone(),
		issue_state: String::from("In Review"),
		initial_issue_state: String::from("In Review"),
		worktree: WorktreeSpec {
			branch_name: String::from("x/pubfi-pub-101"),
			issue_identifier: String::from("PUB-101"),
			path: config.worktree_root().join("PUB-101"),
			reused_existing: true,
		},
		retry_project_slug: String::from("pubfi"),
		dispatch_mode: IssueDispatchMode::Closeout,
		attempt_number: 3,
		run_id: String::from("pub-101-attempt-3-123"),
		retry_budget_base: 0,
	};
	let pr_url = "https://github.com/hack-ink/decodex/pull/175";
	let developer_instructions = orchestrator::build_developer_instructions(
		&tracker,
		&config,
		&workflow,
		&issue_run,
		&StateStore::open_in_memory().expect("state store should open"),
		Some(pr_url),
	)
	.expect("closeout developer instructions should build");
	let user_input = orchestrator::build_user_input(
		&tracker,
		&config,
		&issue,
		&workflow,
		&issue_run,
		&StateStore::open_in_memory().expect("state store should open"),
		Some(pr_url),
	);
	let continuation_input = orchestrator::build_continuation_user_input(
		&issue,
		&workflow,
		IssueDispatchMode::Closeout,
		Some(pr_url),
		workflow.frontmatter().tracker().success_state(),
		config.codex().review_level(),
	);

	for prompt in [&developer_instructions, &user_input, &continuation_input] {
		assert!(prompt.contains("short deterministic tail"));
		assert!(prompt.contains("Do not end the turn without"));
		assert!(!prompt.contains("you may end the turn without terminal finalization"));
	}
}

#[test]
fn review_repair_prompts_require_same_pr_repair_completion() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout_with_max_turns(4);
	let issue = tests::sample_issue("In Review", &[]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let issue_run = orchestrator::IssueRunPlan {
		issue: issue.clone(),
		issue_state: String::from("In Review"),
		initial_issue_state: String::from("In Review"),
		worktree: WorktreeSpec {
			branch_name: String::from("x/pubfi-pub-101"),
			issue_identifier: String::from("PUB-101"),
			path: config.worktree_root().join("PUB-101"),
			reused_existing: true,
		},
		retry_project_slug: String::from("pubfi"),
		dispatch_mode: IssueDispatchMode::ReviewRepair,
		attempt_number: 2,
		run_id: String::from("pub-101-attempt-2-123"),
		retry_budget_base: 0,
	};
	let pr_url = "https://github.com/hack-ink/decodex/pull/77";
	let developer_instructions = orchestrator::build_developer_instructions(
		&tracker,
		&config,
		&workflow,
		&issue_run,
		&StateStore::open_in_memory().expect("state store should open"),
		Some(pr_url),
	)
	.expect("review repair developer instructions should build");
	let user_input = orchestrator::build_user_input(
		&tracker,
		&config,
		&issue,
		&workflow,
		&issue_run,
		&StateStore::open_in_memory().expect("state store should open"),
		Some(pr_url),
	);
	let continuation_input = orchestrator::build_continuation_user_input(
		&issue,
		&workflow,
		IssueDispatchMode::ReviewRepair,
		Some(pr_url),
		workflow.frontmatter().tracker().success_state(),
		config.codex().review_level(),
	);

	assert_review_repair_developer_prompt(&developer_instructions);
	assert_review_repair_user_prompt(&user_input, pr_url);
	assert_review_repair_continuation_prompt(&continuation_input);

	for prompt in [&developer_instructions, &user_input, &continuation_input] {
		assert_manual_attention_prompt_guidance(prompt, false);
	}

	assert_prompt_orders_thread_replies_after_push(
		&developer_instructions,
		"push the repaired head.",
	);
	assert_prompt_orders_thread_replies_after_push(
		&user_input,
		"Commit the repair and push the same branch.",
	);
	assert_prompt_orders_thread_replies_after_push(
		&continuation_input,
		"If the repaired head is ready, push it.",
	);
}

#[test]
fn review_repair_prompts_skip_decodex_review_checkpoint_when_off() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout_with_max_turns(4);
	let config = tests::service_config_with_review_level(&config, ReviewLevel::Off);
	let issue = tests::sample_issue("In Review", &[]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let issue_run = orchestrator::IssueRunPlan {
		issue: issue.clone(),
		issue_state: String::from("In Review"),
		initial_issue_state: String::from("In Review"),
		worktree: WorktreeSpec {
			branch_name: String::from("x/pubfi-pub-101"),
			issue_identifier: String::from("PUB-101"),
			path: config.worktree_root().join("PUB-101"),
			reused_existing: true,
		},
		retry_project_slug: String::from("pubfi"),
		dispatch_mode: IssueDispatchMode::ReviewRepair,
		attempt_number: 2,
		run_id: String::from("pub-101-attempt-2-123"),
		retry_budget_base: 0,
	};
	let pr_url = "https://github.com/hack-ink/decodex/pull/77";
	let developer_instructions = orchestrator::build_developer_instructions(
		&tracker,
		&config,
		&workflow,
		&issue_run,
		&StateStore::open_in_memory().expect("state store should open"),
		Some(pr_url),
	)
	.expect("review repair developer instructions should build");
	let user_input = orchestrator::build_user_input(
		&tracker,
		&config,
		&issue,
		&workflow,
		&issue_run,
		&StateStore::open_in_memory().expect("state store should open"),
		Some(pr_url),
	);
	let continuation_input = orchestrator::build_continuation_user_input(
		&issue,
		&workflow,
		IssueDispatchMode::ReviewRepair,
		Some(pr_url),
		workflow.frontmatter().tracker().success_state(),
		config.codex().review_level(),
	);

	for prompt in [&developer_instructions, &user_input, &continuation_input] {
		assert!(prompt.contains("[codex].review = \"off\""));
		assert!(prompt.contains("do not call `issue_review_checkpoint`"));
		assert!(!prompt.contains("Follow the repo-native bounded review method"));
		assert!(!prompt.contains("only after the latest `issue_review_checkpoint`"));
		assert!(prompt.contains(ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME));
	}

	assert!(
		developer_instructions
			.contains("Call `issue_review_repair_complete` after the repaired head is pushed")
	);
	assert!(user_input.contains("required validation has passed"));
	assert!(continuation_input.contains("required validation has passed"));
	assert!(user_input.contains("validate each actionable claim against the codebase"));
	assert!(continuation_input.contains("Do not request GitHub Review from this run"));
}

#[test]
fn review_repair_continuation_prompt_uses_configured_success_state() {
	let workflow = WorkflowDocument::parse_markdown(
		r#"
+++
version = 1

[tracker]
provider = "linear"
startable_states = ["Todo"]
terminal_states = ["Done", "Canceled", "Duplicate"]
in_progress_state = "In Progress"
success_state = "Ready For QA"
completed_state = "Done"
failure_state = "Todo"
opt_out_label = "decodex:manual-only"
needs_attention_label = "decodex:needs-attention"

[agent]
transport = "stdio://"

[execution]
max_attempts = 3
max_turns = 4
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

Custom workflow.
"#,
	)
	.expect("workflow should parse");
	let issue = tests::sample_issue("Ready For QA", &[]);
	let continuation_input = orchestrator::build_continuation_user_input(
		&issue,
		&workflow,
		IssueDispatchMode::ReviewRepair,
		Some("https://github.com/hack-ink/decodex/pull/77"),
		workflow.frontmatter().tracker().success_state(),
		ReviewLevel::Standard,
	);

	assert!(continuation_input.contains("Ready For QA"));
	assert!(!continuation_input.contains("do not move the issue out of `In Review`"));
}

#[test]
fn review_repair_prompts_surface_architecture_check_on_fourth_external_round() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue("In Review", &[]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_path = config.worktree_root().join(&issue.identifier);
	let pr_url = "https://github.com/hack-ink/decodex/pull/77";
	let review_handoff = ReviewHandoffMarker::new(
		"pub-101-attempt-4-123",
		4,
		"x/pubfi-pub-101",
		pr_url,
		"main",
		"x/pubfi-pub-101",
		"abc123",
	);

	tests::seed_review_handoff_marker_value(
		&state_store,
		config.service_id(),
		&issue.id,
		&review_handoff,
	);
	tests::seed_review_orchestration_marker(
		&state_store,
		config.service_id(),
		&issue.id,
		&ReviewOrchestrationMarker::new(
			"pub-101-attempt-4-123",
			4,
			"x/pubfi-pub-101",
			pr_url,
			"abc123",
			"repair_required",
			None,
			None,
			None,
			0,
			4,
			None,
		),
	);

	let issue_run = orchestrator::IssueRunPlan {
		issue: issue.clone(),
		issue_state: String::from("In Review"),
		initial_issue_state: String::from("In Review"),
		worktree: WorktreeSpec {
			branch_name: String::from("x/pubfi-pub-101"),
			issue_identifier: issue.identifier.clone(),
			path: worktree_path,
			reused_existing: true,
		},
		retry_project_slug: String::from("pubfi"),
		dispatch_mode: IssueDispatchMode::ReviewRepair,
		attempt_number: 4,
		run_id: String::from("pub-101-attempt-4-123"),
		retry_budget_base: 0,
	};
	let developer_instructions = orchestrator::build_developer_instructions(
		&tracker,
		&config,
		&workflow,
		&issue_run,
		&state_store,
		Some(pr_url),
	)
	.expect("developer instructions should build");
	let user_input = orchestrator::build_user_input(
		&tracker,
		&config,
		&issue,
		&workflow,
		&issue_run,
		&state_store,
		Some(pr_url),
	);

	assert!(developer_instructions.contains("GitHub Review round 4"));
	assert!(developer_instructions.contains("architectural or root-cause defect"));
	assert!(developer_instructions.contains("reset the GitHub Review round budget"));
	assert!(user_input.contains("GitHub Review round 4"));
	assert!(user_input.contains("Do not request GitHub Review yourself"));
}

#[test]
fn review_repair_prompts_ignore_newer_unrelated_branch_orchestration_records() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue("In Review", &[]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_path = config.worktree_root().join(&issue.identifier);
	let pr_url = "https://github.com/hack-ink/decodex/pull/77";
	let current_handoff = ReviewHandoffMarker::new(
		"pub-101-attempt-4-123",
		4,
		"x/pubfi-pub-101",
		pr_url,
		"main",
		"x/pubfi-pub-101",
		"abc123",
	);

	tests::seed_review_handoff_marker_value(
		&state_store,
		config.service_id(),
		&issue.id,
		&current_handoff,
	);
	tests::seed_review_orchestration_marker(
		&state_store,
		config.service_id(),
		&issue.id,
		&ReviewOrchestrationMarker::new(
			"pub-101-attempt-4-123",
			4,
			"x/pubfi-pub-101",
			pr_url,
			"abc123",
			"repair_required",
			None,
			None,
			None,
			0,
			3,
			None,
		),
	);

	let unrelated_handoff = ReviewHandoffMarker::new(
		"other-run",
		1,
		"x/pubfi-pub-101-next",
		"https://github.com/hack-ink/decodex/pull/88",
		"main",
		"x/pubfi-pub-101-next",
		"def456",
	);

	tests::seed_review_handoff_marker_value(
		&state_store,
		config.service_id(),
		&issue.id,
		&unrelated_handoff,
	);
	tests::seed_review_orchestration_marker(
		&state_store,
		config.service_id(),
		&issue.id,
		&ReviewOrchestrationMarker::new(
			"other-run",
			1,
			"x/pubfi-pub-101-next",
			"https://github.com/hack-ink/decodex/pull/88",
			"def456",
			"repair_required",
			None,
			None,
			None,
			0,
			4,
			None,
		),
	);

	let issue_run = orchestrator::IssueRunPlan {
		issue: issue.clone(),
		issue_state: String::from("In Review"),
		initial_issue_state: String::from("In Review"),
		worktree: WorktreeSpec {
			branch_name: String::from("x/pubfi-pub-101"),
			issue_identifier: issue.identifier.clone(),
			path: worktree_path,
			reused_existing: true,
		},
		retry_project_slug: String::from("pubfi"),
		dispatch_mode: IssueDispatchMode::ReviewRepair,
		attempt_number: 4,
		run_id: String::from("pub-101-attempt-4-123"),
		retry_budget_base: 0,
	};
	let developer_instructions = orchestrator::build_developer_instructions(
		&tracker,
		&config,
		&workflow,
		&issue_run,
		&state_store,
		Some(pr_url),
	)
	.expect("review repair developer instructions should build");

	assert!(!developer_instructions.contains("GitHub Review round 4"));
	assert!(!developer_instructions.contains("architectural or root-cause defect"));
}

#[test]
fn closeout_prompts_require_retained_pr_closeout_completion() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue("In Review", &[]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let issue_run = orchestrator::IssueRunPlan {
		issue: issue.clone(),
		issue_state: String::from("In Review"),
		initial_issue_state: String::from("In Review"),
		worktree: WorktreeSpec {
			branch_name: String::from("x/pubfi-pub-101"),
			issue_identifier: issue.identifier.clone(),
			path: config.worktree_root().join(&issue.identifier),
			reused_existing: true,
		},
		retry_project_slug: String::from("pubfi"),
		dispatch_mode: IssueDispatchMode::Closeout,
		attempt_number: 3,
		run_id: String::from("pub-101-attempt-3-123"),
		retry_budget_base: 0,
	};
	let pr_url = "https://github.com/hack-ink/decodex/pull/175";
	let developer_instructions = orchestrator::build_developer_instructions(
		&tracker,
		&config,
		&workflow,
		&issue_run,
		&StateStore::open_in_memory().expect("state store should open"),
		Some(pr_url),
	)
	.expect("closeout developer instructions should build");
	let user_input = orchestrator::build_user_input(
		&tracker,
		&config,
		&issue,
		&workflow,
		&issue_run,
		&StateStore::open_in_memory().expect("state store should open"),
		Some(pr_url),
	);
	let continuation_input = orchestrator::build_continuation_user_input(
		&issue,
		&workflow,
		IssueDispatchMode::Closeout,
		Some(pr_url),
		workflow.frontmatter().tracker().success_state(),
		config.codex().review_level(),
	);

	assert!(developer_instructions.contains(ISSUE_DELIVERY_CLOSEOUT_COMPLETE_TOOL_NAME));
	assert!(developer_instructions.contains(ISSUE_TRANSITION_TOOL_NAME));
	assert!(developer_instructions.contains("Merge is already authoritative"));
	assert!(developer_instructions.contains("Do not land, merge, or request review"));
	assert!(developer_instructions.contains("single-line `decodex/commit/1` JSON commit message"));
	assert!(developer_instructions.contains("do not call `issue_review_handoff`"));
	assert!(developer_instructions.contains("may already be in `Done`"));
	assert!(developer_instructions.contains(
		"either omit `head_sha` and let `decodex` record the exact current lane HEAD automatically, or pass the exact full current `HEAD` SHA"
	));
	assert!(developer_instructions.contains(
		"If the issue is still in `In Review`, transition it once to `Done` with `issue_transition` before `issue_closeout_complete`"
	));
	assert!(user_input.contains("merged PR lineage"));
	assert!(user_input.contains("Merge is already authoritative"));
	assert!(user_input.contains("Do not land, merge, or request review"));
	assert!(user_input.contains("may already be in `Done`"));
	assert!(user_input.contains(
		"either omit `head_sha` and let `decodex` record the exact current lane HEAD automatically, or pass the exact full current `HEAD` SHA"
	));
	assert!(user_input.contains(
		"If the issue is still in `In Review`, move it once to `Done` with `issue_transition` before `issue_closeout_complete`"
	));
	assert!(user_input.contains("closeout"));
	assert!(continuation_input.contains("merged PR lineage"));
	assert!(continuation_input.contains("Merge is already authoritative"));
	assert!(continuation_input.contains("Do not land, merge, or request review"));
	assert!(continuation_input.contains("may already be in `Done`"));
	assert!(
		continuation_input
			.contains("either omit `head_sha` or pass the exact full current `HEAD` SHA")
	);
	assert!(continuation_input.contains(
		"If the issue is still in `In Review`, transition it once to `Done` with `issue_transition` before `issue_closeout_complete`"
	));
	assert!(continuation_input.contains("closeout"));

	for prompt in [&developer_instructions, &user_input, &continuation_input] {
		assert_manual_attention_prompt_guidance(prompt, false);
	}
}

#[test]
fn single_turn_prompts_do_not_allow_nonterminal_yield_boundary() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue("Todo", &[]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let issue_run = orchestrator::IssueRunPlan {
		issue,
		issue_state: String::from("In Progress"),
		initial_issue_state: String::from("Todo"),
		worktree: WorktreeSpec {
			branch_name: String::from("x/pubfi-pub-101"),
			issue_identifier: String::from("PUB-101"),
			path: config.worktree_root().join("PUB-101"),
			reused_existing: false,
		},
		retry_project_slug: String::from("pubfi"),
		dispatch_mode: IssueDispatchMode::Normal,
		attempt_number: 1,
		run_id: String::from("pub-101-attempt-1-123"),
		retry_budget_base: 0,
	};
	let developer_instructions = orchestrator::build_developer_instructions(
		&tracker,
		&config,
		&workflow,
		&issue_run,
		&StateStore::open_in_memory().expect("state store should open"),
		None,
	)
	.expect("developer instructions should build");
	let user_input = orchestrator::build_user_input(
		&tracker,
		&config,
		&tests::sample_issue("Todo", &[]),
		&workflow,
		&issue_run,
		&StateStore::open_in_memory().expect("state store should open"),
		None,
	);

	assert!(!developer_instructions.contains("you may end the turn without"));
	assert!(!user_input.contains("you may end the turn without"));
}

#[test]
fn prompts_handle_machine_only_and_text_fenced_tracker_descriptions() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let cases: &[(&str, &str, &[&str])] = &[
		(
			"single json fence",
			"```json\n{\n  \"schema\": \"opaque-pointer/1\",\n  \"id\": \"ptr-1\"\n}\n```",
			&["\"schema\": \"opaque-pointer/1\""],
		),
		(
			"multiple json fences",
			"```json\n{\n  \"schema\": \"opaque-pointer/1\"\n}\n```\n\n```json\n{\n  \"schema\": \"opaque-pointer/2\"\n}\n```",
			&["\"schema\": \"opaque-pointer/1\"", "\"schema\": \"opaque-pointer/2\""],
		),
		(
			"four backtick json fence",
			"````json\n{\n  \"schema\": \"opaque-pointer/1\"\n}\n````",
			&["\"schema\": \"opaque-pointer/1\""],
		),
		(
			"tilde json fence",
			"~~~json\n{\n  \"schema\": \"opaque-pointer/1\"\n}\n~~~",
			&["\"schema\": \"opaque-pointer/1\""],
		),
	];

	for (case_name, description, forbidden_fragments) in cases {
		let mut issue = tests::sample_issue("Todo", &[]);

		issue.description = (*description).to_owned();

		let tracker = FakeTracker::new(vec![issue.clone()]);
		let issue_run = normal_prompt_issue_run(&config, issue.clone());
		let user_input = orchestrator::build_user_input(
			&tracker,
			&config,
			&issue,
			&workflow,
			&issue_run,
			&StateStore::open_in_memory().expect("state store should open"),
			None,
		);

		assert!(
			user_input.contains("machine-only tracker description omitted"),
			"{case_name} should be redacted"
		);

		for forbidden in *forbidden_fragments {
			assert!(!user_input.contains(forbidden), "{case_name} leaked {forbidden}");
		}
	}

	let mut issue = tests::sample_issue("Todo", &[]);
	let tracker = FakeTracker::new(vec![issue.clone()]);

	issue.description =
		String::from("```text\nImplement the retained lane repair and keep scope tight.\n```");

	let issue_run = normal_prompt_issue_run(&config, issue.clone());
	let user_input = orchestrator::build_user_input(
		&tracker,
		&config,
		&issue,
		&workflow,
		&issue_run,
		&StateStore::open_in_memory().expect("state store should open"),
		None,
	);

	assert!(!user_input.contains("machine-only tracker description omitted"));
	assert!(user_input.contains("Implement the retained lane repair and keep scope tight."));
}

fn normal_prompt_issue_run(
	config: &ServiceConfig,
	issue: TrackerIssue,
) -> orchestrator::IssueRunPlan {
	orchestrator::IssueRunPlan {
		issue: issue.clone(),
		issue_state: String::from("In Progress"),
		initial_issue_state: String::from("Todo"),
		worktree: WorktreeSpec {
			branch_name: String::from("x/pubfi-pub-101"),
			issue_identifier: String::from("PUB-101"),
			path: config.worktree_root().join("PUB-101"),
			reused_existing: false,
		},
		retry_project_slug: String::from("pubfi"),
		dispatch_mode: IssueDispatchMode::Normal,
		attempt_number: 1,
		run_id: String::from("pub-101-attempt-1-123"),
		retry_budget_base: 0,
	}
}

#[test]
fn developer_instructions_match_trimmed_prompt_shape() {
	let read_first_files = [
		("docs/index.md", "Use the documentation index.\n"),
		("docs/runbook/index.md", "Use the runbook index.\n"),
	];
	let (_temp_dir, config, workflow) = tests::temp_project_layout_with_read_first(
		&read_first_files,
		"This workflow body should be appended.\n",
	);
	let issue = tests::sample_issue("Todo", &[]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let issue_run = orchestrator::IssueRunPlan {
		issue,
		issue_state: String::from("In Progress"),
		initial_issue_state: String::from("Todo"),
		worktree: WorktreeSpec {
			branch_name: String::from("x/pubfi-pub-101"),
			issue_identifier: String::from("PUB-101"),
			path: config.worktree_root().join("PUB-101"),
			reused_existing: false,
		},
		retry_project_slug: String::from("pubfi"),
		dispatch_mode: IssueDispatchMode::Normal,
		attempt_number: 1,
		run_id: String::from("pub-101-attempt-1-123"),
		retry_budget_base: 0,
	};
	let instructions = orchestrator::build_developer_instructions(
		&tracker,
		&config,
		&workflow,
		&issue_run,
		&StateStore::open_in_memory().expect("state store should open"),
		None,
	)
	.expect("developer instructions should build");

	assert_eq!(
		instructions,
		intake_workflow_reload::expected_developer_instructions(
			&read_first_files,
			&workflow,
			&issue_run
		)
	);
}

#[test]
fn continuation_guard_rejects_first_turn_without_startup_transition() {
	let (_temp_dir, _config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue("Todo", &[]);
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let tracker_tool_bridge = TrackerToolBridge::new(&tracker, &issue, &workflow);
	let guard = IssueTurnContinuationGuard {
		tracker: &tracker,
		tracker_tool_bridge: &tracker_tool_bridge,
		workflow: &workflow,
		service_id: TEST_SERVICE_ID,
		issue_id: &issue.id,
		issue_identifier: &issue.identifier,
		initial_issue_state: &issue.state.name,
		retry_project_slug: issue
			.project_slug
			.as_deref()
			.expect("sample issue should carry a project slug"),
		dispatch_mode: IssueDispatchMode::Normal,
		review_state_inspector: None,
	};
	let error = guard
		.validate_continuation_boundary(1)
		.expect_err("turn 1 should fail if the startup transition never happened");

	assert!(error.to_string().contains("ended without moving the tracker issue to `In Progress`"));
}

#[test]
fn continuation_guard_allows_local_startup_transition_on_stale_rereads() {
	{
		let (_temp_dir, _config, workflow) = tests::temp_project_layout();
		let issue = run_and_prompting_service_owned_issue("Todo");
		let tracker =
			FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
		let tracker_tool_bridge = TrackerToolBridge::new(&tracker, &issue, &workflow);
		let transition_response = DynamicToolHandler::handle_call(
			&tracker_tool_bridge,
			ISSUE_TRANSITION_TOOL_NAME,
			serde_json::json!({ "state": "In Progress" }),
		);

		assert!(transition_response.success);

		let guard = IssueTurnContinuationGuard {
			tracker: &tracker,
			tracker_tool_bridge: &tracker_tool_bridge,
			workflow: &workflow,
			service_id: TEST_SERVICE_ID,
			issue_id: &issue.id,
			issue_identifier: &issue.identifier,
			initial_issue_state: &issue.state.name,
			retry_project_slug: issue
				.project_slug
				.as_deref()
				.expect("sample issue should carry a project slug"),
			dispatch_mode: IssueDispatchMode::Normal,
			review_state_inspector: None,
		};

		assert!(
			guard
				.should_continue_turn(1)
				.expect("a stale pre-write reread should not block turn-one continuation")
		);

		guard.validate_continuation_boundary(1).expect(
			"a stale pre-write reread should not hard-fail turn one after a local startup transition",
		);
	}
	{
		let (_temp_dir, _config, workflow) = tests::temp_project_layout();
		let issue = run_and_prompting_service_owned_issue("Todo");
		let tracker = FakeTracker::with_refresh_snapshots(
			vec![issue.clone()],
			vec![vec![issue.clone()], vec![issue.clone()]],
		);
		let tracker_tool_bridge = TrackerToolBridge::new(&tracker, &issue, &workflow);
		let transition_response = DynamicToolHandler::handle_call(
			&tracker_tool_bridge,
			ISSUE_TRANSITION_TOOL_NAME,
			serde_json::json!({ "state": "In Progress" }),
		);

		assert!(transition_response.success);

		let guard = IssueTurnContinuationGuard {
			tracker: &tracker,
			tracker_tool_bridge: &tracker_tool_bridge,
			workflow: &workflow,
			service_id: TEST_SERVICE_ID,
			issue_id: &issue.id,
			issue_identifier: &issue.identifier,
			initial_issue_state: &issue.state.name,
			retry_project_slug: issue
				.project_slug
				.as_deref()
				.expect("sample issue should carry a project slug"),
			dispatch_mode: IssueDispatchMode::Normal,
			review_state_inspector: None,
		};

		assert!(
			guard
				.should_continue_turn(1)
				.expect("a stale pre-write reread should not block turn-one continuation")
		);
		assert!(
			guard
				.should_continue_turn(2)
				.expect("a stale pre-write reread should remain tolerated after turn one")
		);
	}
}

#[test]
fn continuation_guard_allows_review_repair_continuation_while_issue_remains_in_review() {
	let (_temp_dir, _config, workflow) = tests::temp_project_layout();
	let issue = run_and_prompting_service_owned_issue("In Review");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let tracker_tool_bridge = TrackerToolBridge::new(&tracker, &issue, &workflow);
	let guard = IssueTurnContinuationGuard {
		tracker: &tracker,
		tracker_tool_bridge: &tracker_tool_bridge,
		workflow: &workflow,
		service_id: TEST_SERVICE_ID,
		issue_id: &issue.id,
		issue_identifier: &issue.identifier,
		initial_issue_state: &issue.state.name,
		retry_project_slug: issue
			.project_slug
			.as_deref()
			.expect("sample issue should carry a project slug"),
		dispatch_mode: IssueDispatchMode::ReviewRepair,
		review_state_inspector: None,
	};

	assert!(
		guard
			.should_continue_turn(2)
			.expect("retained review-repair lane should continue while issue remains in review")
	);

	guard.validate_continuation_boundary(2).expect(
		"review-repair continuation boundary should stay valid while the issue remains in review",
	);
}

#[test]
fn continuation_guard_allows_closeout_continuation_after_issue_reaches_completed_state() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = run_and_prompting_service_owned_issue("Done");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree =
		worktree_manager.ensure_worktree(&issue.identifier, false).expect("worktree should exist");
	let head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/175";

	tests::seed_review_handoff_marker_value(
		&state_store,
		config.service_id(),
		&issue.id,
		&tests::sample_review_handoff_marker(&worktree.branch_name, pr_url, &head_oid),
	);

	let tracker_tool_bridge = TrackerToolBridge::with_run_context_and_state_store(
		&tracker,
		&issue,
		&workflow,
		ReviewHandoffContext {
			attempt_number: 1,
			branch_name: worktree.branch_name.clone(),
			run_id: String::from("run-closeout-continuation"),
			service_id: String::from(TEST_SERVICE_ID),
			worktree_path: worktree.path.display().to_string(),
			cwd: worktree.path.clone(),
			github_token_env_var: None,
			github_command_path: None,
			review_level: ReviewLevel::Strict,
			mode: ReviewExecutionMode::Closeout,
			recorded_pr_url: Some(String::from(pr_url)),
		},
		&state_store,
	);
	let mut review_state = tests::sample_pull_request_review_state(
		pr_url,
		&worktree.branch_name,
		&head_oid,
		Some("APPROVED"),
		"MERGEABLE",
		"CLEAN",
		Some("SUCCESS"),
		0,
	);

	review_state.state = String::from("MERGED");

	let review_state_inspector =
		FakePullRequestReviewStateInspector::new(vec![Ok(review_state.clone()), Ok(review_state)]);
	let guard = IssueTurnContinuationGuard {
		tracker: &tracker,
		tracker_tool_bridge: &tracker_tool_bridge,
		workflow: &workflow,
		service_id: TEST_SERVICE_ID,
		issue_id: &issue.id,
		issue_identifier: &issue.identifier,
		initial_issue_state: "In Review",
		retry_project_slug: issue
			.project_slug
			.as_deref()
			.expect("sample issue should carry a project slug"),
		dispatch_mode: IssueDispatchMode::Closeout,
		review_state_inspector: Some(&review_state_inspector),
	};

	assert!(
		guard
			.should_continue_turn(2)
			.expect("retained closeout lane should continue while the issue remains completed")
	);

	guard
		.validate_continuation_boundary(2)
		.expect("closeout continuation boundary should stay valid after tracker completion");
}

#[test]
fn continuation_guard_blocks_closeout_continuation_when_completed_issue_pr_is_open() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = run_and_prompting_service_owned_issue("Done");
	let tracker = FakeTracker::with_refresh_snapshots(
		vec![issue.clone()],
		vec![vec![issue.clone()], vec![issue.clone()]],
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree =
		worktree_manager.ensure_worktree(&issue.identifier, false).expect("worktree should exist");
	let head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/176";

	tests::seed_review_handoff_marker_value(
		&state_store,
		config.service_id(),
		&issue.id,
		&tests::sample_review_handoff_marker(&worktree.branch_name, pr_url, &head_oid),
	);

	let tracker_tool_bridge = TrackerToolBridge::with_run_context_and_state_store(
		&tracker,
		&issue,
		&workflow,
		ReviewHandoffContext {
			attempt_number: 1,
			branch_name: worktree.branch_name.clone(),
			run_id: String::from("run-closeout-open-pr"),
			service_id: String::from(TEST_SERVICE_ID),
			worktree_path: worktree.path.display().to_string(),
			cwd: worktree.path.clone(),
			github_token_env_var: None,
			github_command_path: None,
			review_level: ReviewLevel::Strict,
			mode: ReviewExecutionMode::Closeout,
			recorded_pr_url: Some(String::from(pr_url)),
		},
		&state_store,
	);
	let review_state = tests::sample_pull_request_review_state(
		pr_url,
		&worktree.branch_name,
		&head_oid,
		Some("APPROVED"),
		"MERGEABLE",
		"CLEAN",
		Some("SUCCESS"),
		0,
	);
	let review_state_inspector =
		FakePullRequestReviewStateInspector::new(vec![Ok(review_state.clone()), Ok(review_state)]);
	let guard = IssueTurnContinuationGuard {
		tracker: &tracker,
		tracker_tool_bridge: &tracker_tool_bridge,
		workflow: &workflow,
		service_id: TEST_SERVICE_ID,
		issue_id: &issue.id,
		issue_identifier: &issue.identifier,
		initial_issue_state: "In Review",
		retry_project_slug: issue
			.project_slug
			.as_deref()
			.expect("sample issue should carry a project slug"),
		dispatch_mode: IssueDispatchMode::Closeout,
		review_state_inspector: Some(&review_state_inspector),
	};

	assert!(
		!guard.should_continue_turn(2).expect(
			"completed issues must not continue retained closeout while the PR is still open"
		)
	);

	let error = guard
		.validate_continuation_boundary(2)
		.expect_err("completed issues with open PRs must fail the retained closeout boundary");

	assert!(error.to_string().contains("retained closeout continuation boundary"));
}

#[test]
fn continuation_guard_errors_when_completed_issue_pr_state_cannot_be_read() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = run_and_prompting_service_owned_issue("Done");
	let tracker = FakeTracker::with_refresh_snapshots(
		vec![issue.clone()],
		vec![vec![issue.clone()], vec![issue.clone()]],
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree =
		worktree_manager.ensure_worktree(&issue.identifier, false).expect("worktree should exist");
	let head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/177";

	tests::seed_review_handoff_marker_value(
		&state_store,
		config.service_id(),
		&issue.id,
		&tests::sample_review_handoff_marker(&worktree.branch_name, pr_url, &head_oid),
	);

	let tracker_tool_bridge = TrackerToolBridge::with_run_context_and_state_store(
		&tracker,
		&issue,
		&workflow,
		ReviewHandoffContext {
			attempt_number: 1,
			branch_name: worktree.branch_name.clone(),
			run_id: String::from("run-closeout-read-failed"),
			service_id: String::from(TEST_SERVICE_ID),
			worktree_path: worktree.path.display().to_string(),
			cwd: worktree.path.clone(),
			github_token_env_var: None,
			github_command_path: None,
			review_level: ReviewLevel::Strict,
			mode: ReviewExecutionMode::Closeout,
			recorded_pr_url: Some(String::from(pr_url)),
		},
		&state_store,
	);
	let review_state_inspector = FakePullRequestReviewStateInspector::new(vec![
		Err(color_eyre::eyre::eyre!("gh api failed")),
		Err(color_eyre::eyre::eyre!("gh api failed")),
	]);
	let guard = IssueTurnContinuationGuard {
		tracker: &tracker,
		tracker_tool_bridge: &tracker_tool_bridge,
		workflow: &workflow,
		service_id: TEST_SERVICE_ID,
		issue_id: &issue.id,
		issue_identifier: &issue.identifier,
		initial_issue_state: "In Review",
		retry_project_slug: issue
			.project_slug
			.as_deref()
			.expect("sample issue should carry a project slug"),
		dispatch_mode: IssueDispatchMode::Closeout,
		review_state_inspector: Some(&review_state_inspector),
	};
	let continue_error = guard.should_continue_turn(2).expect_err(
		"GH state read failures must not degrade to a silent completed-state closeout skip",
	);

	assert!(continue_error.to_string().contains("PR state read failed"));

	let boundary_error = guard
		.validate_continuation_boundary(2)
		.expect_err("GH state read failures must fail the retained closeout boundary explicitly");

	assert!(boundary_error.to_string().contains("PR state read failed"));
}

#[test]
fn continuation_guard_preserves_original_startable_state_across_continuation_retries() {
	let (_temp_dir, _config, workflow) = tests::temp_project_layout();
	let issue = run_and_prompting_service_owned_issue("In Progress");
	let stale_issue = run_and_prompting_service_owned_issue("Todo");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![stale_issue.clone()]]);
	let tracker_tool_bridge = TrackerToolBridge::new(&tracker, &issue, &workflow);
	let guard = IssueTurnContinuationGuard {
		tracker: &tracker,
		tracker_tool_bridge: &tracker_tool_bridge,
		workflow: &workflow,
		service_id: TEST_SERVICE_ID,
		issue_id: &issue.id,
		issue_identifier: &issue.identifier,
		initial_issue_state: "Todo",
		retry_project_slug: issue
			.project_slug
			.as_deref()
			.expect("sample issue should carry a project slug"),
		dispatch_mode: IssueDispatchMode::Normal,
		review_state_inspector: None,
	};

	assert!(
		guard
			.should_continue_turn(2)
			.expect("continuation retries must preserve the original startable state even after a refreshed in-progress run plan")
	);
}

#[test]
fn continuation_guard_stops_when_service_active_label_is_removed() {
	let (_temp_dir, _config, workflow) = tests::temp_project_layout();
	let issue = run_and_prompting_service_owned_issue("In Progress");
	let tracker = FakeTracker::with_refresh_snapshots(
		vec![issue.clone()],
		vec![vec![tests::sample_issue("In Progress", &[])]],
	);
	let tracker_tool_bridge = TrackerToolBridge::new(&tracker, &issue, &workflow);
	let guard = IssueTurnContinuationGuard {
		tracker: &tracker,
		tracker_tool_bridge: &tracker_tool_bridge,
		workflow: &workflow,
		service_id: TEST_SERVICE_ID,
		issue_id: &issue.id,
		issue_identifier: &issue.identifier,
		initial_issue_state: &issue.state.name,
		retry_project_slug: issue
			.project_slug
			.as_deref()
			.expect("sample issue should carry a project slug"),
		dispatch_mode: IssueDispatchMode::Normal,
		review_state_inspector: None,
	};

	assert!(
		!guard
			.should_continue_turn(2)
			.expect("continuation must stop once service ownership is removed"),
	);
}
