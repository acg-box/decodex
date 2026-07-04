use serde_json::Value;

use crate::agent::tracker_tool_bridge::tools::review_checkpoint::{
	REVIEW_ROUTE_ARCHITECTURE_SIGNAL, REVIEW_ROUTE_CONTRACT_OR_AUTHORITY_DECISION_REQUIRED,
	REVIEW_ROUTE_CURRENT_BLOCKER, REVIEW_ROUTE_DETERMINISTIC_GATE_CANDIDATE,
	REVIEW_ROUTE_FOLLOW_UP, REVIEW_ROUTE_INVALID_OR_UNSUBSTANTIATED,
	REVIEW_ROUTE_ISSUE_CONTRACT_GAP, REVIEW_ROUTE_LANDING_BLOCKER, REVIEW_ROUTE_NEEDS_EVIDENCE,
	REVIEW_ROUTE_REVIEWER_RUBRIC_GAP, REVIEW_ROUTE_RISK_NOTE, REVIEW_ROUTE_SOURCE_ACCEPTED,
	REVIEW_ROUTE_SOURCE_REJECTED, REVIEW_ROUTE_SOURCE_ROUTE_ONLY,
	schema::non_empty_string_array_schema,
};

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

fn review_checkpoint_severity_schema() -> Value {
	serde_json::json!({
		"type": "string",
		"enum": ["critical", "high", "medium", "low", "info"]
	})
}
