use serde_json::Value;

use crate::autonomy_proposal::{AutonomyProposal, AutonomyProposalDecisionBridgeAuthority};

pub(super) fn autonomy_decision_research_provenance(
	proposal: &AutonomyProposal,
	authority: &AutonomyProposalDecisionBridgeAuthority,
) -> Vec<Value> {
	let mut provenance = vec![
		serde_json::json!({
			"kind": "autonomy_proposal",
			"reference": proposal.id.clone(),
			"summary": proposal.summary.clone(),
		}),
		serde_json::json!({
			"kind": "autonomy_objective",
			"reference": format!(
				"{}:{}@{}",
				proposal.project_id, proposal.objective_id, proposal.objective_version
			),
			"summary": proposal.objective_lineage.objective_summary.as_deref().unwrap_or(
				"Accepted autonomy objective version."
			),
		}),
		serde_json::json!({
			"kind": "proposal_acceptance",
			"reference": authority.acceptance_source.clone(),
			"summary": format!(
				"Accepted by {} ({}) at {}.",
				authority.accepted_by,
				authority.accepted_by_kind.as_str(),
				authority.accepted_at
			),
		}),
	];

	if let Some(policy) = &authority.accepted_project_policy {
		provenance.push(serde_json::json!({
			"kind": "project_policy",
			"reference": policy.authority_ref.clone(),
			"summary": format!(
				"Accepted project policy {}@{} authorized `{}` ({}) for autonomy proposal acceptance.",
				policy.accepted_policy_id,
				policy.accepted_policy_version,
				policy.authorized_actor,
				policy.authorized_actor_kind.as_str()
			)
		}));
	}

	provenance
}
