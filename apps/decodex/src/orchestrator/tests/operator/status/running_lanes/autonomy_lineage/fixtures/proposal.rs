use crate::{
	autonomy_proposal::{
		AutonomyProposalAuthorityActorKind, AutonomyProposalCompileInput,
		AutonomyProposalDecisionBridgeAuthority,
	},
	loop_contract::{DecisionPromotion, DecisionPromotionActorKind},
	orchestrator::tests::operator::status::running_lanes::autonomy_lineage::fixtures::{
		OBJECTIVE_ID, SERVICE_ID,
	},
	state::StateStore,
	tracker::TrackerIssue,
};

pub(super) fn record_autonomy_proposals(
	state_store: &StateStore,
	issue: &TrackerIssue,
	signal_id: &str,
) -> String {
	let signal_ids = vec![signal_id.to_owned()];
	let accepted_proposal = state_store
		.compile_autonomy_proposal_dry_run(
			autonomy_proposal_input("apps/decodex/src/orchestrator/status.rs", &issue.identifier),
			&signal_ids,
		)
		.expect("proposal should compile");
	let accepted_proposal_id = accepted_proposal.id().to_owned();

	state_store
		.record_autonomy_proposal(SERVICE_ID, accepted_proposal)
		.expect("proposal should persist");

	let refused_proposal = state_store
		.compile_autonomy_proposal_dry_run(
			autonomy_proposal_input("site/src/pages/index.astro", &issue.identifier),
			&signal_ids,
		)
		.expect("refused proposal should compile");

	state_store
		.record_autonomy_proposal(SERVICE_ID, refused_proposal)
		.expect("refused proposal should persist");

	accepted_proposal_id
}

pub(super) fn promote_autonomy_proposal(state_store: &StateStore, proposal_id: &str) -> String {
	let authority = AutonomyProposalDecisionBridgeAuthority::new(
		"operator",
		AutonomyProposalAuthorityActorKind::User,
		"2026-06-23T00:02:00Z",
		"linear:XY-1089",
		"Accept autonomy lineage proposal.",
		"operator",
		AutonomyProposalAuthorityActorKind::User,
		None,
	)
	.expect("proposal authority should build");
	let decision = state_store
		.accept_autonomy_proposal_as_decision_contract_candidate(SERVICE_ID, proposal_id, authority)
		.expect("decision contract should persist");
	let decision_contract_id = decision.contract_id().to_owned();

	state_store
		.promote_decision_contract(
			SERVICE_ID,
			&decision_contract_id,
			DecisionPromotion::new(
				"operator",
				DecisionPromotionActorKind::User,
				"2026-06-23T00:03:00Z",
				"linear:XY-1089",
				Some(String::from("Promote accepted autonomy proposal.")),
			)
			.expect("promotion should build"),
		)
		.expect("decision should promote");

	decision_contract_id
}

pub(super) fn autonomy_proposal_input(
	intended_surface: &str,
	issue_identifier: &str,
) -> AutonomyProposalCompileInput {
	AutonomyProposalCompileInput {
		project_id: SERVICE_ID.to_owned(),
		objective_id: String::from(OBJECTIVE_ID),
		objective_version: 1,
		source_family: String::from("operator_status"),
		intended_surface: intended_surface.to_owned(),
		affected_identifiers: vec![issue_identifier.to_owned()],
		summary: String::from("Surface autonomy lineage in operator readback."),
		challenge_requirements: vec![String::from("Verify remote-safe projection.")],
		rejected_alternatives: vec![String::from("Ask operators to inspect SQLite manually.")],
		rollback_path: String::from("Remove the operator readback projection."),
		weakened_validation_or_review: Vec::new(),
		issue_candidates: Vec::new(),
		created_at: String::from("2026-06-23T00:02:00Z"),
	}
}
