use crate::{
	autonomy_proposal::{
		AutonomyProposal, AutonomyProposalAuthorityActorKind, AutonomyProposalChallengeInput,
		AutonomyProposalChallengeSource, AutonomyProposalDecisionBridgeAuthority,
		AutonomyProposalDecisionBridgeAuthorityInput, AutonomyProposalState,
		tests::{self},
	},
	loop_contract::DecisionContractStatus,
	state::{DecisionContractRecord, StateStore},
};

pub(crate) fn bridge_authority() -> AutonomyProposalDecisionBridgeAuthority {
	AutonomyProposalDecisionBridgeAuthority::new(AutonomyProposalDecisionBridgeAuthorityInput {
		accepted_by: String::from("operator"),
		accepted_by_kind: AutonomyProposalAuthorityActorKind::User,
		accepted_at: String::from("2026-06-22T00:03:00Z"),
		acceptance_source: String::from("conversation"),
		reason: String::from("Operator accepted the proposal for Decision Contract promotion."),
		proposal_actor: String::from("subagent"),
		proposal_actor_kind: AutonomyProposalAuthorityActorKind::ExternalAgent,
		accepted_project_policy: None,
	})
	.expect("bridge authority should validate")
}
pub(crate) fn store_challenged_autonomy_candidate() -> (StateStore, String, DecisionContractRecord)
{
	let store = StateStore::open_in_memory().expect("store should open");
	let objective = tests::store_accepted_objective(&store);
	let signal = store
		.record_autonomy_signal("decodex", tests::runtime_signal())
		.expect("signal should store")
		.signal()
		.clone();
	let mut input = tests::compile_input();

	input.affected_identifiers.push(String::from("XY-1087"));

	let mut proposal = AutonomyProposal::compile_dry_run(Some(&objective), &[signal], input)
		.expect("proposal should compile");
	let proposal_id = proposal.id().to_owned();

	proposal
		.record_challenge(AutonomyProposalChallengeInput {
			source: AutonomyProposalChallengeSource::InlineSkeptic,
			actor: String::from("inline"),
			summary: String::from("Inline skeptic found no blocker to latent conversion."),
			objections: Vec::new(),
			evidence_refs: vec![String::from("challenge:inline")],
			recorded_at: String::from("2026-06-22T00:02:00Z"),
		})
		.expect("no-objection challenge should preserve candidate state");

	assert_eq!(proposal.state(), AutonomyProposalState::DecisionCandidate);

	store.record_autonomy_proposal("decodex", proposal).expect("proposal should persist");

	let candidate = store
		.accept_autonomy_proposal_as_decision_contract_candidate(
			"decodex",
			&proposal_id,
			bridge_authority(),
		)
		.expect("accepted proposal should become a latent Decision Contract");

	(store, proposal_id, candidate)
}

pub(crate) fn assert_autonomy_candidate_shape(
	store: &StateStore,
	candidate: &DecisionContractRecord,
) {
	assert_eq!(candidate.status(), DecisionContractStatus::DraftLatent);
	assert!(candidate.contract().promotion().is_none());
	assert_eq!(candidate.source_issue_id(), Some("XY-1087"));
	assert_eq!(candidate.contract().source_intent().source_issue_identifier(), Some("XY-1087"));
	assert!(
		candidate
			.contract()
			.accepted_authority()
			.accepted_objectives()
			.contains(&String::from("Reduce repeated validation and review churn."))
	);
	assert!(
		candidate.contract().accepted_authority().constraints().contains(&String::from(
			"Review requirement: independent current-head review required"
		))
	);
	assert_eq!(
		candidate.contract().execution_readiness().validation_expectations(),
		&[String::from("cargo test -p decodex autonomy_proposal --lib")]
	);
	assert!(
		candidate
			.contract()
			.execution_readiness()
			.risk_notes()
			.contains(&String::from("Evidence gap: No dashboard comparison included."))
	);
	assert_eq!(candidate.contract().execution_readiness().proposed_issues().len(), 1);
	assert!(
		candidate.contract().execution_readiness().proposed_issues()[0]
			.conflict_domains()
			.contains(&String::from("file:apps/decodex/src/orchestrator/status.rs"))
	);
	assert!(store.list_execution_programs("decodex").expect("programs").is_empty());
	assert!(store.list_program_intake_plans("decodex").expect("intake plans").is_empty());
}
