use serde_json::Value;

use crate::agent::tracker_tool_bridge::tools::review_checkpoint::{
	REVIEW_CLASS_COMPACT_CURRENT_HEAD, REVIEW_CLASS_FULL_CURRENT_HEAD,
	schema::non_empty_string_array_schema,
};

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
