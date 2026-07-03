use std::fs;

use serde::Deserialize;

use crate::{
	agent::PhaseGoalController,
	orchestrator::{
		self, EvidenceRequest, PhaseGoalKind, PhaseGoalSpec, PhaseGoalTransition,
		RepoGatePhaseGoalController, StateStore, tests,
		tests::{TEST_SERVICE_ID, runtime_repo_gate::support},
	},
	tracker,
};

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
	support::record_phase_acceptance_progress_checkpoint(&config, &state_store, &issue_run, &[]);

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
	let issue_run = support::phase_goal_repo_gate_issue_run(&config, &issue);
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
