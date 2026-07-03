use std::{
	ffi::{OsStr, OsString},
	fs,
	path::Path,
};

use color_eyre::Report;
use serde::Deserialize;

use crate::{
	agent::PhaseGoalController,
	orchestrator::{
		self, AppServerCapabilityPreflightFailure, EvidenceRequest, IssueDispatchMode,
		IssueRunPlan, ManualAttentionRequested, PHASE_ACCEPTANCE_CHECK_EVENT_TYPE,
		PHASE_GOAL_RECOVERY_BLOCKED_EVENT_TYPE, PHASE_GOAL_RECOVERY_EVENT_TYPE, PhaseGoalKind,
		PhaseGoalSpec, PhaseGoalTransition, RepoGateFailure, RepoGatePhaseGoalController,
		ServiceConfig, StateStore, execution_phase_goal, tests, tests::TEST_SERVICE_ID,
	},
	tracker::{self, TrackerIssue},
	worktree::WorktreeSpec,
};

pub(super) fn record_phase_acceptance_progress_checkpoint(
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

#[test]
fn repo_gate_rejects_dirty_tracked_files_left_by_canonicalize_commands() {
	let (_temp_dir, config, _workflow) = tests::temp_project_layout();
	let repo_root = config.repo_root();

	tests::commit_worktree_change(repo_root, "tracked.txt", "before\n", "add tracked file");

	let error = orchestrator::run_repo_gate_commands(
		&[String::from("printf 'after\\n' > tracked.txt")],
		&[String::from("grep -qx 'after' tracked.txt")],
		repo_root,
	)
	.expect_err("tracked autofix rewrites should fail the repo gate");
	let tracked_contents = fs::read_to_string(repo_root.join("tracked.txt"))
		.expect("tracked file should remain readable");
	let tracked_status =
		tests::git_output(repo_root, &["status", "--porcelain", "--untracked-files=no"]);
	let repo_gate_failure = error
		.downcast_ref::<RepoGateFailure>()
		.expect("repo gate failures should preserve structured classification");

	assert!(error.to_string().contains("verification"));
	assert_eq!(repo_gate_failure.error_class(), "repo_gate_tracked_rewrites_left");
	assert_eq!(
		repo_gate_failure.disposition(),
		orchestrator::RepoGateFailureDisposition::NeedsHumanAttention
	);
	assert_eq!(tracked_contents, "after\n");
	assert!(tracked_status.contains("tracked.txt"));
}

#[test]
fn repo_gate_stops_canonicalize_failure_after_scope_envelope_widening() {
	let (_temp_dir, config, _workflow) = tests::temp_project_layout();
	let repo_root = config.repo_root();

	tests::commit_worktree_change(repo_root, "owned.txt", "before\n", "add owned file");
	tests::commit_worktree_change(repo_root, "outside.txt", "before\n", "add outside file");
	fs::write(repo_root.join("owned.txt"), "implementation\n")
		.expect("pre-gate implementation diff should write");

	let error = orchestrator::run_repo_gate_commands(
		&[String::from("printf 'rewritten\\n' > outside.txt; exit 1")],
		&[],
		repo_root,
	)
	.expect_err("canonicalize widening should stop before ordinary repair retry");
	let repo_gate_failure = error
		.downcast_ref::<RepoGateFailure>()
		.expect("scope envelope violation should preserve repo-gate classification");

	assert_eq!(repo_gate_failure.error_class(), "repo_gate_scope_envelope_violation");
	assert_eq!(
		repo_gate_failure.disposition(),
		orchestrator::RepoGateFailureDisposition::NeedsHumanAttention
	);
	assert!(
		error.to_string().contains("outside.txt"),
		"error should name the out-of-scope rewrite"
	);
}

#[test]
fn repo_gate_stops_verify_failure_after_scope_envelope_widening() {
	let (_temp_dir, config, _workflow) = tests::temp_project_layout();
	let repo_root = config.repo_root();

	tests::commit_worktree_change(repo_root, "owned.txt", "before\n", "add owned file");
	tests::commit_worktree_change(repo_root, "outside.txt", "before\n", "add outside file");
	fs::write(repo_root.join("owned.txt"), "implementation\n")
		.expect("pre-gate implementation diff should write");

	let error = orchestrator::run_repo_gate_commands(
		&[],
		&[String::from("printf 'rewritten\\n' > outside.txt; exit 1")],
		repo_root,
	)
	.expect_err("verify widening should stop before ordinary repair retry");
	let repo_gate_failure = error
		.downcast_ref::<RepoGateFailure>()
		.expect("scope envelope violation should preserve repo-gate classification");

	assert_eq!(repo_gate_failure.error_class(), "repo_gate_scope_envelope_violation");
	assert_eq!(
		repo_gate_failure.disposition(),
		orchestrator::RepoGateFailureDisposition::NeedsHumanAttention
	);
	assert!(
		error.to_string().contains("outside.txt"),
		"error should name the out-of-scope rewrite"
	);
}

#[test]
fn completion_repo_gate_records_lane_decision_for_scope_envelope_violation() {
	let failing_verify = "printf 'rewritten\\n' > outside.txt; exit 1";
	let workflow_markdown =
		tests::sample_workflow_markdown("pubfi", &[], "Completion gate policy.\n", 1).replace(
			"verify_commands = []",
			&format!(
				"verify_commands = [{}]",
				serde_json::to_string(failing_verify).expect("command should serialize")
			),
		);
	let (_temp_dir, config, workflow) =
		tests::temp_project_layout_with_workflow_markdown(&workflow_markdown);
	let issue = tests::sample_issue(
		"In Progress",
		&[tracker::automation_active_label(TEST_SERVICE_ID).as_str()],
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue_run = phase_goal_repo_gate_issue_run(&config, &issue);

	tests::commit_worktree_change(config.repo_root(), "owned.txt", "before\n", "add owned file");
	tests::commit_worktree_change(
		config.repo_root(),
		"outside.txt",
		"before\n",
		"add outside file",
	);
	fs::write(config.repo_root().join("owned.txt"), "implementation\n")
		.expect("pre-gate implementation diff should write");

	let error = orchestrator::run_completion_repo_gate(
		&config,
		&workflow,
		&state_store,
		&issue_run,
		PhaseGoalKind::HandoffEvidence,
	)
	.expect_err("completion repo-gate scope violation should stop");
	let repo_gate_failure = error
		.downcast_ref::<RepoGateFailure>()
		.expect("scope envelope violation should preserve repo-gate classification");
	let events = state_store
		.list_private_execution_events(TEST_SERVICE_ID, &issue.id, &issue_run.run_id, 1)
		.expect("private lane decision events should load");

	assert_eq!(repo_gate_failure.error_class(), "repo_gate_scope_envelope_violation");

	let decision = repo_gate_failure
		.tracked_rewrite_decision()
		.expect("scope envelope violation should retain rewrite decision");
	let decision_json = decision.to_json();

	assert_eq!(decision_json["sourceErrorClass"], "repo_gate_verify_failed");
	assert_eq!(decision_json["sourceRepoGateFailure"]["stage"], "verify");
	assert!(events.iter().any(|event| {
		event.event_type() == "lane_decision"
			&& event.payload()["next_action"] == "needs_attention"
			&& event.payload()["repo_gate_disposition"] == "needs_human_attention"
			&& event.payload()["scope_envelope_violation"] == true
	}));
}

#[test]
fn repo_gate_allows_existing_tracked_diff_when_commands_preserve_it() {
	let (_temp_dir, config, _workflow) = tests::temp_project_layout();
	let repo_root = config.repo_root();

	tests::commit_worktree_change(repo_root, "tracked.txt", "before\n", "add tracked file");
	fs::write(repo_root.join("tracked.txt"), "after\n")
		.expect("tracked implementation diff should write");
	orchestrator::run_repo_gate_commands(
		&[],
		&[String::from("grep -qx 'after' tracked.txt")],
		repo_root,
	)
	.expect("repo gate should allow an existing implementation diff");
}

#[test]
fn repo_gate_cleanliness_check_spawn_failures_require_human_attention() {
	let (_temp_dir, config, _workflow) = tests::temp_project_layout();
	let repo_root = config.repo_root();
	let error = orchestrator::run_repo_gate_cleanliness_check_with_git(
		OsStr::new("/definitely-missing-git-for-tests"),
		repo_root,
	)
	.expect_err("missing git binary should preserve repo gate classification");
	let repo_gate_failure = error
		.downcast_ref::<RepoGateFailure>()
		.expect("repo gate failures should preserve structured classification");

	assert!(error.to_string().contains("tracked-file cleanliness check"));
	assert_eq!(repo_gate_failure.error_class(), "repo_gate_command_spawn_failed");
	assert_eq!(
		repo_gate_failure.disposition(),
		orchestrator::RepoGateFailureDisposition::NeedsHumanAttention
	);
}

#[test]
fn repo_gate_classifies_git_index_lock_contention_as_retryable_runtime_failure() {
	let (_temp_dir, config, _workflow) = tests::temp_project_layout();
	let repo_root = config.repo_root();
	let error = orchestrator::run_repo_gate_commands(
		&[String::from(
			"printf \"%s\\n\" \"fatal: Unable to create '.git/index.lock': File exists.\" >&2; exit 1",
		)],
		&[],
		repo_root,
	)
	.expect_err("git index.lock contention should fail the repo gate");
	let repo_gate_failure = error
		.downcast_ref::<RepoGateFailure>()
		.expect("repo gate failures should preserve structured classification");

	assert_eq!(repo_gate_failure.error_class(), "repo_gate_git_lock_contention");
	assert_eq!(
		repo_gate_failure.disposition(),
		orchestrator::RepoGateFailureDisposition::RetryAfterBackoff
	);
}

#[test]
fn repo_gate_selects_matching_profile_for_scoped_lane_changes() {
	let (temp_dir, config, workflow) = tests::temp_project_layout_with_workflow_markdown(
		&tests::profile_scoped_workflow_markdown("pubfi"),
	);
	let repo_root = config.repo_root();
	let remote_root = temp_dir.path().join("origin.git");

	tests::add_origin_remote(repo_root, &remote_root);
	tests::checkout_new_branch(repo_root, "config-subset");
	tests::commit_worktree_change(
		repo_root,
		"config/new-surface.toml",
		"name = \"new-surface\"\n",
		"config subset change",
	);

	let selection =
		orchestrator::select_repo_gate_for_worktree(workflow.frontmatter().execution(), repo_root);

	assert_eq!(selection.profile_name(), Some("config_subset"));
	assert!(selection.canonicalize_commands().is_empty());
	assert_eq!(selection.verify_commands(), ["python3 -c 'print(\"ok\")'"]);
}

#[test]
fn repo_gate_falls_back_to_full_gate_when_changed_file_classification_is_unavailable() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout_with_workflow_markdown(
		&tests::profile_scoped_workflow_markdown("pubfi"),
	);
	let repo_root = config.repo_root();

	tests::checkout_new_branch(repo_root, "config-subset");
	tests::commit_worktree_change(
		repo_root,
		"config/new-surface.toml",
		"name = \"new-surface\"\n",
		"config subset change",
	);

	let selection =
		orchestrator::select_repo_gate_for_worktree(workflow.frontmatter().execution(), repo_root);

	assert_eq!(selection.profile_name(), None);
	assert_eq!(selection.canonicalize_commands(), ["cargo make fmt", "cargo make lint-fix"]);
	assert_eq!(selection.verify_commands(), ["cargo make check"]);
}

#[test]
fn phase_goal_completion_runs_repo_gate_and_persists_handoff_phase() {
	let workflow_markdown = tests::sample_workflow_markdown(
		"pubfi",
		&[],
		"Phase goal validation policy.\n",
		3,
	)
	.replace(
		"canonicalize_commands = []",
		"canonicalize_commands = [\"printf canonicalized > phase-canonicalized.txt\"]",
	)
	.replace(
		"verify_commands = []",
		"verify_commands = [\"test -f phase-canonicalized.txt && printf verified > phase-verified.txt\"]",
	);
	let (_temp_dir, config, workflow) =
		tests::temp_project_layout_with_workflow_markdown(&workflow_markdown);
	let issue = tests::sample_issue(
		"In Progress",
		&[tracker::automation_active_label(TEST_SERVICE_ID).as_str()],
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue_run = tests::loop_guardrail_issue_run(&config, &issue, 1);

	tests::commit_worktree_change(config.repo_root(), "ready.txt", "before\n", "add ready file");
	fs::write(config.repo_root().join("ready.txt"), "after\n").expect("tracked diff should write");

	record_phase_acceptance_progress_checkpoint(&config, &state_store, &issue_run, &[]);

	let controller = RepoGatePhaseGoalController {
		project: &config,
		workflow: &workflow,
		state_store: &state_store,
		issue_run: &issue_run,
	};
	let transition = controller
		.phase_goal_completed(PhaseGoalKind::ImplementToValidationReady)
		.expect("completed implementation phase should run the repo gate");
	let events = state_store
		.list_private_execution_events(TEST_SERVICE_ID, &issue.id, &issue_run.run_id, 1)
		.expect("private phase goal events should load");

	assert!(config.repo_root().join("phase-canonicalized.txt").exists());
	assert!(config.repo_root().join("phase-verified.txt").exists());
	assert!(matches!(
		transition,
		PhaseGoalTransition::Continue(PhaseGoalSpec { phase: PhaseGoalKind::HandoffEvidence, .. })
	));
	assert!(events.iter().any(|event| {
		event.event_type() == "phase_goal_transition"
			&& event.payload()["signal"] == "validation_pass"
	}));
	assert!(events.iter().any(|event| {
		event.event_type() == "phase_goal_next" && event.payload()["phase"] == "handoff_evidence"
	}));
}

#[test]
fn phase_goal_repo_gate_failure_records_structured_diagnostic() {
	let failing_command = "printf 'error: function has too many lines\\n --> apps/decodex/src/mcp.rs:12:1\\nfn mcp_tools() {}\\n' >&2; exit 1";
	let workflow_markdown =
		tests::sample_workflow_markdown("pubfi", &[], "Phase goal validation policy.\n", 3)
			.replace(
				"canonicalize_commands = []",
				&format!(
					"canonicalize_commands = [{}]",
					serde_json::to_string(failing_command).expect("command should serialize")
				),
			);
	let (_temp_dir, config, workflow) =
		tests::temp_project_layout_with_workflow_markdown(&workflow_markdown);
	let issue = tests::sample_issue(
		"In Progress",
		&[tracker::automation_active_label(TEST_SERVICE_ID).as_str()],
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue_run = phase_goal_repo_gate_issue_run(&config, &issue);
	let transition = RepoGatePhaseGoalController {
		project: &config,
		workflow: &workflow,
		state_store: &state_store,
		issue_run: &issue_run,
	}
	.phase_goal_completed(PhaseGoalKind::ImplementToValidationReady)
	.expect("repo gate failure should continue to repair phase");
	let events = state_store
		.list_private_execution_events(TEST_SERVICE_ID, &issue.id, &issue_run.run_id, 1)
		.expect("private phase goal events should load");

	match transition {
		PhaseGoalTransition::Continue(PhaseGoalSpec {
			phase: PhaseGoalKind::RepairValidationFailures,
			objective,
			..
		}) => {
			assert!(objective.contains("Failed repo-gate command"));
			assert!(objective.contains("function has too many lines"));
			assert!(objective.contains("apps/decodex/src/mcp.rs"));
		},
		_ => panic!("repo gate failure should continue to repair validation failures"),
	}

	let transition_event = events
		.iter()
		.find(|event| {
			event.event_type() == "phase_goal_transition"
				&& event.payload()["signal"] == "validation_fail"
		})
		.expect("validation failure transition should record");
	let diagnostic = &transition_event.payload()["payload"]["repoGateFailure"];

	assert_eq!(diagnostic["stage"], "canonicalize");
	assert_eq!(diagnostic["failed_command"], failing_command);
	assert_eq!(diagnostic["exit_status"], 1);
	assert!(diagnostic["summary"].as_str().is_some_and(|summary| {
		summary.contains("repo gate canonicalize command") && summary.contains("too many lines")
	}));
	assert!(diagnostic["problem_lines"].as_array().is_some_and(|lines| {
		lines.iter().any(|line| line.as_str().is_some_and(|line| line.contains("mcp.rs")))
			&& lines.iter().any(|line| line.as_str().is_some_and(|line| line.contains("mcp_tools")))
	}));

	let guardrail_event = events
		.iter()
		.find(|event| event.event_type() == "loop_guardrail_checkpoint")
		.expect("guardrail checkpoint should record diagnostic details");

	#[derive(Deserialize)]
	struct GuardrailDetails {
		repo_gate_failure: GuardrailRepoGateFailure,
	}

	#[derive(Deserialize)]
	struct GuardrailRepoGateFailure {
		stage: String,
		failed_command: String,
	}

	let guardrail_details: GuardrailDetails = serde_json::from_str(
		guardrail_event.payload()["details"]
			.as_str()
			.expect("guardrail details should be json string"),
	)
	.expect("guardrail details should parse");

	assert_eq!(guardrail_details.repo_gate_failure.stage, "canonicalize");
	assert_eq!(guardrail_details.repo_gate_failure.failed_command, failing_command);

	let request = EvidenceRequest {
		config_path: None,
		project_id: None,
		issue: &issue.id,
		run_id: Some(&issue_run.run_id),
		attempt_number: Some(1),
		json: true,
		include_payload: false,
	};
	let readback = orchestrator::build_private_evidence_readback(&state_store, &config, &request)
		.expect("private evidence should read");

	assert!(readback.repo_gate_failures.iter().any(|failure| {
		failure.error_class == "repo_gate_canonicalize_failed"
			&& failure.failed_command.as_deref() == Some(failing_command)
			&& failure.problem_lines.iter().any(|line| line.contains("mcp_tools"))
	}));
	assert!(
		orchestrator::render_private_evidence_readback(&readback).contains("Repo Gate Failures")
	);
}

fn phase_goal_repo_gate_issue_run(config: &ServiceConfig, issue: &TrackerIssue) -> IssueRunPlan {
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

fn review_repair_phase_goal_issue_run(
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

#[test]
fn review_repair_phase_goal_validation_passes_to_review_repair_evidence() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue("In Review", &[]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue_run = review_repair_phase_goal_issue_run(&config, &issue);

	tests::commit_worktree_change(config.repo_root(), "ready.txt", "before\n", "add ready file");
	fs::write(config.repo_root().join("ready.txt"), "after\n").expect("tracked diff should write");

	record_phase_acceptance_progress_checkpoint(&config, &state_store, &issue_run, &[]);

	let transition = RepoGatePhaseGoalController {
		project: &config,
		workflow: &workflow,
		state_store: &state_store,
		issue_run: &issue_run,
	}
	.phase_goal_completed(PhaseGoalKind::RepairAcceptedReviewFindings)
	.expect("validated review repair should continue to review-repair evidence");
	let events = state_store
		.list_private_execution_events(TEST_SERVICE_ID, &issue.id, &issue_run.run_id, 3)
		.expect("private phase goal events should load");

	match transition {
		PhaseGoalTransition::Continue(PhaseGoalSpec {
			phase: PhaseGoalKind::ReviewRepairEvidence,
			objective,
			..
		}) => {
			assert!(objective.contains("push the current repaired branch"));
			assert!(objective.contains("re-read the PR remote head and mergeability"));
			assert!(objective.contains("issue_review_repair_complete"));
			assert!(objective.contains("review_repair"));
			assert!(objective.contains("Do not call `issue_review_handoff`"));
		},
		_ => panic!("validated review repair should continue to review-repair evidence"),
	}

	assert!(events.iter().any(|event| {
		event.event_type() == "phase_goal_transition"
			&& event.payload()["signal"] == "validation_pass"
			&& event.payload()["payload"]["nextPhase"] == "review_repair_evidence"
	}));
	assert!(events.iter().any(|event| {
		event.event_type() == "phase_goal_next"
			&& event.payload()["phase"] == "review_repair_evidence"
	}));
	assert!(events.iter().all(|event| {
		event.event_type() != "phase_goal_next" || event.payload()["phase"] != "handoff_evidence"
	}));
}

#[test]
fn phase_goal_completion_continues_with_owned_tracked_rewrites_after_validation() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout_with_workflow_markdown(
		&tests::sample_workflow_markdown("pubfi", &[], "Phase goal validation policy.\n", 3)
			.replace(
				"canonicalize_commands = []",
				"canonicalize_commands = [\"printf 'rewritten\\\\n' > ready.txt\"]",
			)
			.replace(
				"verify_commands = []",
				"verify_commands = [\"grep -qx rewritten ready.txt\"]",
			),
	);
	let issue = tests::sample_issue(
		"In Progress",
		&[tracker::automation_active_label(TEST_SERVICE_ID).as_str()],
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue_run = IssueRunPlan {
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
	};

	tests::commit_worktree_change(config.repo_root(), "ready.txt", "before\n", "add ready file");
	fs::write(config.repo_root().join("ready.txt"), "after\n").expect("tracked diff should write");

	record_phase_acceptance_progress_checkpoint(&config, &state_store, &issue_run, &[]);

	let transition = RepoGatePhaseGoalController {
		project: &config,
		workflow: &workflow,
		state_store: &state_store,
		issue_run: &issue_run,
	}
	.phase_goal_completed(PhaseGoalKind::ImplementToValidationReady)
	.expect("owned tracked canonicalize rewrites should satisfy phase validation");
	let events = state_store
		.list_private_execution_events(TEST_SERVICE_ID, &issue.id, &issue_run.run_id, 1)
		.expect("private phase goal events should load");

	match transition {
		PhaseGoalTransition::Continue(PhaseGoalSpec {
			phase: PhaseGoalKind::HandoffEvidence,
			objective,
			..
		}) => {
			assert!(objective.contains("ready.txt"));
			assert!(objective.contains("Commit these issue-owned gate rewrites"));
		},
		_ => panic!("owned tracked rewrites should continue to handoff evidence"),
	}

	assert!(events.iter().any(|event| {
		event.event_type() == "phase_goal_transition"
			&& event.payload()["signal"] == "validation_pass"
			&& event.payload()["payload"]["trackedRewrites"]["owned"] == true
			&& event.payload()["payload"]["trackedRewrites"]["decision"]
				== "continue_to_commit_capable_phase"
	}));
	assert!(events.iter().any(|event| {
		event.event_type() == PHASE_ACCEPTANCE_CHECK_EVENT_TYPE
			&& event.payload()["decision"] == "pass"
			&& event.payload()["validation_evidence"]["tracked_rewrites"]["files"]
				.as_array()
				.is_some_and(|files| files.iter().any(|file| file.as_str() == Some("ready.txt")))
	}));
}

#[test]
fn phase_goal_acceptance_accepts_committed_branch_delta_with_clean_worktree() {
	let (temp_dir, config, workflow) = tests::temp_project_layout();
	let remote_root = temp_dir.path().join("origin.git");
	let issue = tests::sample_issue(
		"In Progress",
		&[tracker::automation_active_label(TEST_SERVICE_ID).as_str()],
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue_run = IssueRunPlan {
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
	};

	tests::add_origin_remote(config.repo_root(), &remote_root);
	tests::checkout_new_branch(config.repo_root(), &issue_run.worktree.branch_name);
	tests::commit_worktree_change(
		config.repo_root(),
		"ready.txt",
		"implementation complete\n",
		"implement issue scope",
	);

	assert_eq!(tests::git_output(config.repo_root(), &["status", "--porcelain"]), "");

	record_phase_acceptance_progress_checkpoint(&config, &state_store, &issue_run, &[]);

	let transition = RepoGatePhaseGoalController {
		project: &config,
		workflow: &workflow,
		state_store: &state_store,
		issue_run: &issue_run,
	}
	.phase_goal_completed(PhaseGoalKind::ImplementToValidationReady)
	.expect("clean committed branch delta should satisfy phase acceptance");
	let events = state_store
		.list_private_execution_events(TEST_SERVICE_ID, &issue.id, &issue_run.run_id, 1)
		.expect("private phase goal events should load");

	assert!(matches!(
		transition,
		PhaseGoalTransition::Continue(PhaseGoalSpec { phase: PhaseGoalKind::HandoffEvidence, .. })
	));
	assert!(events.iter().any(|event| {
		event.event_type() == PHASE_ACCEPTANCE_CHECK_EVENT_TYPE
			&& event.payload()["decision"] == "pass"
			&& event.payload()["reason_code"] == "accepted"
			&& event.payload()["effective_delta"]["present"] == true
			&& event.payload()["effective_delta"]["changed_surfaces"].as_array().is_some_and(
				|surfaces| surfaces.iter().any(|surface| surface.as_str() == Some("ready.txt")),
			)
	}));
}

#[test]
fn phase_goal_acceptance_rejects_repo_gate_pass_without_effective_delta() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue(
		"In Progress",
		&[tracker::automation_active_label(TEST_SERVICE_ID).as_str()],
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue_run = IssueRunPlan {
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
	};

	record_phase_acceptance_progress_checkpoint(&config, &state_store, &issue_run, &[]);

	let transition = RepoGatePhaseGoalController {
		project: &config,
		workflow: &workflow,
		state_store: &state_store,
		issue_run: &issue_run,
	}
	.phase_goal_completed(PhaseGoalKind::ImplementToValidationReady)
	.expect("repo gate pass should still record acceptance failure");
	let events = state_store
		.list_private_execution_events(TEST_SERVICE_ID, &issue.id, &issue_run.run_id, 1)
		.expect("private phase goal events should load");

	assert!(matches!(
		transition,
		PhaseGoalTransition::Continue(PhaseGoalSpec {
			phase: PhaseGoalKind::RepairValidationFailures,
			..
		})
	));
	assert!(events.iter().any(|event| {
		event.event_type() == PHASE_ACCEPTANCE_CHECK_EVENT_TYPE
			&& event.payload()["decision"] == "fail"
			&& event.payload()["reason_code"] == "no_effective_delta"
	}));
	assert!(events.iter().any(|event| {
		event.event_type() == "phase_goal_transition"
			&& event.payload()["signal"] == "validation_fail"
			&& event.payload()["payload"]["errorClass"] == "phase_acceptance_check_failed"
			&& event.payload()["payload"]["laneDecision"] == "retry_failure"
	}));
	assert!(events.iter().any(|event| {
		event.event_type() == "lane_decision"
			&& event.payload()["next_action"] == "retry_failure"
			&& event.payload()["phase_acceptance_failure"] == true
	}));
	assert!(events.iter().all(|event| {
		event.event_type() != "phase_goal_next" || event.payload()["phase"] != "handoff_evidence"
	}));
}

#[test]
fn phase_goal_acceptance_non_goal_violation_requests_manual_attention() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue(
		"In Progress",
		&[tracker::automation_active_label(TEST_SERVICE_ID).as_str()],
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue_run = IssueRunPlan {
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
	};

	tests::commit_worktree_change(config.repo_root(), "ready.txt", "before\n", "add ready file");
	fs::write(config.repo_root().join("ready.txt"), "after\n").expect("tracked diff should write");

	record_phase_acceptance_progress_checkpoint(
		&config,
		&state_store,
		&issue_run,
		&["non-goal violation: changed retained ownership policy"],
	);

	let error = RepoGatePhaseGoalController {
		project: &config,
		workflow: &workflow,
		state_store: &state_store,
		issue_run: &issue_run,
	}
	.phase_goal_completed(PhaseGoalKind::ImplementToValidationReady)
	.expect_err("non-goal acceptance failure should stop automatic repair");
	let manual_attention = error
		.downcast_ref::<ManualAttentionRequested>()
		.expect("non-goal acceptance failure should request manual attention");
	let events = state_store
		.list_private_execution_events(TEST_SERVICE_ID, &issue.id, &issue_run.run_id, 1)
		.expect("private phase goal events should load");

	assert_eq!(manual_attention.error_class.as_deref(), Some("phase_acceptance_check_failed"));
	assert!(events.iter().any(|event| {
		event.event_type() == PHASE_ACCEPTANCE_CHECK_EVENT_TYPE
			&& event.payload()["decision"] == "fail"
			&& event.payload()["reason_code"] == "non_goal_violation"
			&& event.payload()["non_goal_check"]["passed"] == false
	}));
	assert!(events.iter().any(|event| {
		event.event_type() == "lane_decision"
			&& event.payload()["next_action"] == "needs_attention"
			&& event.payload()["non_goal_violation"] == true
	}));
}

#[test]
fn blocking_lane_decision_evidence_clears_after_new_unblocked_checkpoint() {
	let (_temp_dir, config, _workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue(
		"In Progress",
		&[tracker::automation_active_label(TEST_SERVICE_ID).as_str()],
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let run_id = "run-cleared-blocker";

	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			&issue.id,
			run_id,
			1,
			"progress_checkpoint",
			serde_json::json!({
				"blockers": ["repo-wide baseline requires separate authority"],
				"docs_impact": "none",
			}),
		)
		.expect("blocking checkpoint should record");

	assert!(
		orchestrator::issue_has_blocking_lane_decision_evidence(&config, &state_store, &issue.id)
			.expect("blocking evidence should evaluate")
	);

	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			&issue.id,
			run_id,
			1,
			"progress_checkpoint",
			serde_json::json!({
				"docs_impact": "none",
			}),
		)
		.expect("ordinary checkpoint should record");

	assert!(
		orchestrator::issue_has_blocking_lane_decision_evidence(&config, &state_store, &issue.id)
			.expect("blocking evidence should evaluate"),
		"checkpoint without an explicit empty blockers array must not clear older blockers"
	);

	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			&issue.id,
			run_id,
			1,
			"progress_checkpoint",
			serde_json::json!({
				"blockers": [],
				"docs_impact": "none",
			}),
		)
		.expect("clearing checkpoint should record");

	assert!(
		!orchestrator::issue_has_blocking_lane_decision_evidence(&config, &state_store, &issue.id)
			.expect("blocking evidence should evaluate"),
		"latest unblocked checkpoint should clear older progress blockers"
	);
}

#[test]
fn blocking_lane_decision_evidence_prefers_kernel_projection_over_legacy_action() {
	let (_temp_dir, config, _workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue(
		"In Progress",
		&[tracker::automation_active_label(TEST_SERVICE_ID).as_str()],
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let run_id = "run-kernel-lane-decision";

	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			&issue.id,
			run_id,
			1,
			"lane_decision",
			serde_json::json!({
				"next_action": "needs_attention",
				"kernel_decision": {
					"decision_class": "retry_automatically",
					"command_intents": [{"kind": "schedule_retry"}],
				},
			}),
		)
		.expect("kernel retry decision should record");

	assert!(
		!orchestrator::issue_has_blocking_lane_decision_evidence(&config, &state_store, &issue.id)
			.expect("blocking evidence should evaluate"),
		"kernel decision must override stale compatibility action"
	);

	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			&issue.id,
			run_id,
			1,
			"lane_decision",
			serde_json::json!({
				"next_action": "retry_failure",
				"kernel_decision": {
					"decision_class": "manual_intervention_required",
					"command_intents": [{"kind": "request_manual_intervention"}],
				},
			}),
		)
		.expect("kernel manual decision should record");

	assert!(
		orchestrator::issue_has_blocking_lane_decision_evidence(&config, &state_store, &issue.id)
			.expect("blocking evidence should evaluate"),
		"kernel manual decision must block even when compatibility action is stale"
	);
}

#[test]
fn cleared_checkpoint_allows_same_run_phase_goal_recovery_candidate() {
	let (_temp_dir, config, _workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue(
		"In Progress",
		&[tracker::automation_active_label(TEST_SERVICE_ID).as_str()],
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue_run = phase_goal_repo_gate_issue_run(&config, &issue);

	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			&issue.id,
			&issue_run.run_id,
			issue_run.attempt_number,
			"phase_goal_status",
			serde_json::json!({
				"phase": "implement_to_validation_ready",
				"status": "active",
			}),
		)
		.expect("phase goal status should record");
	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			&issue.id,
			&issue_run.run_id,
			issue_run.attempt_number,
			"progress_checkpoint",
			serde_json::json!({
				"blockers": ["repo-wide baseline requires separate authority"],
			}),
		)
		.expect("blocking checkpoint should record");
	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			&issue.id,
			&issue_run.run_id,
			issue_run.attempt_number,
			"progress_checkpoint",
			serde_json::json!({
				"blockers": [],
			}),
		)
		.expect("clearing checkpoint should record");

	assert_eq!(
		execution_phase_goal::latest_phase_goal_recovery_candidate(
			&config,
			&state_store,
			&issue_run,
		)
		.expect("phase goal recovery candidate should evaluate"),
		Some(PhaseGoalKind::ImplementToValidationReady)
	);
}

#[test]
fn cleared_checkpoint_allows_cross_attempt_phase_goal_inheritance() {
	let (_temp_dir, config, _workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue(
		"In Progress",
		&[tracker::automation_active_label(TEST_SERVICE_ID).as_str()],
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let source_run_id = "pub-101-attempt-1";

	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			&issue.id,
			source_run_id,
			1,
			"phase_goal_next",
			serde_json::json!({
				"phase": "handoff_evidence",
			}),
		)
		.expect("phase goal next should record");
	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			&issue.id,
			source_run_id,
			1,
			"progress_checkpoint",
			serde_json::json!({
				"blockers": ["repo-wide baseline requires separate authority"],
			}),
		)
		.expect("blocking checkpoint should record");
	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			&issue.id,
			source_run_id,
			1,
			"progress_checkpoint",
			serde_json::json!({
				"blockers": [],
			}),
		)
		.expect("clearing checkpoint should record");

	assert_eq!(
		orchestrator::latest_open_issue_phase_goal_before_attempt(
			&config,
			&state_store,
			&issue.id,
			"pub-101-attempt-2",
			2,
		)
		.expect("phase goal inheritance should evaluate"),
		Some(PhaseGoalKind::HandoffEvidence)
	);
}

#[test]
fn retry_phase_goal_resumes_cross_attempt_handoff_after_recovered_validation_pass() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue(
		"In Progress",
		&[tracker::automation_active_label(TEST_SERVICE_ID).as_str()],
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let first_issue_run = IssueRunPlan {
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
	};

	tests::commit_worktree_change(config.repo_root(), "ready.txt", "before\n", "add ready file");
	fs::write(config.repo_root().join("ready.txt"), "after\n").expect("tracked diff should write");

	record_phase_acceptance_progress_checkpoint(&config, &state_store, &first_issue_run, &[]);

	RepoGatePhaseGoalController {
		project: &config,
		workflow: &workflow,
		state_store: &state_store,
		issue_run: &first_issue_run,
	}
	.phase_goal_completed(PhaseGoalKind::ImplementToValidationReady)
	.expect("completed implementation phase should persist handoff phase");
	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			&issue.id,
			&first_issue_run.run_id,
			first_issue_run.attempt_number,
			PHASE_GOAL_RECOVERY_EVENT_TYPE,
			serde_json::json!({
				"schema": "decodex.phase_goal_signal/1",
				"phase": "implement_to_validation_ready",
				"signal": "phase_goal_recovered",
				"payload": {
					"nextPhase": "handoff_evidence",
					"sourceErrorClass": "app_server_run_failed",
				},
			}),
		)
		.expect("phase goal recovery should record");

	let retry_issue_run = IssueRunPlan {
		issue: issue.clone(),
		issue_state: String::from("In Progress"),
		initial_issue_state: String::from("Todo"),
		worktree: first_issue_run.worktree.clone(),
		retry_project_slug: String::from("pubfi"),
		dispatch_mode: IssueDispatchMode::Retry,
		attempt_number: 2,
		run_id: String::from("pub-101-attempt-2"),
		retry_budget_base: 1,
	};
	let goal = RepoGatePhaseGoalController {
		project: &config,
		workflow: &workflow,
		state_store: &state_store,
		issue_run: &retry_issue_run,
	}
	.initial_phase_goal()
	.expect("retry phase goal should build")
	.expect("retry should still set a phase goal");

	assert_eq!(goal.phase, PhaseGoalKind::HandoffEvidence);
	assert!(
		goal.objective.contains("prepare PR-backed handoff evidence"),
		"retry should continue to handoff instead of repeating implementation"
	);
}

#[test]
fn retry_phase_goal_resumes_cross_attempt_active_handoff_phase() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue(
		"In Progress",
		&[tracker::automation_active_label(TEST_SERVICE_ID).as_str()],
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree = WorktreeSpec {
		branch_name: String::from("x/pubfi-pub-101"),
		issue_identifier: issue.identifier.clone(),
		path: config.repo_root().to_path_buf(),
		reused_existing: false,
	};

	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			&issue.id,
			"pub-101-attempt-2",
			2,
			"phase_goal_set",
			serde_json::json!({
				"schema": "decodex.phase_goal_signal/1",
				"phase": "handoff_evidence",
				"payload": {
					"phase": "handoff_evidence",
					"status": "active",
				},
			}),
		)
		.expect("active handoff phase should record");

	let retry_issue_run = IssueRunPlan {
		issue: issue.clone(),
		issue_state: String::from("In Progress"),
		initial_issue_state: String::from("Todo"),
		worktree,
		retry_project_slug: String::from("pubfi"),
		dispatch_mode: IssueDispatchMode::Retry,
		attempt_number: 3,
		run_id: String::from("pub-101-attempt-3"),
		retry_budget_base: 2,
	};
	let goal = RepoGatePhaseGoalController {
		project: &config,
		workflow: &workflow,
		state_store: &state_store,
		issue_run: &retry_issue_run,
	}
	.initial_phase_goal()
	.expect("retry phase goal should build")
	.expect("retry should still set a phase goal");

	assert_eq!(goal.phase, PhaseGoalKind::HandoffEvidence);
	assert!(
		goal.objective.contains("prepare PR-backed handoff evidence"),
		"retry should resume handoff evidence instead of repeating implementation"
	);
}

#[test]
fn retry_phase_goal_does_not_resume_cross_attempt_phase_after_terminal_finalize() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue(
		"In Progress",
		&[tracker::automation_active_label(TEST_SERVICE_ID).as_str()],
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree = WorktreeSpec {
		branch_name: String::from("x/pubfi-pub-101"),
		issue_identifier: issue.identifier.clone(),
		path: config.repo_root().to_path_buf(),
		reused_existing: false,
	};

	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			&issue.id,
			"pub-101-attempt-2",
			2,
			"phase_goal_set",
			serde_json::json!({
				"schema": "decodex.phase_goal_signal/1",
				"phase": "handoff_evidence",
				"payload": {
					"phase": "handoff_evidence",
					"status": "active",
				},
			}),
		)
		.expect("active handoff phase should record");
	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			&issue.id,
			"pub-101-attempt-2",
			2,
			"terminal_finalize",
			serde_json::json!({
				"schema": "decodex.terminal_finalize/1",
				"path": "review_handoff",
			}),
		)
		.expect("terminal finalize should record");

	let retry_issue_run = IssueRunPlan {
		issue: issue.clone(),
		issue_state: String::from("In Progress"),
		initial_issue_state: String::from("Todo"),
		worktree,
		retry_project_slug: String::from("pubfi"),
		dispatch_mode: IssueDispatchMode::Retry,
		attempt_number: 3,
		run_id: String::from("pub-101-attempt-3"),
		retry_budget_base: 2,
	};
	let goal = RepoGatePhaseGoalController {
		project: &config,
		workflow: &workflow,
		state_store: &state_store,
		issue_run: &retry_issue_run,
	}
	.initial_phase_goal()
	.expect("retry phase goal should build")
	.expect("retry should still set a phase goal");

	assert_eq!(goal.phase, PhaseGoalKind::ImplementToValidationReady);
}

