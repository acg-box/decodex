use serde_json;

use crate::agent::tracker_tool_bridge::tools::{review_checkpoint::{non_empty_string_array_schema,review_checkpoint_checks_schema,review_checkpoint_contract_schema,review_checkpoint_finding_routes_schema,review_checkpoint_findings_array_schema,review_checkpoint_reviewer_schema,review_checkpoint_status_schema,review_cost_control_schema,},COMMENT_KIND_MANUAL_ATTENTION,};
use crate::agent::tracker_tool_bridge::{
	DynamicToolSpec, ISSUE_COMMENT_TOOL_NAME, ISSUE_DELIVERY_CLOSEOUT_COMPLETE_TOOL_NAME,
	ISSUE_LABEL_ADD_TOOL_NAME, ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME,
	ISSUE_REVIEW_CHECKPOINT_TOOL_NAME, ISSUE_REVIEW_HANDOFF_TOOL_NAME,
	ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME, ISSUE_TERMINAL_FINALIZE_TOOL_NAME,
	ISSUE_TRANSITION_TOOL_NAME, ReviewExecutionMode, ReviewHandoffContext, TrackerToolBridge,
};

impl<'a> TrackerToolBridge<'a> {
	pub(in crate::agent::tracker_tool_bridge) fn build_tool_specs(&self) -> Vec<DynamicToolSpec> {
		let mut tool_specs = match self.review_context.as_ref().map(|context| context.mode) {
			Some(ReviewExecutionMode::Repair) => {
				let mut tool_specs = self.comment_tool_specs();

				tool_specs.extend(self.progress_checkpoint_tool_specs());

				if self
					.review_context
					.as_ref()
					.is_some_and(ReviewHandoffContext::decodex_review_checkpoint_enabled)
				{
					tool_specs.extend(self.review_checkpoint_tool_specs());
				}

				tool_specs
			},
			Some(ReviewExecutionMode::Closeout) => self.closeout_base_tool_specs(),
			Some(ReviewExecutionMode::Handoff) => {
				let mut tool_specs = self.base_tool_specs();

				if self
					.review_context
					.as_ref()
					.is_some_and(ReviewHandoffContext::decodex_review_checkpoint_enabled)
				{
					tool_specs.extend(self.review_checkpoint_tool_specs());
				}

				tool_specs.extend(self.review_handoff_tool_specs());

				tool_specs
			},
			None => self.base_tool_specs(),
		};

		if matches!(
			self.review_context.as_ref().map(|context| context.mode),
			Some(ReviewExecutionMode::Repair)
		) {
			tool_specs.extend(self.review_repair_tool_specs());
		}
		if matches!(
			self.review_context.as_ref().map(|context| context.mode),
			Some(ReviewExecutionMode::Closeout)
		) {
			tool_specs.extend(self.closeout_tool_specs());
		}

		tool_specs.push(self.label_add_tool_spec());

		tool_specs
	}

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

	pub(super) fn review_handoff_tool_specs(&self) -> [DynamicToolSpec; 2] {
		[
			DynamicToolSpec::new(
				ISSUE_REVIEW_HANDOFF_TOOL_NAME,
				"Record a PR-backed review handoff for the currently leased issue after the branch is pushed and a non-draft PR is ready for review.",
				serde_json::json!({
					"type": "object",
					"properties": {
						"issue_id": { "type": "string" },
						"issue_identifier": { "type": "string" },
						"pr_url": { "type": "string" },
						"summary": { "type": "string" }
					},
					"required": ["pr_url", "summary"],
					"additionalProperties": false
				}),
			),
			DynamicToolSpec::new(
				ISSUE_TERMINAL_FINALIZE_TOOL_NAME,
				"Finalize the current run's terminal tracker path after either PR-backed review handoff or the manual-attention exit has been fully recorded.",
				serde_json::json!({
					"type": "object",
					"properties": {
						"issue_id": { "type": "string" },
						"issue_identifier": { "type": "string" },
						"path": {
							"type": "string",
							"enum": ["review_handoff", "manual_attention"]
						}
					},
					"required": ["path"],
					"additionalProperties": false
				}),
			),
		]
	}

