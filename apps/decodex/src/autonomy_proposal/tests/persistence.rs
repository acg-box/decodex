use crate::autonomy_proposal::tests::ExpectNone;
use crate::{
	autonomy_proposal::{
		AutonomyProposal, AutonomyProposalChallengeInput, AutonomyProposalChallengeSource,
		AutonomyProposalState, tests,
	},
	state::StateStore,
};

#[test]
fn autonomy_proposal_store_round_trips_without_execution_authority_side_effects() {
	let store = StateStore::open_in_memory().expect("store should open");
	let objective = tests::store_accepted_objective(&store);

	objective.supersession().expect_none("accepted fixture must not have supersession");

	let signal = store
		.record_autonomy_signal("decodex", tests::runtime_signal())
		.expect("signal should store")
		.signal()
		.clone();
	let proposal =
		AutonomyProposal::compile_dry_run(Some(&objective), &[signal], tests::compile_input())
			.expect("proposal should compile");
	let stored = store
		.record_autonomy_proposal("decodex", proposal.clone())
		.expect("proposal should persist");

	assert_eq!(stored.proposal(), &proposal);
	assert_eq!(
		store
			.autonomy_proposal("decodex", proposal.id())
			.expect("proposal read should work")
			.expect("proposal should exist")
			.proposal(),
		&proposal
	);
	assert!(
		store
			.list_decision_contracts_for_project("decodex")
			.expect("decision contracts should list")
			.is_empty()
	);
	assert!(store.list_execution_programs("decodex").expect("programs should list").is_empty());
	assert!(
		store.list_program_intake_plans("decodex").expect("intake plans should list").is_empty()
	);
}

#[test]
fn autonomy_proposal_sqlite_round_trips_full_dry_run_record() {
	let tempdir = tempfile::tempdir().expect("tempdir should create");
	let db_path = tempdir.path().join("runtime.sqlite3");
	let stored_proposal = {
		let store = StateStore::open(&db_path).expect("store should open");
		let objective = tests::store_accepted_objective(&store);
		let signal = store
			.record_autonomy_signal("decodex", tests::runtime_signal())
			.expect("signal should store")
			.signal()
			.clone();
		let mut proposal =
			AutonomyProposal::compile_dry_run(Some(&objective), &[signal], tests::compile_input())
				.expect("proposal should compile");

		proposal
			.record_challenge(AutonomyProposalChallengeInput {
				source: AutonomyProposalChallengeSource::Subagent,
				actor: String::from("subagent"),
				summary: String::from("Subagent challenged the evidence sufficiency."),
				objections: vec![String::from("Needs a fresher operator status readback.")],
				evidence_refs: vec![String::from("challenge:subagent")],
				recorded_at: String::from("2026-06-22T00:02:00Z"),
			})
			.expect("challenge should record");
		store
			.record_autonomy_proposal("decodex", proposal.clone())
			.expect("proposal should persist");

		proposal
	};
	let reopened = StateStore::open(&db_path).expect("store should reopen");
	let readback = reopened
		.autonomy_proposal("decodex", stored_proposal.id())
		.expect("proposal read should work")
		.expect("proposal should exist");

	assert_eq!(readback.proposal(), &stored_proposal);
	assert_eq!(readback.state(), AutonomyProposalState::DecisionCandidate);
	assert_eq!(
		reopened
			.recent_autonomy_proposals_for_project("decodex", 1)
			.expect("recent proposals should list")[0]
			.proposal(),
		&stored_proposal
	);
	assert!(
		reopened
			.list_decision_contracts_for_project("decodex")
			.expect("decision contracts should list")
			.is_empty()
	);
	assert!(reopened.list_execution_programs("decodex").expect("programs should list").is_empty());
	assert!(
		reopened.list_program_intake_plans("decodex").expect("intake plans should list").is_empty()
	);
}
