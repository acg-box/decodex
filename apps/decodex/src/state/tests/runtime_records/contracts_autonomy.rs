use tempfile::TempDir;

use crate::{
	autonomy_objective::{
		AutonomyObjectiveRejection, AutonomyObjectiveState, AutonomyObjectiveSupersession,
	},
	loop_contract::DecisionContractStatus,
	state::{StateStore, tests},
};

#[test]
fn decision_contracts_persist_reload_and_promote_without_linear_mirror() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let store = StateStore::open(&state_path).expect("state store should open");
	let latent = tests::latent_decision_contract_fixture();
	let record = store
		.upsert_decision_contract("decodex", Some("XY-852"), latent)
		.expect("latent decision contract should persist");

	assert_eq!(record.project_id(), "decodex");
	assert_eq!(record.source_issue_id(), Some("XY-852"));
	assert_eq!(record.contract_id(), "research-x-loop-contract");
	assert_eq!(record.status(), DecisionContractStatus::DraftLatent);
	assert!(record.created_at_unix() > 0);
	assert!(record.updated_at_unix() >= record.created_at_unix());

	let promoted = store
		.promote_decision_contract(
			"decodex",
			"research-x-loop-contract",
			tests::sample_decision_promotion(),
		)
		.expect("latent contract should promote");

	assert_eq!(promoted.status(), DecisionContractStatus::AcceptedPromoted);
	assert_eq!(
		promoted.contract().promotion().expect("promotion metadata should persist").accepted_by(),
		"operator"
	);
	assert!(
		store
			.list_linear_execution_events("decodex", "XY-852")
			.expect("linear mirror should read")
			.is_empty(),
		"decision contracts stay in runtime SQLite and do not populate Linear cache"
	);

	let reopened = StateStore::open(&state_path).expect("state store should reopen");
	let reloaded = reopened
		.decision_contract("decodex", "research-x-loop-contract")
		.expect("decision contract should read")
		.expect("decision contract should exist");

	assert_eq!(reloaded.status(), DecisionContractStatus::AcceptedPromoted);
	assert_eq!(reloaded.source_issue_id(), Some("XY-852"));
	assert_eq!(reloaded.created_at(), record.created_at());
	assert!(reloaded.updated_at_unix() >= record.updated_at_unix());
	assert_eq!(reloaded.contract().accepted_authority().accepted_objectives().len(), 2);

	let issue_contracts = reopened
		.list_decision_contracts_for_issue("decodex", "XY-852")
		.expect("source issue contracts should list");

	assert_eq!(issue_contracts.len(), 1);
	assert_eq!(issue_contracts[0].contract_id(), "research-x-loop-contract");

	let project_contracts = reopened
		.list_decision_contracts_for_project("decodex")
		.expect("project contracts should list");

	assert_eq!(project_contracts.len(), 1);
	assert_eq!(project_contracts[0].contract_id(), "research-x-loop-contract");
}

#[test]
fn decision_contracts_record_human_decision_and_rejection_transitions() {
	let store = StateStore::open_in_memory().expect("in-memory state store should open");

	store
		.upsert_decision_contract(
			"decodex",
			Some("XY-852"),
			tests::latent_decision_contract_fixture(),
		)
		.expect("latent decision contract should persist");

	let waiting = store
		.mark_decision_contract_needs_human_decision(
			"decodex",
			"research-x-loop-contract",
			"Choose which generated issue should run first.",
		)
		.expect("contract should record human decision need");

	assert_eq!(waiting.status(), DecisionContractStatus::NeedsHumanDecision);
	assert!(
		waiting
			.contract()
			.execution_readiness()
			.missing_decisions()
			.iter()
			.any(|decision| decision == "Choose which generated issue should run first.")
	);

	let rejected = store
		.reject_decision_contract(
			"decodex",
			"research-x-loop-contract",
			Some(String::from("research-x-loop-contract-v2")),
		)
		.expect("contract should reject");

	assert_eq!(rejected.status(), DecisionContractStatus::RejectedSuperseded);
	assert_eq!(
		rejected.contract().links().superseded_by_contract_id(),
		Some("research-x-loop-contract-v2")
	);
	assert!(
		store
			.promote_decision_contract(
				"decodex",
				"research-x-loop-contract",
				tests::sample_decision_promotion()
			)
			.is_err(),
		"rejected contracts cannot later become execution authority"
	);
}