#[test]
fn retry_phase_goal_uses_latest_open_phase_for_cross_attempt_resume() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue(
		"In Progress",
		&[tracker::automation_active_label(TEST_SERVICE_ID).as_str()],
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree = WorktreeSpec {
		branch_name: String::from("x/pubfi-pub-101"),
		issue_identifier: issue.identifier.clone(),
		path: config.repo_root().to_path_buf(),
		reused_existing: false,
	};

	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			&issue.id,
			"pub-101-attempt-1",
			1,
			"phase_goal_next",
			serde_json::json!({
				"schema": "decodex.phase_goal_signal/1",
				"phase": "handoff_evidence",
				"reason": "validation_pass",
			}),
		)
		.expect("older handoff phase should record");
	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			&issue.id,
			"pub-101-attempt-2",
			2,
			"phase_goal_set",
			serde_json::json!({
				"schema": "decodex.phase_goal_signal/1",
				"phase": "implement_to_validation_ready",
				"payload": {
					"phase": "implement_to_validation_ready",
					"status": "active",
				},
			}),
		)
		.expect("newer implementation phase should record");

	let retry_issue_run = IssueRunPlan {
		issue: issue.clone(),
		issue_state: String::from("In Progress"),
		initial_issue_state: String::from("Todo"),
		worktree,
		retry_project_slug: String::from("pubfi"),
		dispatch_mode: IssueDispatchMode::Retry,
		attempt_number: 3,
		run_id: String::from("pub-101-attempt-3"),
		retry_budget_base: 2,
	};
	let goal = RepoGatePhaseGoalController {
		project: &config,
		workflow: &workflow,
		state_store: &state_store,
		issue_run: &retry_issue_run,
	}
	.initial_phase_goal()
	.expect("retry phase goal should build")
	.expect("retry should still set a phase goal");

	assert_eq!(goal.phase, PhaseGoalKind::ImplementToValidationReady);
}

