use serde_json::{self, Value};

use crate::{
	autonomy_objective::AutonomyObjectiveContract,
	autonomy_proposal::AutonomyProposal,
	autonomy_signal::AutonomySignal,
	mcp::{autonomy_resources, observability},
	state::AutonomyRuntimePolicyRecord,
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

pub(in crate::mcp) fn autonomy_runtime_policy_acceptance_result(
	project_id: &str,
	policy: &AutonomyRuntimePolicyRecord,
	candidate_digest: &str,
	mode: &str,
	persisted: bool,
) -> Value {
	observability::mcp_sanitized_value(serde_json::json!({
		"schema": "decodex.mcp.autonomy_runtime_policy_acceptance_result/1",
		"status": "ok",
		"mode": mode,
		"persisted": persisted,
		"project_id": project_id,
		"candidate_digest": candidate_digest,
		"policy": {
			"policy_id": policy.policy_id(),
			"policy_version": policy.policy_version(),
			"objective_id": policy.objective_id(),
			"objective_version": policy.objective_version(),
			"authority_ref": policy.authority_ref(),
			"accepted_by": policy.accepted_by(),
			"accepted_at": policy.accepted_at(),
			"acceptance_source": policy.acceptance_source(),
			"public_non_goals": policy.public_non_goals(),
		},
		"authority_effect": if persisted {
			"immutable_runtime_policy_authority_accepted_no_execution_started"
		} else {
			"runtime_policy_acceptance_validation_only"
		},
		"next_action": if persisted {
			"Use autonomy_apply_runtime_policy for an exact accepted-objective proposal; Program Intake remains a separate typed call."
		} else {
			"Re-run with mode=apply only after the user explicitly accepts this exact policy record."
		}
	}))
}

#[allow(clippy::too_many_arguments)]
pub(in crate::mcp) fn autonomy_runtime_policy_apply_result(
	project_id: &str,
	proposal_id: &str,
	contract_id: &str,
	mode: &str,
	persisted: bool,
	objections: &[String],
	challenge_recorded: bool,
	execution_authority_granted: bool,
	program_intake_present: bool,
	program_intake_state: &str,
	intake_team_issue_identifier: Option<&str>,
) -> Value {
	let eligible = objections.is_empty();

	observability::mcp_sanitized_value(serde_json::json!({
		"schema": "decodex.mcp.autonomy_runtime_policy_apply_result/1",
		"status": "ok",
		"mode": mode,
		"persisted": persisted,
		"project_id": project_id,
		"proposal_id": proposal_id,
		"decision_contract_id": contract_id,
		"eligible": eligible,
		"objections": objections,
		"challenge_recorded": challenge_recorded,
		"execution_authority_granted": execution_authority_granted,
		"program_intake_present": program_intake_present,
		"program_intake_state": program_intake_state,
		"intake_team_issue_identifier": intake_team_issue_identifier,
		"authority_effect": if execution_authority_granted {
			"accepted_decision_contract_only_program_intake_not_mutated"
		} else if mode == "dry_run" {
			"runtime_policy_promotion_validation_only"
		} else {
			"runtime_policy_promotion_refused"
		},
		"next_action": if program_intake_state == "partial" || program_intake_state == "inconsistent" {
			"Program Intake is partial or inconsistent; recover it manually and do not apply again."
		} else if program_intake_present {
			"Program Intake already exists for this Decision Contract; read its execution state."
		} else if execution_authority_granted {
			"Run intake_goal dry_run, then apply once only after dry-run succeeds."
		} else if eligible {
			"Re-run with mode=apply to record the internal challenge and promote the Decision Contract."
		} else {
			"Create a corrected proposal that resolves every internal challenge objection."
		}
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