#[test]
fn autonomy_objective_draft_accept_current_history_and_supersession_persist() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let store = StateStore::open(&state_path).expect("state store should open");
	let draft_v1 = store
		.upsert_autonomy_objective_draft("decodex", tests::autonomy_objective_fixture(1))
		.expect("objective draft v1 should persist");

	assert_eq!(draft_v1.project_id(), "decodex");
	assert_eq!(draft_v1.objective_id(), "quality-autonomy");
	assert_eq!(draft_v1.version(), 1);
	assert_eq!(draft_v1.state(), AutonomyObjectiveState::Draft);
	assert_eq!(
		store
			.autonomy_objective("decodex", "quality-autonomy", 1)
			.expect("draft objective should read")
			.expect("draft objective should exist")
			.state(),
		AutonomyObjectiveState::Draft
	);

	let accepted_v1 = store
		.accept_autonomy_objective_version(
			"decodex",
			"quality-autonomy",
			1,
			tests::sample_objective_acceptance(),
		)
		.expect("objective v1 should accept");

	assert_eq!(accepted_v1.state(), AutonomyObjectiveState::Accepted);
	assert_eq!(
		accepted_v1.objective().acceptance().expect("acceptance should be retained").accepted_by(),
		"operator"
	);
	assert_eq!(
		store
			.autonomy_objective("decodex", "quality-autonomy", 1)
			.expect("accepted objective should read")
			.expect("accepted objective should exist")
			.state(),
		AutonomyObjectiveState::Accepted
	);
	assert!(
		store
			.upsert_autonomy_objective_draft("decodex", tests::autonomy_objective_fixture(1))
			.is_err(),
		"accepted objective versions must not be overwritten as drafts"
	);

	store
		.upsert_autonomy_objective_draft("decodex", tests::autonomy_objective_fixture(2))
		.expect("objective draft v2 should persist");

	let accepted_v2 = store
		.accept_autonomy_objective_version(
			"decodex",
			"quality-autonomy",
			2,
			tests::sample_objective_acceptance(),
		)
		.expect("objective v2 should accept and supersede v1");

	assert_eq!(accepted_v2.version(), 2);
	assert_eq!(accepted_v2.state(), AutonomyObjectiveState::Accepted);

	let current = store
		.current_accepted_autonomy_objective("decodex", "quality-autonomy")
		.expect("current accepted objective should read")
		.expect("current accepted objective should exist");

	assert_eq!(current.version(), 2);

	let reopened = StateStore::open(&state_path).expect("state store should reopen");
	let history = reopened
		.list_autonomy_objective_history("decodex", "quality-autonomy")
		.expect("objective history should list");

	assert_eq!(history.len(), 2);
	assert_eq!(history[0].version(), 1);
	assert_eq!(history[0].state(), AutonomyObjectiveState::Superseded);
	assert_eq!(
		history[0]
			.objective()
			.supersession()
			.expect("supersession should be retained")
			.superseded_by_version(),
		2
	);
	assert_eq!(
		history[0].objective().summary(),
		"Improve Decodex autonomy quality version 1.",
		"superseding an accepted version must preserve its objective body"
	);
	assert_eq!(history[1].version(), 2);
	assert_eq!(history[1].state(), AutonomyObjectiveState::Accepted);
}

#[test]
fn autonomy_objective_rejection_and_explicit_supersession_keep_provenance() {
	let store = StateStore::open_in_memory().expect("in-memory state store should open");

	store
		.upsert_autonomy_objective_draft("decodex", tests::autonomy_objective_fixture(1))
		.expect("objective draft v1 should persist");

	let rejected = store
		.reject_autonomy_objective_version(
			"decodex",
			"quality-autonomy",
			1,
			AutonomyObjectiveRejection::new(
				"operator",
				"2026-06-22T10:05:00Z",
				"conversation",
				"Objective version needs narrower surfaces.",
			)
			.expect("rejection should validate"),
		)
		.expect("objective draft should reject");

	assert_eq!(rejected.state(), AutonomyObjectiveState::Rejected);
	assert_eq!(
		rejected.objective().rejection().expect("rejection should exist").reason(),
		"Objective version needs narrower surfaces."
	);
	assert_eq!(
		store
			.autonomy_objective("decodex", "quality-autonomy", 1)
			.expect("rejected objective should read")
			.expect("rejected objective should exist")
			.state(),
		AutonomyObjectiveState::Rejected
	);
	assert!(
		store
			.accept_autonomy_objective_version(
				"decodex",
				"quality-autonomy",
				1,
				tests::sample_objective_acceptance()
			)
			.is_err(),
		"rejected objective versions cannot later become accepted authority"
	);

	store
		.upsert_autonomy_objective_draft("decodex", tests::autonomy_objective_fixture(2))
		.expect("objective draft v2 should persist");

	let superseded = store
		.supersede_autonomy_objective_version(
			"decodex",
			"quality-autonomy",
			2,
			AutonomyObjectiveSupersession::new(
				"quality-autonomy",
				3,
				"operator",
				"2026-06-22T10:10:00Z",
				"conversation",
				"Draft was replaced before acceptance.",
			)
			.expect("supersession should validate"),
		)
		.expect("objective draft should supersede");

	assert_eq!(superseded.state(), AutonomyObjectiveState::Superseded);
	assert_eq!(
		superseded
			.objective()
			.supersession()
			.expect("supersession should exist")
			.superseded_by_version(),
		3
	);
	assert_eq!(
		store
			.autonomy_objective("decodex", "quality-autonomy", 2)
			.expect("superseded objective should read")
			.expect("superseded objective should exist")
			.state(),
		AutonomyObjectiveState::Superseded
	);
	assert_eq!(
		store
			.upsert_autonomy_objective_draft("decodex", tests::autonomy_objective_fixture(3))
			.expect("objective draft v3 should persist")
			.state(),
		AutonomyObjectiveState::Draft
	);
	assert!(
		store
			.supersede_autonomy_objective_version(
				"decodex",
				"quality-autonomy",
				3,
				AutonomyObjectiveSupersession::new(
					"quality-autonomy",
					3,
					"operator",
					"2026-06-22T10:11:00Z",
					"conversation",
					"Invalid self-supersession.",
				)
				.expect("self-supersession payload should build"),
			)
			.is_err(),
		"same objective version cannot supersede itself"
	);
}