#[test]
fn retry_phase_goal_skips_empty_failed_start_attempt_for_cross_attempt_resume() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue(
		"In Progress",
		&[tracker::automation_active_label(TEST_SERVICE_ID).as_str()],
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree = WorktreeSpec {
		branch_name: String::from("x/pubfi-pub-101"),
		issue_identifier: issue.identifier.clone(),
		path: config.repo_root().to_path_buf(),
		reused_existing: false,
	};

	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			&issue.id,
			"pub-101-attempt-1",
			1,
			"phase_goal_next",
			serde_json::json!({
				"schema": "decodex.phase_goal_signal/1",
				"phase": "handoff_evidence",
				"reason": "validation_pass",
			}),
		)
		.expect("older handoff phase should record");
	state_store
		.record_run_attempt("pub-101-attempt-2", &issue.id, 2, "failed")
		.expect("empty previous attempt should record");

	let retry_issue_run = IssueRunPlan {
		issue: issue.clone(),
		issue_state: String::from("In Progress"),
		initial_issue_state: String::from("Todo"),
		worktree,
		retry_project_slug: String::from("pubfi"),
		dispatch_mode: IssueDispatchMode::Retry,
		attempt_number: 3,
		run_id: String::from("pub-101-attempt-3"),
		retry_budget_base: 2,
	};
	let goal = RepoGatePhaseGoalController {
		project: &config,
		workflow: &workflow,
		state_store: &state_store,
		issue_run: &retry_issue_run,
	}
	.initial_phase_goal()
	.expect("retry phase goal should build")
	.expect("retry should still set a phase goal");

	assert_eq!(goal.phase, PhaseGoalKind::HandoffEvidence);
	assert!(
		goal.objective.contains("prepare PR-backed handoff evidence"),
		"empty failed-start attempts must not erase the issue's open handoff phase"
	);
}

