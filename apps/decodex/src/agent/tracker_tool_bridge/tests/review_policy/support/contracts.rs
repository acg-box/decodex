use serde_json::Value;

pub(in crate::agent::tracker_tool_bridge::tests) fn review_checks_json() -> Value {
	serde_json::json!({
		"intended_behavior": "Checked the implementation against the issue requirements.",
		"regression_risk": "Checked shared runtime regression risk for the touched path.",
		"missing_tests": "Checked whether the current change needs additional tests.",
		"docs_config_drift": "Checked docs and config drift for the runtime behavior change.",
		"migration_fallout": "Checked additive runtime-store migration fallout.",
		"operator_facing_fallout": "Checked Linear and operator-facing fallout.",
		"loop_decision_contract": "Compared the change against the accepted Loop/Decision Contract and found no mismatch."
	})
}

pub(in crate::agent::tracker_tool_bridge::tests) fn handoff_review_contract_json() -> Value {
	review_contract_json("full_current_head_review")
}

pub(in crate::agent::tracker_tool_bridge::tests) fn low_risk_handoff_review_contract_json() -> Value
{
	review_contract_with_risk_json("full_current_head_review", "low")
}

pub(in crate::agent::tracker_tool_bridge::tests) fn repair_review_contract_json() -> Value {
	review_contract_json("repair_verification")
}

pub(in crate::agent::tracker_tool_bridge::tests) fn review_contract_json(
	review_type: &str,
) -> Value {
	review_contract_with_risk_json(review_type, "localized")
}

pub(in crate::agent::tracker_tool_bridge::tests) fn review_contract_with_risk_json(
	review_type: &str,
	risk_tier: &str,
) -> Value {
	serde_json::json!({
		"workflow_policy_source": "registered_project_workflow",
		"review_type": review_type,
		"risk_tier": risk_tier,
		"objective": "Review the current committed lane head against the accepted issue contract.",
		"scope": ["Current committed lane diff and directly owned behavior."],
		"non_goals": ["Do not widen into unrelated cleanup or unowned product direction."],
		"required_checks": ["Intended behavior, regression risk, tests, docs/config drift, migration fallout, operator-facing fallout, and Loop/Decision Contract alignment."],
		"allowed_expansion_triggers": ["Safety, authority-boundary, data-loss, security, live-mutation, public-API, migration, or operator-facing regression."],
		"validation_evidence": ["Repo-native validation was rerun for the committed lane head before review."]
	})
}

pub(in crate::agent::tracker_tool_bridge::tests) fn compact_review_cost_control_json() -> Value {
	serde_json::json!({
		"review_class": "compact_current_head_review",
		"risk_class": "low",
		"changed_surface_count": 2,
		"changed_surface_summary": [
			"Review checkpoint prompt guidance changed.",
			"Review checkpoint readback metadata changed."
		],
		"high_risk_surfaces": [],
		"current_head_evidence": true,
		"validation_backed": true,
		"validation_current": true,
		"evidence_sufficient": true,
		"reviewer_judgment": "The reviewer independently checked intended behavior and adversarial risk and found a low-risk small current-head lane."
	})
}

pub(in crate::agent::tracker_tool_bridge::tests) fn full_review_cost_control_json(
	fallback_reason: &str,
) -> Value {
	serde_json::json!({
		"review_class": "full_current_head_review",
		"risk_class": "localized",
		"changed_surface_count": 6,
		"changed_surface_summary": [
			"Runtime review checkpoint behavior changed.",
			"Operator readback behavior changed."
		],
		"high_risk_surfaces": ["operator-facing runtime review behavior"],
		"current_head_evidence": true,
		"validation_backed": true,
		"validation_current": true,
		"evidence_sufficient": true,
		"reviewer_judgment": "The reviewer used full independent review because compact-review guardrails did not all pass.",
		"fallback_reason": fallback_reason
	})
}
