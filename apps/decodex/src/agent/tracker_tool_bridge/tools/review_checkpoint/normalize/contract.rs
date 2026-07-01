use serde_json;
use sha2::{Digest as _, Sha256};

use crate::agent::tracker_tool_bridge::{
	self, ISSUE_REVIEW_CHECKPOINT_TOOL_NAME, NormalizedReviewCheckpointContract,
	ReviewCheckpointContractArgs, ReviewPolicyPhase,
};

pub(super) fn normalize_review_checkpoint_contract(
	contract: ReviewCheckpointContractArgs,
	review_policy_phase: ReviewPolicyPhase,
) -> Result<NormalizedReviewCheckpointContract, String> {
	let workflow_policy_source = super::normalize_required_review_text(
		contract.workflow_policy_source,
		"review_contract.workflow_policy_source",
	)?;

	if workflow_policy_source != "registered_project_workflow" {
		return Err(format!(
			"`{ISSUE_REVIEW_CHECKPOINT_TOOL_NAME}` requires `review_contract.workflow_policy_source` to be `registered_project_workflow`, not `{workflow_policy_source}`."
		));
	}

	let review_type = normalize_review_type(contract.review_type, review_policy_phase)?;
	let risk_tier = normalize_review_risk_tier(contract.risk_tier)?;
	let objective =
		super::normalize_required_review_text(contract.objective, "review_contract.objective")?;
	let scope = normalize_required_review_contract_list(contract.scope, "review_contract.scope")?;
	let non_goals =
		normalize_required_review_contract_list(contract.non_goals, "review_contract.non_goals")?;
	let required_checks = normalize_required_review_contract_list(
		contract.required_checks,
		"review_contract.required_checks",
	)?;
	let allowed_expansion_triggers = normalize_required_review_contract_list(
		contract.allowed_expansion_triggers,
		"review_contract.allowed_expansion_triggers",
	)?;
	let validation_evidence = normalize_required_review_contract_list(
		contract.validation_evidence,
		"review_contract.validation_evidence",
	)?;

	Ok(NormalizedReviewCheckpointContract {
		workflow_policy_source,
		review_type,
		risk_tier,
		objective,
		scope,
		non_goals,
		required_checks,
		allowed_expansion_triggers,
		validation_evidence,
	})
}

pub(super) fn normalize_review_risk_tier(risk_tier: String) -> Result<String, String> {
	let risk_tier = risk_tier.trim().to_ascii_lowercase().replace([' ', '-'], "_");

	match risk_tier.as_str() {
		"low" | "localized" | "high" => Ok(risk_tier),
		other => Err(format!(
			"`{ISSUE_REVIEW_CHECKPOINT_TOOL_NAME}` requires `review_contract.risk_tier` to be `low`, `localized`, or `high`, not `{other}`."
		)),
	}
}

pub(super) fn review_checkpoint_contract_hash(
	contract: &NormalizedReviewCheckpointContract,
) -> Result<String, String> {
	let serialized = serde_json::to_vec(contract).map_err(|error| {
		format!(
			"Failed to serialize `review_contract` for `{ISSUE_REVIEW_CHECKPOINT_TOOL_NAME}`: {error}"
		)
	})?;
	let digest = Sha256::digest(serialized);
	let hash = digest.iter().map(|byte| format!("{byte:02x}")).collect::<String>();

	Ok(format!("review_contract:{hash}"))
}

fn normalize_review_type(
	review_type: String,
	review_policy_phase: ReviewPolicyPhase,
) -> Result<String, String> {
	let review_type = review_type.trim().to_ascii_lowercase().replace([' ', '-'], "_");
	let expected = match review_policy_phase {
		ReviewPolicyPhase::Handoff => "full_current_head_review",
		ReviewPolicyPhase::Repair => "repair_verification",
	};

	if review_type != expected {
		return Err(format!(
			"`{ISSUE_REVIEW_CHECKPOINT_TOOL_NAME}` requires `review_contract.review_type` to be `{expected}` for `{}` review checkpoints, not `{review_type}`.",
			review_policy_phase.as_str()
		));
	}

	Ok(review_type)
}

fn normalize_required_review_contract_list(
	values: Vec<String>,
	field_name: &str,
) -> Result<Vec<String>, String> {
	let values = tracker_tool_bridge::normalize_progress_list(values);

	if values.is_empty() {
		return Err(format!("`{ISSUE_REVIEW_CHECKPOINT_TOOL_NAME}` requires `{field_name}`."));
	}

	Ok(values)
}
