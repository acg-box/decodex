use rusqlite::Connection;

use crate::{
	autonomy_runtime_policy,
	program_intake::{
		self, GoalIntakeRunRequest,
		tests::test_support::{self, FakeTracker},
	},
	state::{ProgramIntakeAttemptClaim, StateStore},
	tracker::IssueTracker,
};

#[test]
fn program_intake_attempt_claim_is_one_shot_in_memory() {
	let store = StateStore::open_in_memory().expect("store should open");

	assert_eq!(
		store
			.begin_program_intake_attempt("decodex", "contract-1", "digest-1")
			.expect("first claim should succeed"),
		ProgramIntakeAttemptClaim::Acquired
	);
	assert_eq!(
		store
			.begin_program_intake_attempt("decodex", "contract-1", "digest-1")
			.expect("replay should read"),
		ProgramIntakeAttemptClaim::Prepared
	);

	store
		.mark_program_intake_attempt_started("decodex", "contract-1")
		.expect("external mutation start should persist");
	store
		.complete_program_intake_attempt("decodex", "contract-1")
		.expect("completion should persist");

	assert_eq!(
		store
			.begin_program_intake_attempt("decodex", "contract-1", "digest-1")
			.expect("completed replay should read"),
		ProgramIntakeAttemptClaim::Completed
	);
}

#[test]
fn program_intake_attempt_claim_survives_reopen_and_blocks_second_store() {
	let temp_dir = tempfile::tempdir().expect("temp dir should create");
	let db_path = temp_dir.path().join("runtime.sqlite3");
	let first = StateStore::open(&db_path).expect("first store should open");
	let second = StateStore::open(&db_path).expect("second store should open");

	assert_eq!(
		first
			.begin_program_intake_attempt("decodex", "contract-1", "digest-1")
			.expect("first claim should succeed"),
		ProgramIntakeAttemptClaim::Acquired
	);
	assert_eq!(
		second
			.begin_program_intake_attempt("decodex", "contract-1", "digest-1")
			.expect("second store should read durable claim"),
		ProgramIntakeAttemptClaim::Prepared
	);

	drop(first);
	drop(second);

	let reopened = StateStore::open(&db_path).expect("store should reopen");

	assert_eq!(
		reopened
			.begin_program_intake_attempt("decodex", "contract-1", "digest-1")
			.expect("reopened store should read durable claim"),
		ProgramIntakeAttemptClaim::Prepared
	);
}

#[test]
fn program_intake_attempt_rejects_changed_bound_inputs() {
	let temp_dir = tempfile::tempdir().expect("temp dir should create");

	for store in [
		StateStore::open_in_memory().expect("memory store should open"),
		StateStore::open(temp_dir.path().join("runtime.sqlite3"))
			.expect("persistent store should open"),
	] {
		store
			.begin_program_intake_attempt("decodex", "contract-1", "digest-team-a")
			.expect("first bound claim should succeed");

		let error = store
			.begin_program_intake_attempt("decodex", "contract-1", "digest-team-b")
			.expect_err("changed team/config/workflow digest must be refused");

		assert!(error.to_string().contains("program_intake_attempt_request_mismatch"));
	}
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

	assert_eq!(
		autonomy_runtime_policy::program_intake_state_for_contract(
			&store,
			"decodex",
			"goal-intake-contract",
		)
		.expect("exact intake readback should evaluate"),
		autonomy_runtime_policy::RuntimePolicyProgramIntakeState::Complete
	);

	let record = store
		.decision_contract("decodex", "goal-intake-contract")
		.expect("contract should read")
		.expect("contract should exist");
	let mut malformed = record.contract().clone();

	malformed
		.link_generated_execution_surfaces(
			malformed.links().generated_issue_ids().to_vec(),
			["XY-WRONG"],
			malformed.links().execution_program_node_ids().to_vec(),
		)
		.expect("malformed correspondence remains structurally valid");
	store
		.upsert_decision_contract("decodex", record.source_issue_id(), malformed)
		.expect("malformed fixture should persist");

	assert_eq!(
		autonomy_runtime_policy::program_intake_state_for_contract(
			&store,
			"decodex",
			"goal-intake-contract",
		)
		.expect("malformed intake readback should evaluate"),
		autonomy_runtime_policy::RuntimePolicyProgramIntakeState::Inconsistent
	);

	let updated = tracker
		.get_issue_by_identifier("XY-G1")
		.expect("issue lookup should work")
		.expect("updated issue should exist");

	test_support::assert_goal_issue_brief_is_public(&updated.description, &report);
}

#[test]
fn program_intake_complete_rejects_tampered_plan_metadata() {
	let temp_dir = tempfile::tempdir().expect("temp dir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let store = StateStore::open(&state_path).expect("store should open");

	store
		.upsert_decision_contract("decodex", Some("XY-852"), test_support::accepted_goal_contract())
		.expect("contract should persist");

	let tracker = FakeTracker::default().with_issues([test_support::issue("XY-852", "Todo")]);
	let config = test_support::test_config();
	let workflow = test_support::workflow();

	program_intake::run_goal_intake(GoalIntakeRunRequest {
		state_store: &store,
		tracker: &tracker,
		config: &config,
		workflow: &workflow,
		contract_id: "goal-intake-contract",
		team_issue_identifier: None,
		dry_run: false,
		apply: true,
	})
	.expect("apply should materialize exact Program Intake state");

	assert_eq!(
		autonomy_runtime_policy::program_intake_state_for_contract(
			&store,
			"decodex",
			"goal-intake-contract",
		)
		.expect("complete state should classify"),
		autonomy_runtime_policy::RuntimePolicyProgramIntakeState::Complete
	);

	Connection::open(&state_path)
		.expect("sqlite should open")
		.execute(
			"UPDATE program_intake_plans SET public_summary = 'tampered' WHERE project_id = 'decodex'",
			[],
		)
		.expect("plan should tamper for counterexample");

	assert_eq!(
		autonomy_runtime_policy::program_intake_state_for_contract(
			&store,
			"decodex",
			"goal-intake-contract",
		)
		.expect("tampered state should classify"),
		autonomy_runtime_policy::RuntimePolicyProgramIntakeState::Inconsistent
	);
}

#[test]
fn program_intake_absent_rejects_orphan_plan_under_expected_program_id() {
	let temp_dir = tempfile::tempdir().expect("temp dir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let store = StateStore::open(&state_path).expect("store should open");

	store
		.upsert_decision_contract("decodex", Some("XY-852"), test_support::accepted_goal_contract())
		.expect("contract should persist");

	let expected_program_id = program_intake::goal_program_id("decodex", "goal-intake-contract");

	Connection::open(&state_path)
		.expect("sqlite should open")
		.execute(
			"INSERT INTO program_intake_plans (
			 project_id, program_id, plan_id, intake_kind, source_contract_id,
			 accepted_contract_fingerprint, public_summary, created_at, created_at_unix,
			 updated_at, updated_at_unix
			) VALUES ('decodex', ?1, 'orphan-plan', 'goal_intake', 'wrong-contract',
			 'sha256:wrong', 'orphan', '2026-07-10T00:00:00Z', 1,
			 '2026-07-10T00:00:00Z', 1)",
			rusqlite::params![expected_program_id],
		)
		.expect("orphan plan should seed");

	assert_eq!(
		autonomy_runtime_policy::program_intake_state_for_contract(
			&store,
			"decodex",
			"goal-intake-contract",
		)
		.expect("orphan state should classify"),
		autonomy_runtime_policy::RuntimePolicyProgramIntakeState::Inconsistent
	);
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