	pub(super) fn review_checkpoint_tool_specs(&self) -> [DynamicToolSpec; 1] {
		[DynamicToolSpec::new(
			ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
			"Record the independent fresh-context read-only bounded-review result for the leased issue so Decodex can decide whether the lane may continue, repair accepted findings, or stop for human intervention. `head_sha` must resolve to the current lane HEAD.",
			serde_json::json!({
				"type": "object",
				"properties": {
					"issue_id": { "type": "string" },
					"issue_identifier": { "type": "string" },
					"reviewer": review_checkpoint_reviewer_schema(),
					"status": review_checkpoint_status_schema(),
					"head_sha": { "type": "string" },
					"review_contract": review_checkpoint_contract_schema(),
					"review_cost_control": review_cost_control_schema(),
					"checks": review_checkpoint_checks_schema(),
					"evidence": non_empty_string_array_schema(),
					"accepted_findings": review_checkpoint_findings_array_schema(false),
					"rejected_findings": review_checkpoint_findings_array_schema(true),
					"finding_routes": review_checkpoint_finding_routes_schema()
				},
				"required": ["reviewer", "status", "head_sha", "review_contract", "checks", "evidence"],
				"additionalProperties": false
			}),
		)]
	}

	pub(super) fn review_repair_tool_specs(&self) -> [DynamicToolSpec; 2] {
		[
			DynamicToolSpec::new(
				ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME,
				"Record that the retained in-review lane repaired the current PR head, pushed it, and requested fresh review on the same PR lineage.",
				serde_json::json!({
					"type": "object",
					"properties": {
						"issue_id": { "type": "string" },
						"issue_identifier": { "type": "string" },
						"pr_url": { "type": "string" },
						"summary": { "type": "string" }
					},
					"required": ["pr_url", "summary"],
					"additionalProperties": false
				}),
			),
			DynamicToolSpec::new(
				ISSUE_TERMINAL_FINALIZE_TOOL_NAME,
				"Finalize the current run's terminal tracker path after either retained review repair or the manual-attention exit has been fully recorded.",
				serde_json::json!({
					"type": "object",
					"properties": {
						"issue_id": { "type": "string" },
						"issue_identifier": { "type": "string" },
						"path": {
							"type": "string",
							"enum": ["review_repair", "manual_attention"]
						}
					},
					"required": ["path"],
					"additionalProperties": false
				}),
			),
		]
	}

	pub(super) fn closeout_tool_specs(&self) -> [DynamicToolSpec; 2] {
		[
			DynamicToolSpec::new(
				ISSUE_DELIVERY_CLOSEOUT_COMPLETE_TOOL_NAME,
				"Record that the retained post-review lane finished merge plus closeout for the same owned PR lineage.",
				serde_json::json!({
					"type": "object",
					"properties": {
						"issue_id": { "type": "string" },
						"issue_identifier": { "type": "string" },
						"pr_url": { "type": "string" },
						"summary": { "type": "string" }
					},
					"required": ["pr_url", "summary"],
					"additionalProperties": false
				}),
			),
			DynamicToolSpec::new(
				ISSUE_TERMINAL_FINALIZE_TOOL_NAME,
				"Finalize the current run's terminal tracker path after either post-review closeout or the manual-attention exit has been fully recorded.",
				serde_json::json!({
					"type": "object",
					"properties": {
						"issue_id": { "type": "string" },
						"issue_identifier": { "type": "string" },
						"path": {
							"type": "string",
							"enum": ["closeout", "manual_attention"]
						}
					},
					"required": ["path"],
					"additionalProperties": false
				}),
			),
		]
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
