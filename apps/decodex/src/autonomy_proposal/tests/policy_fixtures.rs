use crate::autonomy_proposal::{
	AUTONOMY_PROPOSAL_ACCEPTANCE_SCOPE, AutonomyProposalAcceptedProjectPolicy,
	AutonomyProposalAuthorityActorKind, AutonomyProposalDecisionBridgeAuthority,
	AutonomyProposalDecisionBridgeAuthorityInput,
};

pub(crate) fn accepted_project_policy_fixture(
	objective_id: &str,
	authorized_actor: &str,
	authorized_actor_kind: AutonomyProposalAuthorityActorKind,
	acceptance_source: &str,
	acceptance_scope: &str,
) -> AutonomyProposalAcceptedProjectPolicy {
	AutonomyProposalAcceptedProjectPolicy {
		project_id: String::from("decodex"),
		objective_id: objective_id.to_owned(),
		objective_version: 1,
		accepted_policy_id: String::from("quality-autonomy-policy"),
		accepted_policy_version: String::from("1"),
		authority_ref: String::from("decodex.runtime_policy:quality-autonomy-policy@1"),
		authorized_actor: authorized_actor.to_owned(),
		authorized_actor_kind,
		authorized_acceptance_sources: vec![acceptance_source.to_owned()],
		authorized_scopes: vec![acceptance_scope.to_owned()],
		public_non_goals: vec![String::from("Do not bypass accepted review authority.")],
	}
}

pub(crate) fn decision_bridge_authority_input(
	accepted_by: &str,
	accepted_by_kind: AutonomyProposalAuthorityActorKind,
	acceptance_source: &str,
	reason: &str,
	proposal_actor: &str,
	proposal_actor_kind: AutonomyProposalAuthorityActorKind,
	accepted_project_policy: Option<AutonomyProposalAcceptedProjectPolicy>,
) -> AutonomyProposalDecisionBridgeAuthorityInput {
	AutonomyProposalDecisionBridgeAuthorityInput {
		accepted_by: accepted_by.to_owned(),
		accepted_by_kind,
		accepted_at: String::from("2026-06-22T00:03:00Z"),
		acceptance_source: acceptance_source.to_owned(),
		reason: reason.to_owned(),
		proposal_actor: proposal_actor.to_owned(),
		proposal_actor_kind,
		accepted_project_policy,
	}
}
pub(crate) fn accepted_project_policy(
	authorized_actor: &str,
	authorized_actor_kind: AutonomyProposalAuthorityActorKind,
	acceptance_source: &str,
) -> AutonomyProposalAcceptedProjectPolicy {
	accepted_project_policy_fixture(
		"quality-autonomy",
		authorized_actor,
		authorized_actor_kind,
		acceptance_source,
		AUTONOMY_PROPOSAL_ACCEPTANCE_SCOPE,
	)
}

pub(crate) fn runtime_policy_bridge_authority() -> AutonomyProposalDecisionBridgeAuthority {
	AutonomyProposalDecisionBridgeAuthority::new(AutonomyProposalDecisionBridgeAuthorityInput {
		accepted_by: String::from("subagent"),
		accepted_by_kind: AutonomyProposalAuthorityActorKind::ExternalAgent,
		accepted_at: String::from("2026-06-22T00:03:00Z"),
		acceptance_source: String::from("runtime-policy"),
		reason: String::from("Accepted project policy allows this agent to accept the proposal."),
		proposal_actor: String::from("subagent"),
		proposal_actor_kind: AutonomyProposalAuthorityActorKind::ExternalAgent,
		accepted_project_policy: Some(accepted_project_policy(
			"subagent",
			AutonomyProposalAuthorityActorKind::ExternalAgent,
			"runtime-policy",
		)),
	})
	.expect("policy-backed bridge authority should validate")
}