#[test]
fn program_phase_goal_skips_empty_failed_start_attempt_for_cross_attempt_resume() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue(
		"In Progress",
		&[tracker::automation_active_label(TEST_SERVICE_ID).as_str()],
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree = WorktreeSpec {
		branch_name: String::from("x/pubfi-pub-101"),
		issue_identifier: issue.identifier.clone(),
		path: config.repo_root().to_path_buf(),
		reused_existing: false,
	};

	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			&issue.id,
			"pub-101-attempt-1",
			1,
			"phase_goal_next",
			serde_json::json!({
				"schema": "decodex.phase_goal_signal/1",
				"phase": "handoff_evidence",
				"reason": "validation_pass",
			}),
		)
		.expect("older handoff phase should record");
	state_store
		.record_run_attempt("pub-101-attempt-2", &issue.id, 2, "failed")
		.expect("empty previous attempt should record");

	let program_issue_run = IssueRunPlan {
		issue: issue.clone(),
		issue_state: String::from("In Progress"),
		initial_issue_state: String::from("Todo"),
		worktree,
		retry_project_slug: String::from("pubfi"),
		dispatch_mode: IssueDispatchMode::Program,
		attempt_number: 3,
		run_id: String::from("pub-101-attempt-3"),
		retry_budget_base: 2,
	};
	let goal = RepoGatePhaseGoalController {
		project: &config,
		workflow: &workflow,
		state_store: &state_store,
		issue_run: &program_issue_run,
	}
	.initial_phase_goal()
	.expect("program phase goal should build")
	.expect("program run should still set a phase goal");

	assert_eq!(goal.phase, PhaseGoalKind::HandoffEvidence);
	assert!(
		goal.objective.contains("prepare PR-backed handoff evidence"),
		"program dispatch must continue the open handoff phase instead of restarting implementation"
	);
}

