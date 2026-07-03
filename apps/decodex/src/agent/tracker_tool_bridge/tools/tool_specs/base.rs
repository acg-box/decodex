use crate::agent::tracker_tool_bridge::{
	DynamicToolSpec, ISSUE_COMMENT_TOOL_NAME, ISSUE_LABEL_ADD_TOOL_NAME,
	ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME, ISSUE_TRANSITION_TOOL_NAME, TrackerToolBridge,
	tools::COMMENT_KIND_MANUAL_ATTENTION,
};

impl<'a> TrackerToolBridge<'a> {
	pub(super) fn base_tool_specs(&self) -> Vec<DynamicToolSpec> {
		let mut tool_specs = vec![self.transition_tool_spec()];

		tool_specs.extend(self.comment_tool_specs());
		tool_specs.extend(self.progress_checkpoint_tool_specs());

		tool_specs
	}

	pub(super) fn closeout_base_tool_specs(&self) -> Vec<DynamicToolSpec> {
		let mut tool_specs = vec![self.transition_tool_spec()];

		tool_specs.extend(self.comment_tool_specs());
		tool_specs.extend(self.progress_checkpoint_tool_specs());

		tool_specs
	}

	pub(super) fn comment_tool_specs(&self) -> Vec<DynamicToolSpec> {
		vec![DynamicToolSpec::new(
			ISSUE_COMMENT_TOOL_NAME,
			"Add an allowlisted public summary comment to the currently leased issue. The supported automation kind is `manual_attention`; Decodex renders the Linear comment from structured public fields and may attach a durable authority-boundary decision request.",
			serde_json::json!({
				"type": "object",
				"properties": {
					"issue_id": { "type": "string" },
					"issue_identifier": { "type": "string" },
					"kind": {
						"type": "string",
						"enum": [COMMENT_KIND_MANUAL_ATTENTION]
					},
					"error_class": { "type": "string" },
					"next_action": { "type": "string" },
					"blockers": {
						"type": "array",
						"items": { "type": "string" }
					},
					"evidence": {
						"type": "array",
						"items": { "type": "string" }
					},
					"failed_command": { "type": "string" },
					"raw_error": { "type": "string" },
					"summary": { "type": "string" },
					"decision_request": {
						"type": "object",
						"properties": {
							"boundary_check_id": { "type": "integer" },
							"decision_request_id": { "type": "string" },
							"reason_code": { "type": "string" },
							"boundary_type": { "type": "string" },
							"proposed_change": { "type": "string" },
							"why_exceeds_authority": { "type": "string" },
							"options": {
								"type": "array",
								"items": {
									"type": "object",
									"properties": {
										"label": { "type": "string" },
										"description": { "type": "string" }
									},
									"required": ["label", "description"],
									"additionalProperties": false
								}
							},
							"recommendation": { "type": "string" },
							"resume_condition": { "type": "string" },
							"retained_worktree_evidence": {
								"type": "array",
								"items": { "type": "string" }
							},
							"retained_diff_evidence": {
								"type": "array",
								"items": { "type": "string" }
							},
							"recovery_attempt_context": {
								"type": "array",
								"items": { "type": "string" }
							}
						},
						"required": [
							"boundary_check_id",
							"decision_request_id",
							"reason_code",
							"boundary_type",
							"proposed_change",
							"why_exceeds_authority",
							"options",
							"recommendation",
							"resume_condition"
						],
						"additionalProperties": false
					}
				},
				"required": ["kind", "error_class", "next_action", "blockers", "evidence"],
				"additionalProperties": false
			}),
		)]
	}

	pub(super) fn progress_checkpoint_tool_specs(&self) -> [DynamicToolSpec; 1] {
		[DynamicToolSpec::new(
			ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME,
			"Record the current execution-state snapshot for the leased issue as private runtime evidence, then publish only a low-frequency public Linear projection when the public lifecycle signal changes. On retained lanes, omit `head_sha` to capture the exact current lane HEAD automatically, or pass a matching current-lane HEAD SHA.",
			serde_json::json!({
			"type": "object",
			"properties": {
				"issue_id": { "type": "string" },
				"issue_identifier": { "type": "string" },
					"phase": {
						"type": "string",
						"enum": [
							"probing",
						"implementing",
						"verifying",
						"blocked",
						"ready_for_review",
						"review_repair",
						"ready_to_land",
							"closeout"
						]
					},
					"docs_impact": {
						"type": "string",
						"enum": [
							"none",
							"update_required",
							"research_required",
							"drift_required"
						]
					},
					"focus": { "type": "string" },
					"next_action": { "type": "string" },
				"blockers": {
					"type": "array",
					"items": { "type": "string" }
				},
				"evidence": {
					"type": "array",
					"items": { "type": "string" }
				},
				"verification": {
					"type": "array",
					"items": { "type": "string" }
				},
				"head_sha": { "type": "string" },
				"branch": { "type": "string" },
				"pr_url": { "type": "string" }
			},
				"required": ["phase", "docs_impact", "focus", "next_action", "blockers", "evidence"],
				"additionalProperties": false
			}),
		)]
	}

	pub(super) fn transition_tool_spec(&self) -> DynamicToolSpec {
		DynamicToolSpec::new(
			ISSUE_TRANSITION_TOOL_NAME,
			"Move the currently leased issue to another allowed workflow state.",
			serde_json::json!({
				"type": "object",
				"properties": {
					"issue_id": { "type": "string" },
					"issue_identifier": { "type": "string" },
					"state": { "type": "string" }
				},
				"required": ["state"],
				"additionalProperties": false
			}),
		)
	}

	pub(super) fn label_add_tool_spec(&self) -> DynamicToolSpec {
		DynamicToolSpec::new(
			ISSUE_LABEL_ADD_TOOL_NAME,
			"Add an allowed workflow label to the currently leased issue. For `needs_attention_label`, this records a manual-attention intent; Decodex applies the actual label only after the paired `manual_attention` comment validates.",
			serde_json::json!({
				"type": "object",
				"properties": {
					"issue_id": { "type": "string" },
					"issue_identifier": { "type": "string" },
					"label": { "type": "string" }
				},
				"required": ["label"],
				"additionalProperties": false
			}),
		)
	}
}
