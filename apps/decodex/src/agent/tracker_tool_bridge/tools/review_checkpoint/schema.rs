use serde_json::{self, Map, Value};

use super::{
	REVIEW_CLASS_COMPACT_CURRENT_HEAD, REVIEW_CLASS_FULL_CURRENT_HEAD,
	REVIEW_ROUTE_ARCHITECTURE_SIGNAL, REVIEW_ROUTE_CONTRACT_OR_AUTHORITY_DECISION_REQUIRED,
	REVIEW_ROUTE_CURRENT_BLOCKER, REVIEW_ROUTE_DETERMINISTIC_GATE_CANDIDATE,
	REVIEW_ROUTE_FOLLOW_UP, REVIEW_ROUTE_INVALID_OR_UNSUBSTANTIATED,
	REVIEW_ROUTE_ISSUE_CONTRACT_GAP, REVIEW_ROUTE_LANDING_BLOCKER, REVIEW_ROUTE_NEEDS_EVIDENCE,
	REVIEW_ROUTE_REVIEWER_RUBRIC_GAP, REVIEW_ROUTE_RISK_NOTE, REVIEW_ROUTE_SOURCE_ACCEPTED,
	REVIEW_ROUTE_SOURCE_REJECTED, REVIEW_ROUTE_SOURCE_ROUTE_ONLY,
};

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

pub(in crate::agent::tracker_tool_bridge::tools) fn review_cost_control_schema() -> Value {
	serde_json::json!({
		"type": "object",
		"properties": {
			"review_class": {
				"type": "string",
				"enum": [REVIEW_CLASS_COMPACT_CURRENT_HEAD, REVIEW_CLASS_FULL_CURRENT_HEAD]
			},
			"risk_class": {
				"type": "string",
				"enum": ["low", "localized", "high"]
			},
			"changed_surface_count": {
				"type": "integer",
				"minimum": 0
			},
			"changed_surface_summary": non_empty_string_array_schema(),
			"high_risk_surfaces": {
				"type": "array",
				"items": { "type": "string" }
			},
			"current_head_evidence": { "type": "boolean" },
			"validation_backed": { "type": "boolean" },
			"validation_current": { "type": "boolean" },
			"evidence_sufficient": { "type": "boolean" },
			"reviewer_judgment": { "type": "string" },
			"fallback_reason": { "type": "string" }
		},
		"required": [
			"review_class",
			"risk_class",
			"changed_surface_count",
			"changed_surface_summary",
			"current_head_evidence",
			"validation_backed",
			"reviewer_judgment"
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

pub(in crate::agent::tracker_tool_bridge::tools) fn review_checkpoint_findings_array_schema(
	rejected: bool,
) -> Value {
	serde_json::json!({
		"type": "array",
		"items": review_checkpoint_finding_schema(rejected)
	})
}

pub(in crate::agent::tracker_tool_bridge::tools) fn review_checkpoint_finding_routes_schema()
-> Value {
	serde_json::json!({
		"type": "array",
		"items": {
			"type": "object",
			"properties": {
				"route": review_checkpoint_finding_route_schema(),
				"severity": review_checkpoint_severity_schema(),
				"risk_tier": {
					"type": "string",
					"enum": ["low", "medium", "high"]
				},
				"summary": { "type": "string" },
				"evidence": non_empty_string_array_schema(),
				"resolver": { "type": "string" },
				"next_action": { "type": "string" },
				"finding_source": {
					"type": "string",
					"enum": [
						REVIEW_ROUTE_SOURCE_ACCEPTED,
						REVIEW_ROUTE_SOURCE_REJECTED,
						REVIEW_ROUTE_SOURCE_ROUTE_ONLY
					]
				},
				"finding_index": { "type": "integer", "minimum": 0 }
			},
			"required": ["route", "severity", "summary", "evidence", "resolver", "next_action"],
			"additionalProperties": false
		}
	})
}

fn review_checkpoint_finding_route_schema() -> Value {
	serde_json::json!({
		"type": "string",
		"enum": [
			REVIEW_ROUTE_CURRENT_BLOCKER,
			REVIEW_ROUTE_LANDING_BLOCKER,
			REVIEW_ROUTE_CONTRACT_OR_AUTHORITY_DECISION_REQUIRED,
			REVIEW_ROUTE_NEEDS_EVIDENCE,
			REVIEW_ROUTE_FOLLOW_UP,
			REVIEW_ROUTE_DETERMINISTIC_GATE_CANDIDATE,
			REVIEW_ROUTE_ARCHITECTURE_SIGNAL,
			REVIEW_ROUTE_ISSUE_CONTRACT_GAP,
			REVIEW_ROUTE_REVIEWER_RUBRIC_GAP,
			REVIEW_ROUTE_RISK_NOTE,
			REVIEW_ROUTE_INVALID_OR_UNSUBSTANTIATED
		]
	})
}

fn review_checkpoint_finding_schema(rejected: bool) -> Value {
	let mut properties = Map::from_iter([
		(String::from("severity"), review_checkpoint_severity_schema()),
		(String::from("summary"), serde_json::json!({ "type": "string" })),
		(String::from("evidence"), non_empty_string_array_schema()),
		(String::from("kind"), serde_json::json!({ "type": "string" })),
		(String::from("file"), serde_json::json!({ "type": "string" })),
		(String::from("line"), serde_json::json!({ "type": "integer", "minimum": 1 })),
		(String::from("line_range"), review_checkpoint_line_range_schema()),
	]);
	let required = if rejected {
		properties
			.insert(String::from("rejection_reason"), serde_json::json!({ "type": "string" }));

		serde_json::json!(["severity", "summary", "rejection_reason", "evidence"])
	} else {
		properties.insert(String::from("guidance"), serde_json::json!({ "type": "string" }));

		serde_json::json!(["severity", "summary", "evidence", "guidance"])
	};

	serde_json::json!({
		"type": "object",
		"properties": properties,
		"required": required,
		"additionalProperties": false
	})
}

fn review_checkpoint_severity_schema() -> Value {
	serde_json::json!({
		"type": "string",
		"enum": ["critical", "high", "medium", "low", "info"]
	})
}

fn review_checkpoint_line_range_schema() -> Value {
	serde_json::json!({
		"type": "object",
		"properties": {
			"start": { "type": "integer", "minimum": 1 },
			"end": { "type": "integer", "minimum": 1 }
		},
		"required": ["start", "end"],
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