#[test]
fn open_phase_goal_unowned_tracked_rewrites_stop_instead_of_repair_continuation() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout_with_workflow_markdown(
		&tests::sample_workflow_markdown("pubfi", &[], "Phase goal validation policy.\n", 1)
			.replace(
				"canonicalize_commands = []",
				"canonicalize_commands = [\"printf 'rewritten\\\\n' > other.txt\"]",
			),
	);
	let repo_root = config.repo_root();
	let issue = tests::sample_issue(
		"In Progress",
		&[tracker::automation_active_label(TEST_SERVICE_ID).as_str()],
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue_run = IssueRunPlan {
		issue: issue.clone(),
		issue_state: String::from("In Progress"),
		initial_issue_state: String::from("Todo"),
		worktree: WorktreeSpec {
			branch_name: String::from("x/pubfi-pub-101"),
			issue_identifier: issue.identifier.clone(),
			path: repo_root.to_path_buf(),
			reused_existing: false,
		},
		retry_project_slug: String::from("pubfi"),
		dispatch_mode: IssueDispatchMode::Normal,
		attempt_number: 1,
		run_id: String::from("pub-101-attempt-1"),
		retry_budget_base: 0,
	};

	tests::commit_worktree_change(repo_root, "ready.txt", "before\n", "add ready file");
	tests::commit_worktree_change(repo_root, "other.txt", "before\n", "add other file");
	fs::write(repo_root.join("ready.txt"), "after\n").expect("tracked diff should write");

	record_phase_acceptance_progress_checkpoint(&config, &state_store, &issue_run, &[]);

	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			&issue.id,
			&issue_run.run_id,
			1,
			"phase_goal_set",
			serde_json::json!({
				"schema": "decodex.phase_goal_signal/1",
				"phase": "implement_to_validation_ready",
				"payload": {
					"phase": "implement_to_validation_ready",
					"status": "active",
				},
			}),
		)
		.expect("phase goal event should record");

	let error = orchestrator::maybe_continue_after_phase_goal_recovery(
		&config,
		&workflow,
		&state_store,
		&issue_run,
		&Report::msg("app server transport closed after local verification"),
	)
	.expect_err("tracked repo-gate rewrites should stop phase-goal continuation");
	let events = state_store
		.list_private_execution_events(TEST_SERVICE_ID, &issue.id, &issue_run.run_id, 1)
		.expect("private events should load");
	let repo_gate_failure = error
		.downcast_ref::<RepoGateFailure>()
		.expect("phase goal recovery should preserve repo-gate failure");

	assert_eq!(repo_gate_failure.error_class(), "repo_gate_tracked_rewrites_left");
	assert_eq!(
		repo_gate_failure.disposition(),
		orchestrator::RepoGateFailureDisposition::NeedsHumanAttention
	);
	assert!(events.iter().any(|event| {
		event.event_type() == "phase_goal_transition"
			&& event.payload()["signal"] == "validation_fail"
			&& event.payload()["payload"]["disposition"] == "needs_human_attention"
			&& event.payload()["payload"]["trackedRewrites"]["owned"] == false
			&& event.payload()["payload"]["trackedRewrites"]["files"]
				.as_array()
				.is_some_and(|files| files.iter().any(|file| file.as_str() == Some("other.txt")))
	}));
	assert!(events.iter().all(|event| event.event_type() != "phase_goal_next"));
	assert!(events.iter().all(|event| event.event_type() != "phase_goal_recovery"));
}

