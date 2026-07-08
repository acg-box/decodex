use std::fs;

use crate::{
	agent::PhaseGoalController,
	orchestrator::{
		PhaseGoalKind, PhaseGoalSpec, PhaseGoalTransition, RepoGatePhaseGoalController, StateStore,
		tests,
		tests::{TEST_SERVICE_ID, runtime_repo_gate::support},
	},
};

#[test]
fn review_repair_phase_goal_validation_passes_to_review_repair_evidence() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue("In Review", &[]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue_run = support::review_repair_phase_goal_issue_run(&config, &issue);

	tests::commit_worktree_change(config.repo_root(), "ready.txt", "before\n", "add ready file");
	fs::write(config.repo_root().join("ready.txt"), "after\n").expect("tracked diff should write");
	support::record_validation_evidence_progress_checkpoint(&config, &state_store, &issue_run, &[]);

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
