use tempfile::TempDir;

use crate::{
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
