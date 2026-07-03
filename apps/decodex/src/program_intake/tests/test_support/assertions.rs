use crate::{
	program_intake::{GoalIntakeIssueAction, GoalIntakeReport, tests::test_support::FakeTracker},
	state::{ExecutionProgramRecord, StateStore},
};

pub(crate) fn assert_goal_intake_apply_report(report: &GoalIntakeReport, tracker: &FakeTracker) {
	assert!(report.applied);
	assert!(report.persisted);
	assert_eq!(tracker.updated_issue_count(), 1);
	assert_eq!(tracker.created_issue_count(), 1);
	assert_eq!(report.issues[0].action, GoalIntakeIssueAction::Updated);
	assert_eq!(report.issues[1].action, GoalIntakeIssueAction::Created);
	assert_eq!(report.issues[0].dispatch_action.as_deref(), Some("dispatch"));
	assert_eq!(report.issues[1].dispatch_action.as_deref(), None);
	assert!(
		report.issues[1]
			.reasons
			.iter()
			.any(|reason| reason.contains("has not reached a required terminal state"))
	);
}

pub(crate) fn assert_goal_intake_runtime_links(store: &StateStore, report: &GoalIntakeReport) {
	let linked_contract = store
		.decision_contract("decodex", "goal-intake-contract")
		.expect("contract lookup should read")
		.expect("contract should exist");
	let node_ids = report.issues.iter().map(|issue| issue.node_id.clone()).collect::<Vec<_>>();

	assert_eq!(
		linked_contract.contract().links().generated_issue_identifiers(),
		&[String::from("XY-G1"), String::from("XY-G2")]
	);
	assert_eq!(linked_contract.contract().links().execution_program_node_ids(), &node_ids);

	let programs = store
		.list_execution_programs_for_contract("decodex", "goal-intake-contract")
		.expect("programs should list");
	let intake_plans =
		store.list_program_intake_plans("decodex").expect("intake plans should list");

	assert_eq!(programs.len(), 1);
	assert_eq!(programs[0].program_id(), report.program_id);
	assert_eq!(intake_plans.len(), 1);
	assert_eq!(intake_plans[0].intake_kind(), "goal_intake");
	assert_eq!(intake_plans[0].source_contract_id(), Some("goal-intake-contract"));

	assert_goal_intake_plan_lineage(&programs[0]);

	assert_eq!(
		store
			.list_program_issue_mappings("decodex", &report.program_id)
			.expect("mappings should list")
			.len(),
		2
	);
}

pub(crate) fn assert_goal_issue_brief_is_public(description: &str, report: &GoalIntakeReport) {
	for heading in [
		"## Objective",
		"## Authority",
		"## Required Reading",
		"## Ownership Boundary",
		"## Dependencies",
		"## Current-tree Landing Zone",
		"## Acceptance",
		"## Validation",
		"## Lifecycle Gates",
		"## Risk",
		"## Stop Conditions",
	] {
		assert!(description.contains(heading));
	}

	assert!(
		description
			.contains("Accepted Decision Contract authority is recorded in Decodex runtime state.")
	);
	assert!(description.contains("Source issue: `XY-852`"));
	assert!(description.contains("Goal intake dry-run renders generated issue briefs"));
	assert!(description.contains("Use normal Decodex review, PR handoff, landing"));
	assert!(description.contains("Run install or restart steps only when"));
	assert!(description.contains("Stop when promotion authority"));

	assert_goal_issue_brief_hides_private_ids(description, report);
}

fn assert_goal_intake_plan_lineage(record: &ExecutionProgramRecord) {
	let intake_plan = record
		.program()
		.program_intake_plan()
		.expect("program payload should retain intake plan lineage");

	assert_eq!(intake_plan.source_objective_ref(), Some("decodex:quality-autonomy@1"));
	assert_eq!(intake_plan.source_proposal_id(), Some("autonomy_proposal:test-proposal"));
	assert_eq!(intake_plan.source_signal_refs(), &[String::from("autonomy_signal:test-signal")]);
}

fn assert_goal_issue_brief_hides_private_ids(description: &str, report: &GoalIntakeReport) {
	for private_id in [
		"Execution Program: `",
		"Execution Program node:",
		"goal-intake-contract",
		"autonomy_proposal:test-proposal",
		"decodex:quality-autonomy@1",
		"autonomy_signal:test-signal",
		&report.program_id,
		"```",
		"private_evidence_refs",
	] {
		assert!(!description.contains(private_id));
	}
	for issue in &report.issues {
		assert!(!description.contains(&issue.node_id));
	}
}
