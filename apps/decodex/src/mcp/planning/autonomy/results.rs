use serde_json::{self, Value};

use crate::{
	autonomy_objective::AutonomyObjectiveContract,
	autonomy_proposal::AutonomyProposal,
	autonomy_signal::AutonomySignal,
	mcp::{autonomy_resources, observability},
};

pub(in crate::mcp) fn autonomy_objective_tool_result(
	project_id: &str,
	objective: &AutonomyObjectiveContract,
	mode: &str,
	persisted: bool,
	updated_at: Option<&str>,
) -> Value {
	observability::mcp_sanitized_value(serde_json::json!({
		"schema": "decodex.mcp.autonomy_objective_result/1",
		"status": "ok",
		"mode": mode,
		"persisted": persisted,
		"project_id": project_id,
		"objective": autonomy_resources::mcp_autonomy_objective_summary(objective, updated_at),
		"authority_effect": "draft_only_no_execution_authority",
		"next_action": "Accept an Objective Contract only through explicit human or accepted-policy authority; MCP profile access is not acceptance authority.",
		"updated_at": updated_at
	}))
}

pub(in crate::mcp) fn autonomy_objective_accept_tool_result(
	project_id: &str,
	objective: &AutonomyObjectiveContract,
	mode: &str,
	persisted: bool,
	updated_at: Option<&str>,
) -> Value {
	observability::mcp_sanitized_value(serde_json::json!({
		"schema": "decodex.mcp.autonomy_objective_result/1",
		"status": "ok",
		"mode": mode,
		"persisted": persisted,
		"project_id": project_id,
		"objective": autonomy_resources::mcp_autonomy_objective_summary(objective, updated_at),
		"authority_effect": "accepted_objective_no_execution_authority",
		"next_action": "Accepted Objective Contracts allow objective-bound signals and proposals; execution still requires proposal acceptance, Decision Contract promotion, and Program Intake.",
		"updated_at": updated_at
	}))
}

pub(in crate::mcp) fn autonomy_signal_tool_result(
	project_id: &str,
	signal: &AutonomySignal,
	mode: &str,
	persisted: bool,
	updated_at: Option<&str>,
) -> Value {
	observability::mcp_sanitized_value(serde_json::json!({
		"schema": "decodex.mcp.autonomy_signal_result/1",
		"status": "ok",
		"mode": mode,
		"persisted": persisted,
		"project_id": project_id,
		"signal": autonomy_resources::mcp_autonomy_signal_summary(signal, updated_at),
		"authority_effect": "proposal_only_evidence_no_execution_authority",
		"next_action": "Cluster accepted-objective signals into a non-executable proposal before any Decision Contract promotion.",
		"updated_at": updated_at
	}))
}

pub(in crate::mcp) fn autonomy_proposal_tool_result(
	project_id: &str,
	proposal: &AutonomyProposal,
	mode: &str,
	persisted: bool,
	updated_at: Option<&str>,
) -> Value {
	observability::mcp_sanitized_value(serde_json::json!({
		"schema": "decodex.mcp.autonomy_proposal_result/1",
		"status": "ok",
		"mode": mode,
		"persisted": persisted,
		"project_id": project_id,
		"proposal": autonomy_resources::mcp_autonomy_proposal_summary(proposal, updated_at),
		"authority_effect": "non_executable_proposal_evidence",
		"next_action": "Challenge the proposal and request explicit promotion authority before creating a latent Decision Contract candidate.",
		"updated_at": updated_at
	}))
}

pub(in crate::mcp) fn autonomy_challenge_tool_result(
	project_id: &str,
	proposal: &AutonomyProposal,
	mode: &str,
	persisted: bool,
	updated_at: Option<&str>,
) -> Value {
	observability::mcp_sanitized_value(serde_json::json!({
		"schema": "decodex.mcp.autonomy_challenge_result/1",
		"status": "ok",
		"mode": mode,
		"persisted": persisted,
		"project_id": project_id,
		"proposal": autonomy_resources::mcp_autonomy_proposal_summary(proposal, updated_at),
		"challenge_evidence_count": proposal.challenge_evidence().len(),
		"authority_effect": "challenge_evidence_not_acceptance_authority",
		"next_action": "Carry challenge objections as promotion constraints and request explicit promotion authority before creating execution work.",
		"updated_at": updated_at
	}))
}

pub(in crate::mcp) fn autonomy_promotion_request_result(
	project_id: &str,
	proposal: &AutonomyProposal,
	mode: &str,
	persisted: bool,
	decision_contract_id: Option<&str>,
) -> Value {
	observability::mcp_sanitized_value(serde_json::json!({
		"schema": "decodex.mcp.autonomy_promotion_request_result/1",
		"status": "ok",
		"mode": mode,
		"persisted": persisted,
		"project_id": project_id,
		"proposal": autonomy_resources::mcp_autonomy_proposal_summary(proposal, None),
		"decision_contract_id": decision_contract_id,
		"execution_authority_granted": false,
		"required_authority": [
			"acceptedBy",
			"acceptedByKind",
			"acceptanceSource",
			"reason",
			"proposalActor",
			"proposalActorKind",
			"trusted Decodex policy authority when runtime policy or external-agent self-acceptance is involved"
		],
		"authority_effect": if persisted {
			"latent_decision_contract_candidate_only"
		} else {
			"promotion_requirements_readback_only"
		},
		"next_action": if persisted {
			"Accept the resulting Decision Contract before Program Intake or issue work."
		} else {
			"Re-run with mode=apply only after explicit proposal acceptance authority is available."
		}
	}))
}
