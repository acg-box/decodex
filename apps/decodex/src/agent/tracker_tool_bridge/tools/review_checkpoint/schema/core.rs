use serde_json::Value;

pub(in crate::agent::tracker_tool_bridge::tools) fn review_checkpoint_reviewer_schema() -> Value {
	serde_json::json!({
		"type": "string",
		"enum": ["independent_fresh_context"]
	})
}

pub(in crate::agent::tracker_tool_bridge::tools) fn review_checkpoint_status_schema() -> Value {
	serde_json::json!({
		"type": "string",
		"enum": ["clean", "findings", "needs_architecture_review", "blocked"]
	})
}

pub(in crate::agent::tracker_tool_bridge::tools) fn review_checkpoint_contract_schema() -> Value {
	serde_json::json!({
		"type": "object",
		"properties": {
			"workflow_policy_source": {
				"type": "string",
				"enum": ["registered_project_workflow"]
			},
			"review_type": {
				"type": "string",
				"enum": ["full_current_head_review", "repair_verification"]
			},
			"risk_tier": {
				"type": "string",
				"enum": ["low", "localized", "high"]
			},
			"objective": { "type": "string" },
			"scope": non_empty_string_array_schema(),
			"non_goals": non_empty_string_array_schema(),
			"required_checks": non_empty_string_array_schema(),
			"allowed_expansion_triggers": non_empty_string_array_schema(),
			"validation_evidence": non_empty_string_array_schema()
		},
		"required": [
			"workflow_policy_source",
			"review_type",
			"risk_tier",
			"objective",
			"scope",
			"non_goals",
			"required_checks",
			"allowed_expansion_triggers",
			"validation_evidence"
		],
		"additionalProperties": false
	})
}

pub(in crate::agent::tracker_tool_bridge::tools) fn review_checkpoint_checks_schema() -> Value {
	serde_json::json!({
		"type": "object",
		"properties": {
			"intended_behavior": { "type": "string" },
			"regression_risk": { "type": "string" },
			"missing_tests": { "type": "string" },
			"docs_config_drift": { "type": "string" },
			"migration_fallout": { "type": "string" },
			"operator_facing_fallout": { "type": "string" },
			"loop_decision_contract": { "type": "string" }
		},
		"required": [
			"intended_behavior",
			"regression_risk",
			"missing_tests",
			"docs_config_drift",
			"migration_fallout",
			"operator_facing_fallout",
			"loop_decision_contract"
		],
		"additionalProperties": false
	})
}

pub(in crate::agent::tracker_tool_bridge::tools) fn non_empty_string_array_schema() -> Value {
	serde_json::json!({
		"type": "array",
		"items": { "type": "string" },
		"minItems": 1
	})
}
