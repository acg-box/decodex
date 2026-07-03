use crate::{
	loop_contract::DecisionContract,
	program_intake::{
		self, GoalIntakeIssueAction, GoalIntakeRunRequest,
		tests::{test_support, test_support::FakeTracker},
	},
	state::StateStore,
	tracker::IssueTracker,
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

#[test]
fn goal_intake_refuses_latent_or_missing_decision_authority() {
	let store = StateStore::open_in_memory().expect("store should open");
	let tracker = FakeTracker::default().with_issues([test_support::issue("XY-852", "Todo")]);
	let latent = test_support::latent_goal_contract();

	store
		.upsert_decision_contract("decodex", Some("XY-852"), latent)
		.expect("latent contract should persist");

	let config = test_support::test_config();
	let workflow = test_support::workflow();
	let latent_error = program_intake::run_goal_intake(GoalIntakeRunRequest {
		state_store: &store,
		tracker: &tracker,
		config: &config,
		workflow: &workflow,
		contract_id: "goal-intake-contract",
		team_issue_identifier: None,
		dry_run: false,
		apply: true,
	})
	.expect_err("latent contract must not materialize");

	assert!(latent_error.to_string().contains("requires accepted execution authority"));

	let mut needs_decision = test_support::latent_goal_contract();

	needs_decision
		.require_human_decision("Choose the public issue split before apply.")
		.expect("contract should record missing decision");
	store
		.upsert_decision_contract("decodex", Some("XY-852"), needs_decision)
		.expect("needs-decision contract should persist");

	let missing_decision_error = program_intake::run_goal_intake(GoalIntakeRunRequest {
		state_store: &store,
		tracker: &tracker,
		config: &config,
		workflow: &workflow,
		contract_id: "goal-intake-contract",
		team_issue_identifier: None,
		dry_run: false,
		apply: true,
	})
	.expect_err("missing decision must stop apply");

	assert!(missing_decision_error.to_string().contains("needs_human_decision"));
	assert_eq!(tracker.created_issue_count(), 0);
	assert!(store.list_execution_programs("decodex").expect("programs").is_empty());
}

#[test]
fn goal_intake_apply_creates_updates_and_persists_links() {
	let store = StateStore::open_in_memory().expect("store should open");
	let mut contract = test_support::accepted_goal_contract();

	contract
		.link_generated_execution_surfaces(["id-XY-G1"], ["XY-G1"], ["old-node"])
		.expect("existing generated link should attach");
	store
		.upsert_decision_contract("decodex", Some("XY-852"), contract)
		.expect("contract should persist");

	let tracker = FakeTracker::default()
		.with_issues([test_support::issue("XY-852", "Todo"), test_support::issue("XY-G1", "Todo")]);
	let config = test_support::test_config();
	let workflow = test_support::workflow();
	let report = program_intake::run_goal_intake(GoalIntakeRunRequest {
		state_store: &store,
		tracker: &tracker,
		config: &config,
		workflow: &workflow,
		contract_id: "goal-intake-contract",
		team_issue_identifier: None,
		dry_run: false,
		apply: true,
	})
	.expect("apply should materialize issues and program");

	test_support::assert_goal_intake_apply_report(&report, &tracker);
	test_support::assert_goal_intake_runtime_links(&store, &report);

	let updated = tracker
		.get_issue_by_identifier("XY-G1")
		.expect("issue lookup should work")
		.expect("updated issue should exist");

	test_support::assert_goal_issue_brief_is_public(&updated.description, &report);
}

#[test]
fn generated_issue_text_validation_rejects_private_program_identifiers() {
	let title_error = program_intake::validate_generated_issue_text(
		"Expose goal-decodex-contract-private",
		"## Objective\nUse normal public text.",
		&["goal-decodex-contract-private"],
	)
	.expect_err("title must reject private program id");

	assert!(
		title_error
			.to_string()
			.contains("generated issue title contains a private Program Intake identifier")
	);

	let description_error = program_intake::validate_generated_issue_text(
		"Use normal public text.",
		"## Objective\nExpose goal:contract:01-private-node.",
		&["goal:contract:01-private-node"],
	)
	.expect_err("description must reject private node id");

	assert!(
		description_error
			.to_string()
			.contains("generated issue description contains a private Program Intake identifier")
	);
}

#[test]
fn goal_intake_apply_rejects_generated_briefs_that_leak_autonomy_lineage_ids() {
	let store = StateStore::open_in_memory().expect("store should open");
	let mut payload = serde_json::to_value(test_support::accepted_goal_contract())
		.expect("accepted goal contract should serialize");

	payload["accepted_authority"]["constraints"]
		.as_array_mut()
		.expect("constraints should be an array")
		.push(serde_json::json!(
			"Do not expose autonomy_signal:test-signal in generated issue text."
		));

	let leaking_contract: DecisionContract =
		serde_json::from_value(payload).expect("leaking contract should deserialize");

	store
		.upsert_decision_contract("decodex", Some("XY-852"), leaking_contract)
		.expect("contract should persist");

	let tracker = FakeTracker::default().with_issues([test_support::issue("XY-852", "Todo")]);
	let config = test_support::test_config();
	let workflow = test_support::workflow();
	let error = program_intake::run_goal_intake(GoalIntakeRunRequest {
		state_store: &store,
		tracker: &tracker,
		config: &config,
		workflow: &workflow,
		contract_id: "goal-intake-contract",
		team_issue_identifier: None,
		dry_run: false,
		apply: true,
	})
	.expect_err("private autonomy lineage ids must fail generated brief validation");

	assert!(
		error
			.to_string()
			.contains("generated issue description contains a private Program Intake identifier")
	);
	assert_eq!(tracker.created_issue_count(), 0);
	assert!(store.list_execution_programs("decodex").expect("programs").is_empty());
}

#[test]
fn goal_intake_apply_persists_links_after_each_successful_issue_mutation() {
	let store = StateStore::open_in_memory().expect("store should open");
	let contract = test_support::accepted_goal_contract();

	store
		.upsert_decision_contract("decodex", Some("XY-852"), contract)
		.expect("contract should persist");

	let tracker = FakeTracker::default()
		.with_issues([test_support::issue("XY-852", "Todo")])
		.with_create_failure_after_successes(1);
	let config = test_support::test_config();
	let workflow = test_support::workflow();
	let error = program_intake::run_goal_intake(GoalIntakeRunRequest {
		state_store: &store,
		tracker: &tracker,
		config: &config,
		workflow: &workflow,
		contract_id: "goal-intake-contract",
		team_issue_identifier: None,
		dry_run: false,
		apply: true,
	})
	.expect_err("second issue create should fail");

	assert!(error.to_string().contains("injected create failure"));
	assert_eq!(tracker.created_issue_count(), 1);

	let linked_contract = store
		.decision_contract("decodex", "goal-intake-contract")
		.expect("contract lookup should read")
		.expect("contract should exist");

	assert_eq!(
		linked_contract.contract().links().generated_issue_identifiers(),
		&[String::from("XY-G1")]
	);
	assert_eq!(linked_contract.contract().links().generated_issue_ids().len(), 1);
	assert_eq!(linked_contract.contract().links().execution_program_node_ids().len(), 1);
	assert!(store.list_execution_programs("decodex").expect("programs").is_empty());
}

#[test]
fn goal_intake_apply_preserves_later_existing_links_after_update_failure() {
	let store = StateStore::open_in_memory().expect("store should open");
	let mut contract = test_support::accepted_goal_contract();

	contract
		.link_generated_execution_surfaces(
			["id-XY-G1", "id-XY-G2"],
			["XY-G1", "XY-G2"],
			["old-node-1", "old-node-2"],
		)
		.expect("existing generated links should attach");
	store
		.upsert_decision_contract("decodex", Some("XY-852"), contract)
		.expect("contract should persist");

	let tracker = FakeTracker::default()
		.with_issues([
			test_support::issue("XY-852", "Todo"),
			test_support::issue("XY-G1", "Todo"),
			test_support::issue("XY-G2", "Todo"),
		])
		.with_update_failure_after_successes(1);
	let config = test_support::test_config();
	let workflow = test_support::workflow();
	let error = program_intake::run_goal_intake(GoalIntakeRunRequest {
		state_store: &store,
		tracker: &tracker,
		config: &config,
		workflow: &workflow,
		contract_id: "goal-intake-contract",
		team_issue_identifier: None,
		dry_run: false,
		apply: true,
	})
	.expect_err("second issue update should fail");

	assert!(error.to_string().contains("injected update failure"));
	assert_eq!(tracker.updated_issue_count(), 1);

	let linked_contract = store
		.decision_contract("decodex", "goal-intake-contract")
		.expect("contract lookup should read")
		.expect("contract should exist");

	assert_eq!(
		linked_contract.contract().links().generated_issue_identifiers(),
		&[String::from("XY-G1"), String::from("XY-G2")]
	);
	assert_eq!(linked_contract.contract().links().execution_program_node_ids().len(), 2);
	assert_eq!(linked_contract.contract().links().execution_program_node_ids()[1], "old-node-2");
	assert!(store.list_execution_programs("decodex").expect("programs").is_empty());
}

#[test]
fn goal_intake_apply_fails_closed_when_existing_generated_link_is_missing() {
	let store = StateStore::open_in_memory().expect("store should open");
	let mut contract = test_support::accepted_goal_contract();

	contract
		.link_generated_execution_surfaces(["id-XY-G1"], ["XY-G1"], ["old-node"])
		.expect("existing generated link should attach");
	store
		.upsert_decision_contract("decodex", Some("XY-852"), contract)
		.expect("contract should persist");

	let tracker = FakeTracker::default().with_issues([test_support::issue("XY-852", "Todo")]);
	let config = test_support::test_config();
	let workflow = test_support::workflow();
	let error = program_intake::run_goal_intake(GoalIntakeRunRequest {
		state_store: &store,
		tracker: &tracker,
		config: &config,
		workflow: &workflow,
		contract_id: "goal-intake-contract",
		team_issue_identifier: None,
		dry_run: false,
		apply: true,
	})
	.expect_err("missing generated issue link should block apply");

	assert!(error.to_string().contains("Generated issue link `XY-G1`"));
	assert_eq!(tracker.created_issue_count(), 0);
	assert_eq!(tracker.updated_issue_count(), 0);
	assert!(store.list_execution_programs("decodex").expect("programs").is_empty());
}