#[test]
fn open_phase_goal_owned_tracked_rewrites_continue_to_handoff_recovery() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout_with_workflow_markdown(
		&tests::sample_workflow_markdown("pubfi", &[], "Phase goal validation policy.\n", 1)
			.replace(
				"canonicalize_commands = []",
				"canonicalize_commands = [\"printf 'rewritten\\\\n' > ready.txt\"]",
			),
	);
	let repo_root = config.repo_root();
	let issue = tests::sample_issue(
		"In Progress",
		&[tracker::automation_active_label(TEST_SERVICE_ID).as_str()],
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue_run = IssueRunPlan {
		issue: issue.clone(),
		issue_state: String::from("In Progress"),
		initial_issue_state: String::from("Todo"),
		worktree: WorktreeSpec {
			branch_name: String::from("x/pubfi-pub-101"),
			issue_identifier: issue.identifier.clone(),
			path: repo_root.to_path_buf(),
			reused_existing: false,
		},
		retry_project_slug: String::from("pubfi"),
		dispatch_mode: IssueDispatchMode::Normal,
		attempt_number: 1,
		run_id: String::from("pub-101-attempt-1"),
		retry_budget_base: 0,
	};

	tests::commit_worktree_change(repo_root, "ready.txt", "before\n", "add ready file");
	fs::write(repo_root.join("ready.txt"), "after\n").expect("tracked diff should write");

	record_phase_acceptance_progress_checkpoint(&config, &state_store, &issue_run, &[]);

	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			&issue.id,
			&issue_run.run_id,
			1,
			"phase_goal_set",
			serde_json::json!({
				"schema": "decodex.phase_goal_signal/1",
				"phase": "implement_to_validation_ready",
				"payload": {
					"phase": "implement_to_validation_ready",
					"status": "active",
				},
			}),
		)
		.expect("phase goal event should record");

	let summary = orchestrator::maybe_continue_after_phase_goal_recovery(
		&config,
		&workflow,
		&state_store,
		&issue_run,
		&Report::msg("app server transport closed after local verification"),
	)
	.expect("owned tracked repo-gate rewrites should keep phase-goal recovery automatic")
	.expect("owned tracked repo-gate rewrites should schedule continuation");
	let events = state_store
		.list_private_execution_events(TEST_SERVICE_ID, &issue.id, &issue_run.run_id, 1)
		.expect("private events should load");

	assert!(summary.continuation_pending);
	assert!(events.iter().any(|event| {
		event.event_type() == "phase_goal_transition"
			&& event.payload()["signal"] == "validation_pass"
			&& event.payload()["payload"]["nextPhase"] == "handoff_evidence"
			&& event.payload()["payload"]["trackedRewrites"]["owned"] == true
			&& event.payload()["payload"]["trackedRewrites"]["decision"]
				== "continue_to_commit_capable_phase"
			&& event.payload()["payload"]["trackedRewrites"]["files"]
				.as_array()
				.is_some_and(|files| files.iter().any(|file| file.as_str() == Some("ready.txt")))
	}));
	assert!(events.iter().any(|event| {
		event.event_type() == PHASE_ACCEPTANCE_CHECK_EVENT_TYPE
			&& event.payload()["decision"] == "pass"
			&& event.payload()["validation_evidence"]["tracked_rewrites"]["owned"] == true
	}));
	assert!(events.iter().any(|event| {
		event.event_type() == "phase_goal_next" && event.payload()["phase"] == "handoff_evidence"
	}));
	assert!(events.iter().any(|event| event.event_type() == "phase_goal_recovery"));
}

