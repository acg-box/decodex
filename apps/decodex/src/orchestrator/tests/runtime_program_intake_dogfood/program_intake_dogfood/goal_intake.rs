use crate::{
	loop_contract::{DecisionPromotion, DecisionPromotionActorKind},
	orchestrator,
	orchestrator::tests::runtime_program_intake_dogfood::program_intake_dogfood::support::{
		self, DogfoodTracker,
	},
	program_intake::{self, GoalIntakeRunRequest},
	state::StateStore,
};

#[test]
fn goal_intake_apply_direct_dispatch_is_end_to_end() {
	let (_temp_dir, config, workflow) = super::temp_project_layout();
	let store = StateStore::open_in_memory().expect("state store should open");
	let source_issue =
		support::dogfood_issue(config.service_id(), "issue-source", "PUB-940A", "Todo", &[]);
	let tracker = DogfoodTracker::default().with_issues([source_issue]);
	let latent = support::dogfood_goal_contract();

	store
		.upsert_decision_contract(config.service_id(), Some("PUB-940A"), latent.clone())
		.expect("latent contract should persist");

	let latent_error = program_intake::run_goal_intake(GoalIntakeRunRequest {
		state_store: &store,
		tracker: &tracker,
		config: &config,
		workflow: &workflow,
		contract_id: latent.contract_id(),
		team_issue_identifier: None,
		dry_run: false,
		apply: true,
	})
	.expect_err("latent contract must not materialize");

	assert!(latent_error.to_string().contains("requires accepted execution authority"));
	assert!(store.list_execution_programs(config.service_id()).expect("programs").is_empty());

	let mut accepted = latent.clone();

	accepted
		.promote(
			DecisionPromotion::new(
				"operator",
				DecisionPromotionActorKind::User,
				"2026-06-12T00:00:00Z",
				"controlled_fixture",
				Some(String::from("XY-942 controlled dogfood accepted authority.")),
			)
			.expect("promotion should build"),
		)
		.expect("contract should promote");
	store
		.upsert_decision_contract(config.service_id(), Some("PUB-940A"), accepted.clone())
		.expect("accepted contract should persist");

	let apply_report = program_intake::run_goal_intake(GoalIntakeRunRequest {
		state_store: &store,
		tracker: &tracker,
		config: &config,
		workflow: &workflow,
		contract_id: accepted.contract_id(),
		team_issue_identifier: None,
		dry_run: false,
		apply: true,
	})
	.expect("accepted contract should materialize issues and program");

	assert!(apply_report.applied);
	assert!(apply_report.persisted);
	assert_eq!(apply_report.issues.len(), 2);
	assert!(tracker.label_additions().is_empty());

	let selection = orchestrator::select_execution_program_run_candidate_with_summary(
		&tracker,
		&config,
		&workflow,
		&store,
		&[],
	)
	.expect("goal-intake program scheduler selection should find generated ready nodes");
	let selected =
		selection.selected.expect("one generated issue should be selected for direct dispatch");

	assert_eq!(selection.summary.dispatchable_nodes, 1);
	assert_eq!(selected.dispatch_mode, orchestrator::IssueDispatchMode::Program);
	assert!(tracker.label_additions().is_empty());
	assert!(tracker.label_removals().is_empty());

	let snapshot =
		orchestrator::build_live_operator_status_snapshot(&tracker, &config, &workflow, &store, 10)
			.expect("goal-intake status snapshot should build");
	let program = snapshot.execution_programs.first().expect("goal program should surface");
	let rendered_status = orchestrator::render_operator_status(&snapshot);

	assert_eq!(program.status, "blocked");
	assert_eq!(program.source_contract_id.as_deref(), Some(accepted.contract_id()));
	assert_eq!(program.intake_kind.as_deref(), Some("goal_intake"));
	assert_eq!(
		program.public_summary.as_deref(),
		Some("Dogfood accepted goal intake through generated issues.")
	);
	assert_eq!(program.ready_count, 1);
	assert_eq!(program.blocked_count, 1);
	assert!(
		program
			.node_readbacks
			.iter()
			.any(|node| node.reason_codes.contains(&String::from("dependency_not_terminal")))
	);
	assert_eq!(program.queued_count, 0);
	assert!(rendered_status.contains("source_contract_id: dogfood-goal-contract"));
	assert!(!rendered_status.contains("private_evidence"));
	assert!(!rendered_status.contains("research_evidence"));
}
