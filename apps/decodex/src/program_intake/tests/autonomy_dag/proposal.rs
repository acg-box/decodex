use crate::{
	autonomy_objective::{AutonomyObjectiveContract, AutonomyObjectiveState},
	autonomy_proposal::{AutonomyProposalChallengeInput, AutonomyProposalChallengeSource},
	autonomy_signal::AutonomySignal,
	program_intake::tests::autonomy_dag::fixtures::{self},
	state::StateStore,
};

pub(crate) fn persist_autonomy_dag_proposal(
	store: &StateStore,
	objective: &AutonomyObjectiveContract,
) -> String {
	let signal = AutonomySignal::runtime_health(fixtures::autonomy_dag_signal_input())
		.expect("autonomy signal should validate");
	let signal_id = signal.id().to_owned();

	store
		.record_autonomy_signal("decodex", signal)
		.expect("signal should persist in isolated store");

	let proposal = store
		.compile_autonomy_proposal_dry_run(fixtures::autonomy_dag_proposal_input(), &[signal_id])
		.expect("proposal should compile explicit issue DAG from persisted evidence");
	let proposal_id = proposal.id().to_owned();

	assert_eq!(
		store
			.autonomy_objective("decodex", objective.id(), objective.version())
			.expect("objective should read back")
			.expect("objective should exist")
			.objective()
			.state(),
		AutonomyObjectiveState::Accepted
	);

	store
		.record_autonomy_proposal("decodex", proposal)
		.expect("proposal should persist in isolated store");

	let proposal_record = store
		.record_autonomy_proposal_challenge(
			"decodex",
			&proposal_id,
			AutonomyProposalChallengeInput {
				source: AutonomyProposalChallengeSource::InlineSkeptic,
				actor: String::from("isolated-skeptic"),
				summary: String::from("No blocker found for the isolated issue split."),
				objections: Vec::new(),
				evidence_refs: vec![String::from("isolated:test")],
				recorded_at: String::from("2026-06-30T00:02:00Z"),
			},
		)
		.expect("challenge evidence should persist without granting authority");

	assert_eq!(proposal_record.proposal().issue_candidates().len(), 2);
	assert_eq!(
		store
			.autonomy_proposal("decodex", &proposal_id)
			.expect("proposal should read back")
			.expect("proposal should exist")
			.proposal()
			.challenge_evidence()
			.len(),
		1
	);

	proposal_id
}