#[test]
fn repeated_phase_goal_recovery_blocks_second_automatic_continuation() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let repo_root = config.repo_root();
	let issue = tests::sample_issue(
		"In Progress",
		&[tracker::automation_active_label(TEST_SERVICE_ID).as_str()],
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let app_server_timeout =
		Report::new(AppServerCapabilityPreflightFailure::method_timed_out_for_test(
			"thread/goal/get",
			String::from("Timed out while waiting for app-server output."),
		));

	tests::commit_worktree_change(repo_root, "ready.txt", "before\n", "add ready file");
	fs::write(repo_root.join("ready.txt"), "after\n").expect("tracked diff should write");

	for (run_id, attempt_number) in [("pub-101-attempt-1", 1), ("pub-101-attempt-2", 2)] {
		state_store
			.append_private_execution_event(
				TEST_SERVICE_ID,
				&issue.id,
				run_id,
				attempt_number,
				"phase_goal_set",
				serde_json::json!({
					"schema": "decodex.phase_goal_signal/1",
					"phase": "implement_to_validation_ready",
					"payload": {
						"phase": "implement_to_validation_ready",
						"status": "active",
					},
				}),
			)
			.expect("phase goal event should record");
	}

	let first_issue_run = IssueRunPlan {
		issue: issue.clone(),
		issue_state: String::from("In Progress"),
		initial_issue_state: String::from("Todo"),
		worktree: WorktreeSpec {
			branch_name: String::from("x/pubfi-pub-101"),
			issue_identifier: issue.identifier.clone(),
			path: repo_root.to_path_buf(),
			reused_existing: false,
		},
		retry_project_slug: String::from("pubfi"),
		dispatch_mode: IssueDispatchMode::Normal,
		attempt_number: 1,
		run_id: String::from("pub-101-attempt-1"),
		retry_budget_base: 0,
	};
	let second_issue_run = IssueRunPlan {
		issue: issue.clone(),
		issue_state: String::from("In Progress"),
		initial_issue_state: String::from("Todo"),
		worktree: WorktreeSpec {
			branch_name: String::from("x/pubfi-pub-101"),
			issue_identifier: issue.identifier.clone(),
			path: repo_root.to_path_buf(),
			reused_existing: false,
		},
		retry_project_slug: String::from("pubfi"),
		dispatch_mode: IssueDispatchMode::Normal,
		attempt_number: 2,
		run_id: String::from("pub-101-attempt-2"),
		retry_budget_base: 0,
	};
	let first = orchestrator::maybe_continue_after_phase_goal_recovery(
		&config,
		&workflow,
		&state_store,
		&first_issue_run,
		&app_server_timeout,
	)
	.expect("first recovery should evaluate")
	.expect("first recovery should schedule continuation");
	let second = orchestrator::maybe_continue_after_phase_goal_recovery(
		&config,
		&workflow,
		&state_store,
		&second_issue_run,
		&app_server_timeout,
	)
	.expect("second recovery should evaluate");
	let events = state_store
		.list_private_execution_events_for_issue(TEST_SERVICE_ID, &issue.id)
		.expect("private phase goal events should load");
	let scheduled_events = events
		.iter()
		.filter(|event| event.event_type() == PHASE_GOAL_RECOVERY_EVENT_TYPE)
		.collect::<Vec<_>>();
	let blocked_event = events
		.iter()
		.find(|event| event.event_type() == PHASE_GOAL_RECOVERY_BLOCKED_EVENT_TYPE)
		.expect("second recovery should record blocked event");

	assert!(first.continuation_pending);
	assert!(second.is_none());
	assert_eq!(scheduled_events.len(), 1);
	assert_eq!(blocked_event.payload()["signal"], "continuation_budget_exhausted");
	assert_eq!(blocked_event.payload()["payload"]["priorRecoveryCount"], 1);
	assert_eq!(
		blocked_event.payload()["payload"]["automaticContinuationLimit"],
		orchestrator::PHASE_GOAL_RECOVERY_AUTOMATIC_CONTINUATION_LIMIT
	);
	assert!(blocked_event.payload()["payload"]["sourceErrorMessage"].as_str().is_some_and(
		|message| { message.contains("Timed out while waiting for app-server output.") }
	));
	assert_eq!(
		blocked_event.payload()["payload"]["sourceErrorClass"],
		"app_server_preflight_timeout"
	);
}

