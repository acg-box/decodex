use std::slice;

use crate::{
	autonomy_proposal::{AutonomyProposal, AutonomyProposalState, tests},
	autonomy_signal::AutonomySignal,
	state::StateStore,
};

#[test]
fn rejected_and_needs_human_remain_non_executable() {
	let store = StateStore::open_in_memory().expect("store should open");
	let objective = tests::store_accepted_objective(&store);
	let signal = store
		.record_autonomy_signal("decodex", tests::runtime_signal())
		.expect("signal should store")
		.signal()
		.clone();
	let mut rejected_input = tests::compile_input();

	rejected_input.intended_surface = String::from("scripts/unowned.rs");

	let rejected = AutonomyProposal::compile_dry_run(
		Some(&objective),
		slice::from_ref(&signal),
		rejected_input,
	)
	.expect("rejected proposal should compile");
	let rejected_id = rejected.id().to_owned();

	assert_eq!(rejected.state(), AutonomyProposalState::Rejected);

	store.record_autonomy_proposal("decodex", rejected).expect("rejected proposal should persist");

	assert!(
		store
			.accept_autonomy_proposal_as_decision_contract_candidate(
				"decodex",
				&rejected_id,
				tests::bridge_authority(),
			)
			.is_err()
	);

	let mut contradiction_input = tests::signal_input();

	contradiction_input.contradictions =
		vec![String::from("Runtime and tracker authority disagree.")];

	let contradiction_signal = store
		.record_autonomy_signal(
			"decodex",
			AutonomySignal::runtime_health(contradiction_input).expect("signal should validate"),
		)
		.expect("contradiction signal should store")
		.signal()
		.clone();
	let needs_human = AutonomyProposal::compile_dry_run(
		Some(&objective),
		&[contradiction_signal],
		tests::compile_input(),
	)
	.expect("needs-human proposal should compile");
	let needs_human_id = needs_human.id().to_owned();

	assert_eq!(needs_human.state(), AutonomyProposalState::NeedsHumanDecision);

	store
		.record_autonomy_proposal("decodex", needs_human)
		.expect("needs-human proposal should persist");

	assert!(
		store
			.accept_autonomy_proposal_as_decision_contract_candidate(
				"decodex",
				&needs_human_id,
				tests::bridge_authority(),
			)
			.is_err()
	);
	assert!(
		store
			.list_decision_contracts_for_project("decodex")
			.expect("contracts should list")
			.is_empty()
	);
	assert!(store.list_execution_programs("decodex").expect("programs").is_empty());
	assert!(store.list_program_intake_plans("decodex").expect("intake plans").is_empty());
}
