use crate::{
	program_intake::{
		self, GoalIntakeIssueAction, GoalIntakeRunRequest,
		tests::{test_support, test_support::FakeTracker},
	},
	state::StateStore,
};

#[test]
fn goal_intake_dry_run_shows_issue_split_without_mutation() {
	let store = StateStore::open_in_memory().expect("store should open");
	let contract = test_support::accepted_goal_contract();

	store
		.upsert_decision_contract("decodex", Some("XY-852"), contract)
		.expect("contract should persist");

	let tracker = FakeTracker::default().with_issues([test_support::issue("XY-852", "Todo")]);
	let config = test_support::test_config();
	let workflow = test_support::workflow();
	let report = program_intake::run_goal_intake(GoalIntakeRunRequest {
		state_store: &store,
		tracker: &tracker,
		config: &config,
		workflow: &workflow,
		contract_id: "goal-intake-contract",
		team_issue_identifier: None,
		dry_run: true,
		apply: false,
	})
	.expect("dry-run should produce materialization plan");

	assert!(report.dry_run);
	assert!(!report.persisted);
	assert_eq!(report.issues.len(), 2);
	assert_eq!(report.issues[0].action, GoalIntakeIssueAction::WouldCreate);
	assert_eq!(report.issues[0].dependencies, Vec::<String>::new());
	assert_eq!(report.issues[0].conflict_domains, vec![String::from("module:runtime")]);
	assert_eq!(report.issues[1].dependencies, vec![String::from("goal-intake-runtime")]);
	assert_eq!(tracker.created_issue_count(), 0);
	assert!(store.list_execution_programs("decodex").expect("programs").is_empty());

	let rendered = program_intake::render_goal_intake_report(&report);

	assert!(rendered.contains("dependencies=none"));
	assert!(rendered.contains("conflict_domains=module:runtime"));
	assert!(rendered.contains("dependencies=goal-intake-runtime"));
}