#[test]
fn implementation_phase_goal_contract_requires_explicit_goal_completion() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue(
		"In Progress",
		&[tracker::automation_active_label(TEST_SERVICE_ID).as_str()],
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue_run = IssueRunPlan {
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
	};
	let controller = RepoGatePhaseGoalController {
		project: &config,
		workflow: &workflow,
		state_store: &state_store,
		issue_run: &issue_run,
	};
	let goal = controller
		.initial_phase_goal()
		.expect("phase goal should build")
		.expect("normal dispatch should set an implementation phase goal");

	assert_eq!(goal.phase, PhaseGoalKind::ImplementToValidationReady);
	assert!(goal.objective.contains(
		"explicitly mark the active phase goal complete with the Codex goal completion mechanism"
	));
	assert!(goal.objective.contains("Decodex can run its repo gate and select the next phase"));
	assert!(goal.objective.contains("Do not end with only an `issue_progress_checkpoint`"));
	assert!(goal.objective.contains("while the phase goal is still active"));
}

#[test]
fn repo_gate_shell_falls_back_to_non_login_posix_sh_for_missing_absolute_shell() {
	let (shell, shell_flag) = orchestrator::repo_gate_shell_from_env(Some(OsString::from(
		"/definitely-missing-shell-for-tests",
	)));

	assert_eq!(Path::new(&shell), Path::new("/bin/sh"));
	assert_eq!(shell_flag, "-c");
}

#[test]
fn repo_gate_shell_uses_non_login_mode_when_shell_is_bin_sh() {
	let (shell, shell_flag) =
		orchestrator::repo_gate_shell_from_env(Some(OsString::from("/bin/sh")));

	assert_eq!(Path::new(&shell), Path::new("/bin/sh"));
	assert_eq!(shell_flag, "-c");
}

#[test]
fn repo_gate_shell_keeps_login_mode_for_other_configured_shells() {
	let (shell, shell_flag) =
		orchestrator::repo_gate_shell_from_env(Some(OsString::from("/bin/bash")));

	assert_eq!(Path::new(&shell), Path::new("/bin/bash"));
	assert_eq!(shell_flag, "-lc");
}
