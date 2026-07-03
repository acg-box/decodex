use crate::autonomy_proposal::{
	AutonomyProposal, AutonomyProposalChallengeInput, AutonomyProposalChallengeSource,
	AutonomyProposalState, tests,
};

#[test]
fn autonomy_proposal_challenge_records_objections_without_acceptance_authority() {
	let objective = tests::objective_fixture();
	let signal = tests::runtime_signal();
	let mut proposal =
		AutonomyProposal::compile_dry_run(Some(&objective), &[signal], tests::compile_input())
			.expect("proposal should compile");
	let proposal_id = proposal.id().to_owned();

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

	assert_eq!(proposal.id(), proposal_id);
	assert_eq!(proposal.state(), AutonomyProposalState::DecisionCandidate);
	assert_eq!(proposal.challenge_evidence().len(), 1);
	assert!(!proposal.challenge_evidence()[0].acceptance_authority);
	assert_eq!(
		proposal.challenge_evidence()[0].objections,
		["Needs a fresher operator status readback."]
	);

	let dry_run_json = serde_json::to_value(&proposal).expect("proposal should encode");

	assert_eq!(dry_run_json["challenge_evidence"][0]["acceptance_authority"], false);
	assert_eq!(
		dry_run_json["challenge_evidence"][0]["objections"][0],
		"Needs a fresher operator status readback."
	);

	let candidate = proposal
		.to_decision_contract_candidate(tests::bridge_authority())
		.expect("challenge objections should remain promotion constraints");

	assert!(candidate.accepted_authority().constraints().contains(&String::from(
		"Challenge promotion constraint: Needs a fresher operator status readback."
	)));
	assert!(
		candidate
			.accepted_authority()
			.objections()
			.contains(&String::from("Needs a fresher operator status readback."))
	);
}
