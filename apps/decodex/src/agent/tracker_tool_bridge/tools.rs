use std::collections::{BTreeMap, BTreeSet};

use serde_json::{self, Map, Value};
use sha2::{Digest as _, Sha256};

use crate::{
	agent::tracker_tool_bridge::{
		self, AuthorityDecisionOptionArgs, AuthorityDecisionRequestArgs, CommentArgs, DocsImpact,
		DynamicToolCallResponse, DynamicToolSpec, ExecutionProgressPhase, ISSUE_COMMENT_TOOL_NAME,
		ISSUE_DELIVERY_CLOSEOUT_COMPLETE_TOOL_NAME, ISSUE_LABEL_ADD_TOOL_NAME,
		ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME, ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
		ISSUE_REVIEW_HANDOFF_TOOL_NAME, ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME,
		ISSUE_TERMINAL_FINALIZE_TOOL_NAME, ISSUE_TRANSITION_TOOL_NAME, LabelArgs, LocalRepoDetails,
		NormalizedProgressCheckpoint, NormalizedRejectedReviewCheckpointFinding,
		NormalizedReviewCheckpointContract, NormalizedReviewCheckpointFinding,
		NormalizedReviewCheckpointFindingRoute, NormalizedReviewCheckpointPayload,
		NormalizedReviewCostControl, PendingReviewAction, PendingReviewCompletion,
		ProgressCheckpointArgs, PullRequestDetails, REVIEW_POLICY_CONVERGENCE_BUDGET,
		ReviewCheckpointArgs, ReviewCheckpointChecksArgs, ReviewCheckpointContractArgs,
		ReviewCheckpointFindingArgs, ReviewCheckpointFindingRouteArgs,
		ReviewCheckpointFindingRouteCount, ReviewCheckpointFindingRouteSummary,
		ReviewCheckpointHeadBinding, ReviewCheckpointLineRangeArgs,
		ReviewCheckpointRejectedFindingArgs, ReviewCostControlArgs, ReviewExecutionMode,
		ReviewFindingPolicyRecord, ReviewFindingPolicyState, ReviewHandoffArgs,
		ReviewHandoffContext, ReviewPolicyPhase, ReviewPolicyState, ReviewPolicyStatus,
		RunCompletionDisposition, TerminalFinalizeArgs, TrackerToolBridge, TransitionArgs,
	},
	orchestrator::{
		self, AUTHORITY_BOUNDARY_CHECK_EVENT_TYPE, AuthorityDecisionOption,
		AuthorityDecisionRequestInput,
	},
	state::StateStore,
	tracker::{
		self, public_text, records,
		records::{LinearExecutionEventIdentity, LinearExecutionEventRecord},
	},
};

const COMMENT_KIND_MANUAL_ATTENTION: &str = "manual_attention";
const MANUAL_ATTENTION_TERMINAL_PATH: &str = "manual_attention";
const INDEPENDENT_FRESH_CONTEXT_REVIEWER: &str = "independent_fresh_context";
const REVIEW_COMPLETION_INTENT_EVENT_TYPE: &str = "review_completion_intent";
const TERMINAL_FINALIZE_EVENT_TYPE: &str = "terminal_finalize";
const REVIEW_CLASS_COMPACT_CURRENT_HEAD: &str = "compact_current_head_review";
const REVIEW_CLASS_FULL_CURRENT_HEAD: &str = "full_current_head_review";
const REVIEW_COST_CONTROL_NOT_PROVIDED: &str = "review_cost_control_not_provided";
const MAX_COMPACT_REVIEW_CHANGED_SURFACE_COUNT: u64 = 5;
const REVIEW_ROUTE_CURRENT_BLOCKER: &str = "current_blocker";
const REVIEW_ROUTE_LANDING_BLOCKER: &str = "landing_blocker";
const REVIEW_ROUTE_CONTRACT_OR_AUTHORITY_DECISION_REQUIRED: &str =
	"contract_or_authority_decision_required";
const REVIEW_ROUTE_NEEDS_EVIDENCE: &str = "needs_evidence";
const REVIEW_ROUTE_FOLLOW_UP: &str = "follow_up";
const REVIEW_ROUTE_DETERMINISTIC_GATE_CANDIDATE: &str = "deterministic_gate_candidate";
const REVIEW_ROUTE_ARCHITECTURE_SIGNAL: &str = "architecture_signal";
const REVIEW_ROUTE_ISSUE_CONTRACT_GAP: &str = "issue_contract_gap";
const REVIEW_ROUTE_REVIEWER_RUBRIC_GAP: &str = "reviewer_rubric_gap";
const REVIEW_ROUTE_RISK_NOTE: &str = "risk_note";
const REVIEW_ROUTE_INVALID_OR_UNSUBSTANTIATED: &str = "invalid_or_unsubstantiated";
const REVIEW_ROUTE_SOURCE_ACCEPTED: &str = "accepted_findings";
const REVIEW_ROUTE_SOURCE_REJECTED: &str = "rejected_findings";
const REVIEW_ROUTE_SOURCE_ROUTE_ONLY: &str = "route_only";
const REVIEW_ROUTE_RISK_HIGH: &str = "high";

#[derive(Debug)]
struct NormalizedManualAttentionComment {
	error_class: String,
	next_action: String,
	blockers: Vec<String>,
	evidence: Vec<String>,
	failed_command: Option<String>,
	raw_error: Option<String>,
	summary: Option<String>,
	decision_request: Option<NormalizedAuthorityDecisionRequest>,
}

#[derive(Debug)]
struct NormalizedAuthorityDecisionRequest {
	boundary_check_id: i64,
	decision_request_id: String,
	reason_code: String,
	boundary_type: String,
	proposed_change: String,
	why_exceeds_authority: String,
	options: Vec<NormalizedAuthorityDecisionOption>,
	recommendation: String,
	resume_condition: String,
	retained_worktree_evidence: Vec<String>,
	retained_diff_evidence: Vec<String>,
	recovery_attempt_context: Vec<String>,
}

#[derive(Debug)]
struct NormalizedAuthorityDecisionOption {
	label: String,
	description: String,
}

struct ReviewCheckpointPayloadCounts {
	evidence: usize,
	accepted_findings: usize,
	rejected_findings: usize,
	finding_routes: usize,
	current_blockers: usize,
}

struct ReviewFindingPolicyUpdate {
	nonclean_rounds: i64,
	previous_nonclean_rounds: i64,
	finding_policy: ReviewFindingPolicyState,
}

struct PreparedReviewCheckpoint {
	review_policy_phase: ReviewPolicyPhase,
	review_policy_status: ReviewPolicyStatus,
	head_sha: String,
	checkpoint_payload: NormalizedReviewCheckpointPayload,
	nonclean_rounds: i64,
}

impl<'a> TrackerToolBridge<'a> {
	pub(super) fn build_tool_specs(&self) -> Vec<DynamicToolSpec> {
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

	pub(super) fn handle_call_inner(
		&self,
		tool_name: &str,
		arguments: Value,
	) -> DynamicToolCallResponse {
		if let Some(response) = self.review_policy_mutation_fence(tool_name) {
			return response;
		}

		match tool_name {
			ISSUE_TRANSITION_TOOL_NAME => self.handle_transition(arguments),
			ISSUE_COMMENT_TOOL_NAME => self.handle_comment(arguments),
			ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME => self.handle_progress_checkpoint(arguments),
			ISSUE_REVIEW_CHECKPOINT_TOOL_NAME => self.handle_review_checkpoint(arguments),
			ISSUE_REVIEW_HANDOFF_TOOL_NAME => self.handle_review_handoff(arguments),
			ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME => self.handle_review_repair_complete(arguments),
			ISSUE_DELIVERY_CLOSEOUT_COMPLETE_TOOL_NAME => self.handle_closeout_complete(arguments),
			ISSUE_LABEL_ADD_TOOL_NAME => self.handle_add_label(arguments),
			ISSUE_TERMINAL_FINALIZE_TOOL_NAME => self.handle_terminal_finalize(arguments),
			_ =>
				DynamicToolCallResponse::failure(format!("Unsupported tracker tool `{tool_name}`.")),
		}
	}

	fn review_policy_mutation_fence(&self, tool_name: &str) -> Option<DynamicToolCallResponse> {
		if matches!(
			tool_name,
			ISSUE_REVIEW_CHECKPOINT_TOOL_NAME | ISSUE_TERMINAL_FINALIZE_TOOL_NAME
		) {
			return None;
		}

		let review_context = self.review_context.as_ref()?;

		match self.review_policy_stop_requested(review_context) {
			Ok(Some(stop)) => Some(DynamicToolCallResponse::failure(format!(
				"Review policy stop `{}` is active for issue `{}` after `{}` non-clean rounds; `{tool_name}` is fenced until architecture recovery or human attention resolves the lane.",
				stop.reason.error_class(),
				stop.issue_identifier,
				stop.nonclean_rounds.unwrap_or_default()
			))),
			Ok(None) => None,
			Err(error) => Some(DynamicToolCallResponse::failure(format!(
				"Failed to evaluate review policy mutation fence for `{tool_name}`: {error}"
			))),
		}
	}

	pub(super) fn handle_progress_checkpoint(&self, arguments: Value) -> DynamicToolCallResponse {
		let parsed = match serde_json::from_value::<ProgressCheckpointArgs>(arguments) {
			Ok(parsed) => parsed,
			Err(error) => {
				return DynamicToolCallResponse::failure(format!(
					"Invalid `issue.progress_checkpoint` arguments: {error}"
				));
			},
		};

		if let Err(error) = self.ensure_issue_scope(&parsed.scope) {
			return DynamicToolCallResponse::failure(error);
		}

		let checkpoint = match self.normalize_progress_checkpoint(parsed) {
			Ok(checkpoint) => checkpoint,
			Err(error) => return DynamicToolCallResponse::failure(error),
		};
		let (review_context, state_store) = match self.progress_checkpoint_context() {
			Ok(context) => context,
			Err(error) => return DynamicToolCallResponse::failure(error),
		};

		if let Err(error) =
			self.append_private_progress_checkpoint(review_context, state_store, &checkpoint)
		{
			return DynamicToolCallResponse::failure(error);
		}

		let public_projection =
			self.render_progress_checkpoint_projection(review_context, &checkpoint);

		match self.publish_progress_checkpoint_projection(state_store, &public_projection) {
			Ok(true) => DynamicToolCallResponse::success(format!(
				"Recorded private `{}` execution state for issue `{}` and published the public Linear projection.",
				checkpoint.phase.as_str(),
				self.issue.identifier
			)),
			Ok(false) => DynamicToolCallResponse::success(format!(
				"Recorded private `{}` execution state for issue `{}`; public Linear projection is unchanged.",
				checkpoint.phase.as_str(),
				self.issue.identifier
			)),
			Err(error) => DynamicToolCallResponse::failure(error),
		}
	}

	fn normalize_progress_checkpoint(
		&self,
		parsed: ProgressCheckpointArgs,
	) -> Result<NormalizedProgressCheckpoint, String> {
		let phase = ExecutionProgressPhase::parse(&parsed.phase)?;
		let docs_impact = DocsImpact::parse(&parsed.docs_impact)?;
		let focus = tracker_tool_bridge::normalize_summary(&parsed.focus);
		let next_action = tracker_tool_bridge::normalize_summary(&parsed.next_action);
		let blockers = tracker_tool_bridge::normalize_progress_list(parsed.blockers);
		let evidence = tracker_tool_bridge::normalize_progress_list(parsed.evidence);
		let verification = tracker_tool_bridge::normalize_progress_list(parsed.verification);
		let head_sha = self.resolve_progress_checkpoint_head_sha(parsed.head_sha)?;
		let branch = tracker_tool_bridge::normalize_optional_progress_field(parsed.branch);
		let pr_url = tracker_tool_bridge::normalize_optional_progress_field(parsed.pr_url);

		if focus.is_empty() {
			return Err(String::from("`issue_progress_checkpoint` requires a non-empty `focus`."));
		}
		if next_action.is_empty() {
			return Err(String::from(
				"`issue_progress_checkpoint` requires a non-empty `next_action`.",
			));
		}
		if phase == ExecutionProgressPhase::Blocked && blockers.is_empty() {
			return Err(String::from(
				"`issue_progress_checkpoint` phase `blocked` requires at least one blocker.",
			));
		}

		Ok(NormalizedProgressCheckpoint {
			phase,
			docs_impact,
			focus,
			next_action,
			blockers,
			evidence,
			verification,
			head_sha,
			branch,
			pr_url,
		})
	}

	fn progress_checkpoint_context(&self) -> Result<(&ReviewHandoffContext, &StateStore), String> {
		let review_context = self.review_context.as_ref().ok_or_else(|| {
			String::from("`issue_progress_checkpoint` requires an active Decodex run context.")
		})?;
		let state_store = self.state_store.ok_or_else(|| {
			format!(
				"`issue_progress_checkpoint` requires the Decodex runtime state store for issue `{}`.",
				self.issue.identifier
			)
		})?;

		Ok((review_context, state_store))
	}

	fn append_private_progress_checkpoint(
		&self,
		review_context: &ReviewHandoffContext,
		state_store: &StateStore,
		checkpoint: &NormalizedProgressCheckpoint,
	) -> Result<(), String> {
		let branch = checkpoint.public_branch(review_context);
		let private_payload = serde_json::json!({
				"phase": checkpoint.phase.as_str(),
				"docs_impact": checkpoint.docs_impact.as_str(),
				"focus": checkpoint.focus.as_str(),
			"next_action": checkpoint.next_action.as_str(),
			"blockers": &checkpoint.blockers,
			"evidence": &checkpoint.evidence,
			"verification": &checkpoint.verification,
			"head_sha": checkpoint.head_sha.as_deref(),
			"branch": branch.as_str(),
			"worktree_path": review_context.worktree_path.as_str(),
			"pr_url": checkpoint.pr_url.as_deref(),
		});

		state_store
			.append_private_execution_event(
				&review_context.service_id,
				&self.issue.id,
				&review_context.run_id,
				review_context.attempt_number,
				"progress_checkpoint",
				private_payload,
			)
			.map(|_| ())
			.map_err(|error| {
				format!(
					"Failed to persist the private execution-state checkpoint for issue `{}`: {error}",
					self.issue.identifier
				)
			})
	}

	fn render_progress_checkpoint_projection(
		&self,
		review_context: &ReviewHandoffContext,
		checkpoint: &NormalizedProgressCheckpoint,
	) -> LinearExecutionEventRecord {
		let branch = checkpoint.public_branch(review_context);

		records::render_progress_checkpoint_public_projection(
			LinearExecutionEventIdentity {
				service_id: &review_context.service_id,
				issue_id: &self.issue.id,
				issue_identifier: &self.issue.identifier,
				run_id: &review_context.run_id,
				attempt_number: review_context.attempt_number,
			},
			tracker_tool_bridge::current_timestamp(),
			checkpoint.phase.as_str(),
			Some(branch.as_str()),
			Some(review_context.worktree_path.as_str()),
			checkpoint.pr_url.as_deref(),
		)
	}

	fn publish_progress_checkpoint_projection(
		&self,
		state_store: &StateStore,
		public_projection: &LinearExecutionEventRecord,
	) -> Result<bool, String> {
		let projection = tracker::prepare_linear_execution_event_comment(
			"",
			public_projection,
			self.public_projection_privacy_classifier,
		)
		.map_err(|error| {
			format!(
				"Failed to prepare the public progress projection for issue `{}`: {error}",
				self.issue.identifier
			)
		})?;

		if self.progress_checkpoint_projection_cached(state_store, &projection.record)? {
			return Ok(false);
		}

		let comment_created = match tracker::create_prepared_linear_execution_event_comment(
			self.tracker,
			&self.issue.id,
			&projection,
		) {
			Ok(comment_created) => comment_created,
			Err(error) => {
				return Err(format!(
					"Failed to record an execution-state checkpoint for issue `{}`: {error}",
					self.issue.identifier
				));
			},
		};

		state_store.record_linear_execution_event(&projection.record).map_err(|error| {
			format!(
				"Failed to persist the public progress projection cache for issue `{}`: {error}",
				self.issue.identifier
			)
		})?;

		Ok(comment_created)
	}

	fn progress_checkpoint_projection_cached(
		&self,
		state_store: &StateStore,
		public_projection: &LinearExecutionEventRecord,
	) -> Result<bool, String> {
		let records = state_store
			.list_linear_execution_events(
				&public_projection.service_id,
				&public_projection.issue_id,
			)
			.map_err(|error| {
				format!(
					"Failed to read the public progress projection cache for issue `{}`: {error}",
					self.issue.identifier
				)
			})?;

		Ok(records.iter().any(|record| record.idempotency_key == public_projection.idempotency_key))
	}

	pub(super) fn handle_transition(&self, arguments: Value) -> DynamicToolCallResponse {
		let parsed = match serde_json::from_value::<TransitionArgs>(arguments) {
			Ok(parsed) => parsed,
			Err(error) => {
				return DynamicToolCallResponse::failure(format!(
					"Invalid `issue.transition` arguments: {error}"
				));
			},
		};

		if let Err(error) = self.ensure_issue_scope(&parsed.scope) {
			return DynamicToolCallResponse::failure(error);
		}

		let allowed_states = self.allowed_transition_states();

		if !allowed_states.iter().any(|state| state == &parsed.state) {
			let success_state = self.workflow.frontmatter().tracker().success_state();

			if parsed.state == success_state {
				return DynamicToolCallResponse::failure(format!(
					"State `{}` requires `{}` after the branch is pushed and a reviewable PR exists.",
					parsed.state, ISSUE_REVIEW_HANDOFF_TOOL_NAME
				));
			}

			return DynamicToolCallResponse::failure(format!(
				"State `{}` is outside the allowed tracker tool policy.",
				parsed.state
			));
		}

		let Some(state_id) = self.issue.state_id_for_name(&parsed.state) else {
			return DynamicToolCallResponse::failure(format!(
				"State `{}` does not exist on issue `{}`.",
				parsed.state, self.issue.identifier
			));
		};

		match self.tracker.update_issue_state(&self.issue.id, state_id) {
			Ok(()) => {
				self.local_issue_state_name.replace(parsed.state.clone());
				self.record_continuation_blocking_transition(&parsed.state);

				DynamicToolCallResponse::success(format!(
					"Issue `{}` moved to `{}`.",
					self.issue.identifier, parsed.state
				))
			},
			Err(error) => DynamicToolCallResponse::failure(format!(
				"Failed to move issue `{}` to `{}`: {error}",
				self.issue.identifier, parsed.state
			)),
		}
	}

	pub(super) fn handle_comment(&self, arguments: Value) -> DynamicToolCallResponse {
		let parsed = match serde_json::from_value::<CommentArgs>(arguments) {
			Ok(parsed) => parsed,
			Err(error) => {
				return DynamicToolCallResponse::failure(format!(
					"Invalid `issue.comment` arguments: {error}"
				));
			},
		};

		if let Err(error) = self.ensure_issue_scope(&parsed.scope) {
			return DynamicToolCallResponse::failure(error);
		}

		match parsed.kind.as_str() {
			COMMENT_KIND_MANUAL_ATTENTION => self.handle_manual_attention_comment(parsed),
			other => DynamicToolCallResponse::failure(format!(
				"Unsupported `{ISSUE_COMMENT_TOOL_NAME}` kind `{other}`. Supported kinds: `{COMMENT_KIND_MANUAL_ATTENTION}`."
			)),
		}
	}

	fn handle_manual_attention_comment(&self, parsed: CommentArgs) -> DynamicToolCallResponse {
		if !*self.manual_attention_requested.borrow() {
			return DynamicToolCallResponse::failure(format!(
				"`{ISSUE_COMMENT_TOOL_NAME}` kind `{COMMENT_KIND_MANUAL_ATTENTION}` requires a successful `{ISSUE_LABEL_ADD_TOOL_NAME}` call for label `{}` before writing the explanatory comment.",
				self.workflow.frontmatter().tracker().needs_attention_label()
			));
		}

		let review_context = match self.review_context.as_ref() {
			Some(review_context) => review_context,
			None => {
				return DynamicToolCallResponse::failure(format!(
					"`{ISSUE_COMMENT_TOOL_NAME}` kind `{COMMENT_KIND_MANUAL_ATTENTION}` requires an active Decodex run context."
				));
			},
		};
		let state_store = match self.state_store {
			Some(state_store) => state_store,
			None => {
				return DynamicToolCallResponse::failure(format!(
					"`{ISSUE_COMMENT_TOOL_NAME}` kind `{COMMENT_KIND_MANUAL_ATTENTION}` requires the Decodex runtime state store for issue `{}`.",
					self.issue.identifier
				));
			},
		};
		let comment = match Self::normalize_manual_attention_comment(parsed) {
			Ok(comment) => comment,
			Err(error) => return DynamicToolCallResponse::failure(error),
		};

		if let Some(decision_request) = comment.decision_request.as_ref()
			&& let Err(error) = self.append_private_authority_decision_request(
				review_context,
				state_store,
				decision_request,
			) {
			return DynamicToolCallResponse::failure(error);
		}

		let record = self.manual_attention_execution_event(review_context, &comment);
		let body = format_manual_attention_comment(review_context, &comment);
		let projection = match tracker::prepare_linear_execution_event_comment(
			&body,
			&record,
			self.public_projection_privacy_classifier,
		) {
			Ok(projection) => projection,
			Err(error) => return DynamicToolCallResponse::failure(error.to_string()),
		};

		if let Err(error) = self.apply_manual_attention_label() {
			return DynamicToolCallResponse::failure(error);
		}

		match tracker::create_prepared_linear_execution_event_comment(
			self.tracker,
			&self.issue.id,
			&projection,
		) {
			Ok(created) => {
				if let Err(error) = state_store.record_linear_execution_event(&projection.record) {
					return DynamicToolCallResponse::failure(format!(
						"Failed to persist the public manual-attention summary for issue `{}`: {error}",
						self.issue.identifier
					));
				}

				self.manual_attention_comment_recorded.replace(true);
				self.manual_attention_error_class.replace(Some(comment.error_class.clone()));

				let verb = if created { "added" } else { "already existed for" };

				DynamicToolCallResponse::success(format!(
					"Manual-attention public summary {verb} issue `{}`.",
					self.issue.identifier
				))
			},
			Err(error) => DynamicToolCallResponse::failure(format!(
				"Failed to add a manual-attention public summary to issue `{}`: {error}",
				self.issue.identifier
			)),
		}
	}

	fn normalize_manual_attention_comment(
		parsed: CommentArgs,
	) -> Result<NormalizedManualAttentionComment, String> {
		let error_class = normalize_required_comment_field(parsed.error_class, "error_class")?;
		let next_action = normalize_required_comment_field(parsed.next_action, "next_action")?;
		let blockers = tracker_tool_bridge::normalize_progress_list(parsed.blockers);
		let evidence = tracker_tool_bridge::normalize_progress_list(parsed.evidence);
		let failed_command =
			tracker_tool_bridge::normalize_optional_progress_field(parsed.failed_command);
		let raw_error = tracker_tool_bridge::normalize_optional_progress_field(parsed.raw_error);
		let summary = tracker_tool_bridge::normalize_optional_progress_field(parsed.summary);
		let decision_request =
			parsed.decision_request.map(Self::normalize_authority_decision_request).transpose()?;

		validate_manual_attention_error_class(&error_class)?;

		if blockers.is_empty() {
			return Err(format!(
				"`{ISSUE_COMMENT_TOOL_NAME}` kind `{COMMENT_KIND_MANUAL_ATTENTION}` requires at least one public `blockers` item."
			));
		}
		if evidence.is_empty() {
			return Err(format!(
				"`{ISSUE_COMMENT_TOOL_NAME}` kind `{COMMENT_KIND_MANUAL_ATTENTION}` requires at least one public `evidence` item."
			));
		}

		Ok(NormalizedManualAttentionComment {
			error_class,
			next_action,
			blockers,
			evidence,
			failed_command,
			raw_error,
			summary,
			decision_request,
		})
	}

	fn apply_manual_attention_label(&self) -> Result<(), String> {
		let label = self.workflow.frontmatter().tracker().needs_attention_label();
		let current_issue = match self.refreshed_issue_snapshot() {
			Ok(Some(issue)) => issue,
			Ok(None) => {
				return Err(format!(
					"Failed to refresh issue `{}` before applying manual-attention label `{label}`: tracker returned no current snapshot.",
					self.issue.identifier
				));
			},
			Err(error) => {
				return Err(format!(
					"Failed to refresh issue `{}` before applying manual-attention label `{label}`: {error}",
					self.issue.identifier
				));
			},
		};

		tracker::set_issue_label_presence(self.tracker, &current_issue, label, true).map_err(
			|error| {
				format!(
					"Failed to add label `{label}` to issue `{}`: {error}",
					self.issue.identifier
				)
			},
		)?;

		Ok(())
	}

	fn normalize_authority_decision_request(
		parsed: AuthorityDecisionRequestArgs,
	) -> Result<NormalizedAuthorityDecisionRequest, String> {
		let decision_request_id = normalize_required_decision_request_field(
			Some(parsed.decision_request_id),
			"decision_request_id",
		)?;
		let reason_code =
			normalize_required_decision_request_field(Some(parsed.reason_code), "reason_code")?;
		let boundary_type =
			normalize_required_decision_request_field(Some(parsed.boundary_type), "boundary_type")?;
		let proposed_change = normalize_required_decision_request_field(
			Some(parsed.proposed_change),
			"proposed_change",
		)?;
		let why_exceeds_authority = normalize_required_decision_request_field(
			Some(parsed.why_exceeds_authority),
			"why_exceeds_authority",
		)?;
		let recommendation = normalize_required_decision_request_field(
			Some(parsed.recommendation),
			"recommendation",
		)?;
		let resume_condition = normalize_required_decision_request_field(
			Some(parsed.resume_condition),
			"resume_condition",
		)?;
		let options = parsed
			.options
			.into_iter()
			.map(Self::normalize_authority_decision_option)
			.collect::<Result<Vec<_>, _>>()?;
		let retained_worktree_evidence =
			tracker_tool_bridge::normalize_progress_list(parsed.retained_worktree_evidence);
		let retained_diff_evidence =
			tracker_tool_bridge::normalize_progress_list(parsed.retained_diff_evidence);
		let recovery_attempt_context =
			tracker_tool_bridge::normalize_progress_list(parsed.recovery_attempt_context);

		if parsed.boundary_check_id < 1 {
			return Err(String::from(
				"`decision_request.boundary_check_id` must be a positive private evidence record id.",
			));
		}

		validate_public_error_class(&reason_code)?;
		validate_public_error_class(&boundary_type)?;

		if options.is_empty() {
			return Err(String::from(
				"`decision_request.options` must include at least one public option.",
			));
		}

		validate_public_decision_request_text(
			&decision_request_id,
			&proposed_change,
			&why_exceeds_authority,
			&options,
			&recommendation,
			&resume_condition,
		)?;

		Ok(NormalizedAuthorityDecisionRequest {
			boundary_check_id: parsed.boundary_check_id,
			decision_request_id,
			reason_code,
			boundary_type,
			proposed_change,
			why_exceeds_authority,
			options,
			recommendation,
			resume_condition,
			retained_worktree_evidence,
			retained_diff_evidence,
			recovery_attempt_context,
		})
	}

	fn normalize_authority_decision_option(
		parsed: AuthorityDecisionOptionArgs,
	) -> Result<NormalizedAuthorityDecisionOption, String> {
		let label = normalize_required_decision_request_field(Some(parsed.label), "option.label")?;
		let description = normalize_required_decision_request_field(
			Some(parsed.description),
			"option.description",
		)?;

		validate_public_decision_request_field("decision_request.option.label", &label)?;
		validate_public_decision_request_field(
			"decision_request.option.description",
			&description,
		)?;

		Ok(NormalizedAuthorityDecisionOption { label, description })
	}

	fn append_private_authority_decision_request(
		&self,
		review_context: &ReviewHandoffContext,
		state_store: &StateStore,
		decision_request: &NormalizedAuthorityDecisionRequest,
	) -> Result<(), String> {
		let boundary_events = state_store
			.list_private_execution_events(
				&review_context.service_id,
				&self.issue.id,
				&review_context.run_id,
				review_context.attempt_number,
			)
			.map_err(|error| {
				format!(
					"Failed to inspect authority boundary evidence for issue `{}`: {error}",
					self.issue.identifier
				)
			})?;
		let Some(boundary_event) = boundary_events
			.iter()
			.find(|event| event.record_id() == decision_request.boundary_check_id)
		else {
			return Err(format!(
				"`decision_request.boundary_check_id` {} does not reference a private event for issue `{}` run `{}` attempt {}.",
				decision_request.boundary_check_id,
				self.issue.identifier,
				review_context.run_id,
				review_context.attempt_number
			));
		};

		if boundary_event.event_type() != AUTHORITY_BOUNDARY_CHECK_EVENT_TYPE {
			return Err(format!(
				"`decision_request.boundary_check_id` {} references `{}` instead of an authority boundary check.",
				decision_request.boundary_check_id,
				boundary_event.event_type()
			));
		}

		let disposition = boundary_event.payload().get("disposition").and_then(Value::as_str);

		if disposition != Some("requires_human") {
			return Err(format!(
				"`decision_request.boundary_check_id` {} must reference a `requires_human` authority boundary check.",
				decision_request.boundary_check_id
			));
		}

		let options = decision_request
			.options
			.iter()
			.map(|option| AuthorityDecisionOption {
				label: option.label.as_str(),
				description: option.description.as_str(),
			})
			.collect::<Vec<_>>();
		let retained_worktree_evidence = decision_request
			.retained_worktree_evidence
			.iter()
			.map(String::as_str)
			.collect::<Vec<_>>();
		let retained_diff_evidence =
			decision_request.retained_diff_evidence.iter().map(String::as_str).collect::<Vec<_>>();
		let recovery_attempt_context = decision_request
			.recovery_attempt_context
			.iter()
			.map(String::as_str)
			.collect::<Vec<_>>();

		orchestrator::record_authority_decision_request_private_event(
			state_store,
			AuthorityDecisionRequestInput {
				project_id: &review_context.service_id,
				issue_id: &self.issue.id,
				issue_identifier: &self.issue.identifier,
				run_id: &review_context.run_id,
				attempt_number: review_context.attempt_number,
				boundary_check_record_id: decision_request.boundary_check_id,
				decision_request_id: &decision_request.decision_request_id,
				reason_code: &decision_request.reason_code,
				boundary_type: &decision_request.boundary_type,
				proposed_change: &decision_request.proposed_change,
				why_exceeds_authority: &decision_request.why_exceeds_authority,
				options,
				recommendation: &decision_request.recommendation,
				resume_condition: &decision_request.resume_condition,
				retained_worktree_evidence,
				retained_diff_evidence,
				recovery_attempt_context,
			},
		)
		.map(|_| ())
		.map_err(|error| {
			format!(
				"Failed to persist authority decision request `{}` for issue `{}`: {error}",
				decision_request.decision_request_id, self.issue.identifier
			)
		})
	}

	fn manual_attention_execution_event(
		&self,
		review_context: &ReviewHandoffContext,
		comment: &NormalizedManualAttentionComment,
	) -> LinearExecutionEventRecord {
		let decision_request_id = comment
			.decision_request
			.as_ref()
			.map(|request| request.decision_request_id.as_str())
			.unwrap_or_default();
		let anchor = records::stable_event_anchor(&[
			COMMENT_KIND_MANUAL_ATTENTION,
			comment.error_class.as_str(),
			comment.next_action.as_str(),
			comment.failed_command.as_deref().unwrap_or_default(),
			comment.raw_error.as_deref().unwrap_or_default(),
			decision_request_id,
		]);
		let mut record = LinearExecutionEventRecord::new(
			LinearExecutionEventIdentity {
				service_id: &review_context.service_id,
				issue_id: &self.issue.id,
				issue_identifier: &self.issue.identifier,
				run_id: &review_context.run_id,
				attempt_number: review_context.attempt_number,
			},
			"needs_attention",
			tracker_tool_bridge::current_timestamp(),
			&anchor,
		);

		record.branch = Some(review_context.branch_name.clone());
		record.worktree_path = Some(review_context.worktree_path.clone());
		record.pr_url = review_context.recorded_pr_url.clone();
		record.summary = Some(
			comment
				.summary
				.clone()
				.unwrap_or_else(|| format!("Manual attention required: {}.", comment.error_class)),
		);
		record.error_class = Some(comment.error_class.clone());
		record.next_action = Some(comment.next_action.clone());
		record.blockers = Some(comment.blockers.clone());
		record.evidence = Some(comment.evidence.clone());
		record.terminal_path = Some(String::from(MANUAL_ATTENTION_TERMINAL_PATH));
		record.failed_command = comment.failed_command.clone();
		record.raw_error = comment.raw_error.clone();

		record
	}

	pub(super) fn handle_review_checkpoint(&self, arguments: Value) -> DynamicToolCallResponse {
		let parsed = match serde_json::from_value::<ReviewCheckpointArgs>(arguments) {
			Ok(parsed) => parsed,
			Err(error) => {
				return DynamicToolCallResponse::failure(format!(
					"Invalid `issue.review_checkpoint` arguments: {error}"
				));
			},
		};

		if let Err(error) = self.ensure_issue_scope(&parsed.scope) {
			return DynamicToolCallResponse::failure(error);
		}

		let Some(review_context) = self.review_context.as_ref() else {
			return DynamicToolCallResponse::failure(String::from(
				"`issue_review_checkpoint` is unavailable for this run.",
			));
		};

		if !review_context.decodex_review_checkpoint_enabled() {
			return DynamicToolCallResponse::failure(format!(
				"`issue_review_checkpoint` is disabled because `[codex].review = \"{}\"` for this run.",
				review_context.review_level.as_str()
			));
		}

		let prepared = match self.prepare_review_checkpoint(parsed, review_context) {
			Ok(prepared) => prepared,
			Err(error) => return DynamicToolCallResponse::failure(error),
		};
		let details_json = match self.review_checkpoint_details_json(&prepared.checkpoint_payload) {
			Ok(details_json) => details_json,
			Err(error) => return DynamicToolCallResponse::failure(error),
		};

		if let Err(error) = self.persist_review_policy_state(
			review_context,
			prepared.review_policy_phase,
			prepared.review_policy_status,
			&prepared.head_sha,
			prepared.nonclean_rounds,
			&details_json,
		) {
			return DynamicToolCallResponse::failure(error);
		}
		if let Err(error) = self.append_private_review_checkpoint(
			review_context,
			prepared.review_policy_phase,
			prepared.review_policy_status,
			&prepared.head_sha,
			prepared.nonclean_rounds,
			&prepared.checkpoint_payload,
		) {
			return DynamicToolCallResponse::failure(error);
		}

		let message = self.review_checkpoint_success_message(
			prepared.review_policy_phase,
			prepared.review_policy_status,
			&prepared.head_sha,
			prepared.nonclean_rounds,
			ReviewCheckpointPayloadCounts {
				evidence: prepared.checkpoint_payload.evidence.len(),
				accepted_findings: prepared.checkpoint_payload.accepted_findings.len(),
				rejected_findings: prepared.checkpoint_payload.rejected_findings.len(),
				finding_routes: prepared.checkpoint_payload.finding_routes.len(),
				current_blockers: current_review_blocker_findings(&prepared.checkpoint_payload)
					.count(),
			},
		);

		if let Some(response) = self.review_checkpoint_churn_stop_response(
			prepared.review_policy_status,
			prepared.nonclean_rounds,
			&prepared.checkpoint_payload,
			&message,
		) {
			return response;
		}

		DynamicToolCallResponse::success(message)
	}

	fn prepare_review_checkpoint(
		&self,
		parsed: ReviewCheckpointArgs,
		review_context: &ReviewHandoffContext,
	) -> Result<PreparedReviewCheckpoint, String> {
		let Some(review_policy_phase) = ReviewPolicyPhase::for_mode(review_context.mode) else {
			return Err(String::from(
				"`issue_review_checkpoint` is unavailable for retained closeout runs.",
			));
		};
		let review_policy_status = ReviewPolicyStatus::parse(&parsed.status)?;
		let local_repo = self.current_local_repo_details(review_context)?;
		let head_sha = self.canonicalize_current_lane_head_sha(
			ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
			parsed.head_sha.as_str(),
			&local_repo.head_oid,
		)?;

		self.ensure_review_checkpoint_committed_head(&local_repo)?;

		let mut checkpoint_payload = normalize_review_checkpoint_payload(
			parsed,
			review_policy_phase,
			review_policy_status,
			&head_sha,
			&local_repo,
		)?;
		let policy_update = self.review_checkpoint_finding_policy_update(
			review_context,
			review_policy_phase,
			review_policy_status,
			&head_sha,
			&checkpoint_payload,
		)?;

		validate_review_cost_control_policy_state(
			&checkpoint_payload.review_cost_control,
			&policy_update,
		)?;

		checkpoint_payload.finding_policy = policy_update.finding_policy;

		Ok(PreparedReviewCheckpoint {
			review_policy_phase,
			review_policy_status,
			head_sha,
			checkpoint_payload,
			nonclean_rounds: policy_update.nonclean_rounds,
		})
	}

	fn review_checkpoint_details_json(
		&self,
		checkpoint_payload: &NormalizedReviewCheckpointPayload,
	) -> Result<String, String> {
		serde_json::to_string(checkpoint_payload).map_err(|error| {
			format!(
				"Failed to serialize the structured review checkpoint for issue `{}`: {error}",
				self.issue.identifier
			)
		})
	}

	fn review_checkpoint_churn_stop_response(
		&self,
		review_policy_status: ReviewPolicyStatus,
		nonclean_rounds: i64,
		checkpoint_payload: &NormalizedReviewCheckpointPayload,
		message: &str,
	) -> Option<DynamicToolCallResponse> {
		if review_policy_status != ReviewPolicyStatus::Findings
			|| nonclean_rounds < REVIEW_POLICY_CONVERGENCE_BUDGET
		{
			return None;
		}

		let fingerprint = checkpoint_payload
			.finding_policy
			.stop_fingerprint
			.as_ref()
			.map_or_else(String::new, |fingerprint| {
				format!(" Finding fingerprint `{fingerprint}` caused the stop.")
			});

		Some(DynamicToolCallResponse::failure(format!(
			"{message} Review churn threshold exceeded.{fingerprint} Stop the current repair strategy now and route through architecture recovery or human attention before making further repair mutations."
		)))
	}

	fn review_checkpoint_finding_policy_update(
		&self,
		review_context: &ReviewHandoffContext,
		review_policy_phase: ReviewPolicyPhase,
		review_policy_status: ReviewPolicyStatus,
		head_sha: &str,
		checkpoint_payload: &NormalizedReviewCheckpointPayload,
	) -> Result<ReviewFindingPolicyUpdate, String> {
		let previous_state = self
			.review_policy_artifact_for_head(review_context, review_policy_phase, head_sha)
			.map_err(|error| error.to_string())?;
		let previous_finding_policy = previous_state
			.as_ref()
			.and_then(|previous_state| {
				review_finding_policy_from_previous_state(previous_state, review_policy_phase)
			})
			.unwrap_or_default();
		let previous_nonclean_rounds = previous_state
			.as_ref()
			.filter(|previous_state| previous_state.phase == review_policy_phase)
			.map_or(0, |previous_state| previous_state.nonclean_rounds);
		let previous_threshold_exceeded = previous_state.as_ref().is_some_and(|previous_state| {
			previous_state.phase == review_policy_phase
				&& previous_state.status == ReviewPolicyStatus::Findings
				&& previous_state.nonclean_rounds >= REVIEW_POLICY_CONVERGENCE_BUDGET
		});

		if review_policy_status == ReviewPolicyStatus::Findings
			&& (previous_finding_policy.stop_fingerprint.is_some() || previous_threshold_exceeded)
		{
			return Err(format!(
				"Review churn threshold already exceeded for issue `{}`; do not record another findings checkpoint. Route through architecture recovery or human attention before making further repair mutations.",
				self.issue.identifier
			));
		}

		Ok(review_finding_policy_update(
			previous_finding_policy,
			previous_nonclean_rounds,
			review_policy_phase,
			review_policy_status,
			head_sha,
			checkpoint_payload,
		))
	}

	fn review_checkpoint_success_message(
		&self,
		review_policy_phase: ReviewPolicyPhase,
		review_policy_status: ReviewPolicyStatus,
		head_sha: &str,
		nonclean_rounds: i64,
		counts: ReviewCheckpointPayloadCounts,
	) -> String {
		let evidence_suffix = format!(
			"{} evidence item(s), {} accepted finding(s), {} rejected finding(s), {} route(s), and {} current blocker(s) recorded",
			counts.evidence,
			counts.accepted_findings,
			counts.rejected_findings,
			counts.finding_routes,
			counts.current_blockers,
		);

		match review_policy_status {
			ReviewPolicyStatus::Clean => format!(
				"Recorded a clean `{}` review checkpoint for issue `{}` at HEAD `{head_sha}`; {evidence_suffix}.",
				review_policy_phase.as_str(),
				self.issue.identifier,
			),
			ReviewPolicyStatus::Findings => format!(
				"Recorded `{}` review findings for issue `{}` at HEAD `{head_sha}`; max unresolved finding repeat count now `{nonclean_rounds}`; {evidence_suffix}.",
				review_policy_phase.as_str(),
				self.issue.identifier,
			),
			ReviewPolicyStatus::NeedsArchitectureReview => format!(
				"Recorded `needs_architecture_review` for issue `{}` at HEAD `{head_sha}`; Decodex will require human architecture review if the turn ends on this checkpoint.",
				self.issue.identifier,
			),
			ReviewPolicyStatus::Blocked => format!(
				"Recorded `blocked` for issue `{}` at HEAD `{head_sha}`; Decodex will require human intervention if the turn ends on this checkpoint.",
				self.issue.identifier,
			),
		}
	}

	fn append_private_review_checkpoint(
		&self,
		review_context: &ReviewHandoffContext,
		review_policy_phase: ReviewPolicyPhase,
		review_policy_status: ReviewPolicyStatus,
		head_sha: &str,
		nonclean_rounds: i64,
		checkpoint_payload: &NormalizedReviewCheckpointPayload,
	) -> Result<(), String> {
		let state_store = self.state_store.ok_or_else(|| {
			format!(
				"`{ISSUE_REVIEW_CHECKPOINT_TOOL_NAME}` requires the Decodex runtime state store for issue `{}`.",
				self.issue.identifier
			)
		})?;
		let private_payload = serde_json::json!({
			"phase": review_policy_phase.as_str(),
			"status": review_policy_status.as_str(),
			"head_sha": head_sha,
			"nonclean_rounds": nonclean_rounds,
			"active_fingerprints": &checkpoint_payload.finding_policy.active_fingerprints,
			"stop_fingerprint": &checkpoint_payload.finding_policy.stop_fingerprint,
			"route_counts": &checkpoint_payload.finding_route_summary.route_counts,
			"route_next_action": &checkpoint_payload.finding_route_summary.next_action,
			"review_class": &checkpoint_payload.review_cost_control.review_class,
			"risk_class": &checkpoint_payload.review_cost_control.risk_class,
			"compact_eligible": checkpoint_payload.review_cost_control.compact_eligible,
			"review_fallback_reason": &checkpoint_payload.review_cost_control.fallback_reason,
			"review": checkpoint_payload,
		});

		state_store
			.append_private_execution_event(
				&review_context.service_id,
				&self.issue.id,
				&review_context.run_id,
				review_context.attempt_number,
				"review_checkpoint",
				private_payload,
			)
			.map(|_| ())
			.map_err(|error| {
				format!(
					"Failed to persist the private review checkpoint for issue `{}`: {error}",
					self.issue.identifier
				)
			})
	}

	fn ensure_review_checkpoint_committed_head(
		&self,
		local_repo: &LocalRepoDetails,
	) -> Result<(), String> {
		if local_repo.review_worktree_clean() {
			return Ok(());
		}

		Err(format!(
			"`{ISSUE_REVIEW_CHECKPOINT_TOOL_NAME}` requires a clean committed lane HEAD before recording formal Decodex Review evidence. Commit or revert review-blocking local changes, rerun required validation, then request review for the committed HEAD. Review-blocking local changes: {}",
			tracker_tool_bridge::summarize_review_blocking_changes(
				&local_repo.review_blocking_changes
			)
		))
	}

	fn append_review_completion_intent(
		&self,
		review_context: &ReviewHandoffContext,
		path: RunCompletionDisposition,
		pull_request: &PullRequestDetails,
		summary: &str,
	) -> Result<(), String> {
		let state_store = self.state_store.ok_or_else(|| {
			format!(
				"`{}` requires the Decodex runtime state store for issue `{}`.",
				self.required_pr_completion_tool_name(),
				self.issue.identifier
			)
		})?;

		state_store
			.append_private_execution_event(
				&review_context.service_id,
				&self.issue.id,
				&review_context.run_id,
				review_context.attempt_number,
				REVIEW_COMPLETION_INTENT_EVENT_TYPE,
				serde_json::json!({
					"path": path.as_str(),
					"mode": review_context.mode.as_str(),
					"branch": review_context.branch_name.as_str(),
					"worktree_path": review_context.worktree_path.as_str(),
					"pr_url": pull_request.url.as_str(),
					"pr_base_ref": pull_request.base_ref_name.as_str(),
					"pr_head_ref": pull_request.head_ref_name.as_str(),
					"pr_head_oid": pull_request.head_ref_oid.as_str(),
					"summary": summary,
				}),
			)
			.map(|_| ())
			.map_err(|error| {
				format!(
					"Failed to persist review completion intent for issue `{}`: {error}",
					self.issue.identifier
				)
			})
	}

	fn append_terminal_finalize_event(
		&self,
		review_context: &ReviewHandoffContext,
		path: RunCompletionDisposition,
	) -> Result<(), String> {
		let state_store = self.state_store.ok_or_else(|| {
			format!(
				"`{ISSUE_TERMINAL_FINALIZE_TOOL_NAME}` requires the Decodex runtime state store for issue `{}`.",
				self.issue.identifier
			)
		})?;

		state_store
			.append_private_execution_event(
				&review_context.service_id,
				&self.issue.id,
				&review_context.run_id,
				review_context.attempt_number,
				TERMINAL_FINALIZE_EVENT_TYPE,
				serde_json::json!({
					"path": path.as_str(),
					"mode": review_context.mode.as_str(),
					"branch": review_context.branch_name.as_str(),
					"worktree_path": review_context.worktree_path.as_str(),
				}),
			)
			.map(|_| ())
			.map_err(|error| {
				format!(
					"Failed to persist terminal finalize intent for issue `{}`: {error}",
					self.issue.identifier
				)
			})
	}

	fn ensure_docs_impact_checkpoint(
		&self,
		review_context: &ReviewHandoffContext,
		path: RunCompletionDisposition,
	) -> Result<(), String> {
		let state_store = self.state_store.ok_or_else(|| {
			format!(
				"`{ISSUE_TERMINAL_FINALIZE_TOOL_NAME}` requires the Decodex runtime state store for issue `{}`.",
				self.issue.identifier
			)
		})?;
		let local_repo = self.current_local_repo_details(review_context)?;
		let events = state_store
			.list_private_execution_events(
				&review_context.service_id,
				&self.issue.id,
				&review_context.run_id,
				review_context.attempt_number,
			)
			.map_err(|error| {
				format!(
					"Failed to inspect docs-impact checkpoints for issue `{}`: {error}",
					self.issue.identifier
				)
			})?;
		let Some(checkpoint) =
			events.iter().rev().find(|event| event.event_type() == "progress_checkpoint")
		else {
			return Err(format!(
				"`{ISSUE_TERMINAL_FINALIZE_TOOL_NAME}` path `{}` requires a prior `{ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME}` with `docs_impact` for the current lane HEAD `{}`.",
				path.as_str(),
				local_repo.head_oid
			));
		};
		let has_docs_impact = checkpoint
			.payload()
			.get("docs_impact")
			.and_then(Value::as_str)
			.is_some_and(|value| DocsImpact::parse(value).is_ok());
		let matches_current_head = checkpoint
			.payload()
			.get("head_sha")
			.and_then(Value::as_str)
			.is_some_and(|head_sha| head_sha == local_repo.head_oid);

		if has_docs_impact && matches_current_head {
			return Ok(());
		}

		Err(format!(
			"`{ISSUE_TERMINAL_FINALIZE_TOOL_NAME}` path `{}` requires the latest `{ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME}` to record `docs_impact` for the current lane HEAD `{}`.",
			path.as_str(),
			local_repo.head_oid
		))
	}

	fn clear_review_policy_state_after_completion(
		&self,
		review_context: &ReviewHandoffContext,
		tool_name: &str,
	) -> Result<(), String> {
		if let Some(state_store) = self.state_store {
			state_store
				.clear_review_policy_checkpoints_for_run_attempt(
					&review_context.service_id,
					&self.issue.id,
					&review_context.run_id,
					review_context.attempt_number,
				)
				.map_err(|error| {
					format!(
						"Failed to clear review policy state for issue `{}` after recording `{tool_name}`: {error}",
						self.issue.identifier
					)
				})?;
		} else if review_context.decodex_review_checkpoint_enabled() {
			return Err(format!(
				"Runtime state store is required to clear review policy state for issue `{}` after recording `{tool_name}`.",
				self.issue.identifier
			));
		}

		Ok(())
	}

	pub(super) fn handle_review_handoff(&self, arguments: Value) -> DynamicToolCallResponse {
		let parsed = match serde_json::from_value::<ReviewHandoffArgs>(arguments) {
			Ok(parsed) => parsed,
			Err(error) => {
				return DynamicToolCallResponse::failure(format!(
					"Invalid `issue.review_handoff` arguments: {error}"
				));
			},
		};

		if let Err(error) = self.ensure_issue_scope(&parsed.scope) {
			return DynamicToolCallResponse::failure(error);
		}

		let Some(review_context) = self.review_context.as_ref() else {
			return DynamicToolCallResponse::failure(String::from(
				"`issue_review_handoff` is unavailable for this run.",
			));
		};

		if review_context.mode != ReviewExecutionMode::Handoff {
			return DynamicToolCallResponse::failure(String::from(
				"`issue_review_handoff` is unavailable for retained review-repair runs.",
			));
		}

		let pr_url = parsed.pr_url.trim();

		if pr_url.is_empty() {
			return DynamicToolCallResponse::failure(String::from(
				"`issue_review_handoff` requires a non-empty `pr_url`.",
			));
		}

		let summary = tracker_tool_bridge::normalize_summary(&parsed.summary);

		if summary.is_empty() {
			return DynamicToolCallResponse::failure(String::from(
				"`issue_review_handoff` requires a non-empty `summary`.",
			));
		}

		let pull_request = match self.validate_review_action_pr(review_context, pr_url) {
			Ok(pull_request) => pull_request,
			Err(error) => return DynamicToolCallResponse::failure(error),
		};

		if let Err(error) = self.require_clean_review_checkpoint(review_context) {
			return DynamicToolCallResponse::failure(error);
		}
		if let Err(error) = self.clear_review_policy_state_after_completion(
			review_context,
			ISSUE_REVIEW_HANDOFF_TOOL_NAME,
		) {
			return DynamicToolCallResponse::failure(error);
		}
		if let Err(error) = self.append_review_completion_intent(
			review_context,
			RunCompletionDisposition::ReviewHandoff,
			&pull_request,
			&summary,
		) {
			return DynamicToolCallResponse::failure(error);
		}

		self.pending_review_completion.borrow_mut().replace(PendingReviewCompletion::Handoff(
			PendingReviewAction { pr_url: pull_request.url.clone(), summary },
		));

		DynamicToolCallResponse::success(format!(
			"Recorded review handoff for issue `{}` with PR `{}`. Decodex will apply the completion comment and move the issue to `{}` after service validation passes.",
			self.issue.identifier,
			pull_request.url,
			self.workflow.frontmatter().tracker().success_state()
		))
	}

	pub(super) fn handle_review_repair_complete(
		&self,
		arguments: Value,
	) -> DynamicToolCallResponse {
		let parsed = match serde_json::from_value::<ReviewHandoffArgs>(arguments) {
			Ok(parsed) => parsed,
			Err(error) => {
				return DynamicToolCallResponse::failure(format!(
					"Invalid `issue.review_repair_complete` arguments: {error}"
				));
			},
		};

		if let Err(error) = self.ensure_issue_scope(&parsed.scope) {
			return DynamicToolCallResponse::failure(error);
		}

		let Some(review_context) = self.review_context.as_ref() else {
			return DynamicToolCallResponse::failure(String::from(
				"`issue_review_repair_complete` is unavailable for this run.",
			));
		};

		if review_context.mode != ReviewExecutionMode::Repair {
			return DynamicToolCallResponse::failure(String::from(
				"`issue_review_repair_complete` is unavailable before a retained in-review repair run starts.",
			));
		}

		let pr_url = parsed.pr_url.trim();

		if pr_url.is_empty() {
			return DynamicToolCallResponse::failure(String::from(
				"`issue_review_repair_complete` requires a non-empty `pr_url`.",
			));
		}

		let summary = tracker_tool_bridge::normalize_summary(&parsed.summary);

		if summary.is_empty() {
			return DynamicToolCallResponse::failure(String::from(
				"`issue_review_repair_complete` requires a non-empty `summary`.",
			));
		}

		let pull_request = match self.validate_review_action_pr(review_context, pr_url) {
			Ok(pull_request) => pull_request,
			Err(error) => return DynamicToolCallResponse::failure(error),
		};

		if let Err(error) = self.require_clean_review_checkpoint(review_context) {
			return DynamicToolCallResponse::failure(error);
		}
		if let Err(error) = self.clear_review_policy_state_after_completion(
			review_context,
			ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME,
		) {
			return DynamicToolCallResponse::failure(error);
		}
		if let Err(error) = self.append_review_completion_intent(
			review_context,
			RunCompletionDisposition::ReviewRepair,
			&pull_request,
			&summary,
		) {
			return DynamicToolCallResponse::failure(error);
		}

		self.pending_review_completion.borrow_mut().replace(PendingReviewCompletion::Repair(
			PendingReviewAction { pr_url: pull_request.url.clone(), summary },
		));

		DynamicToolCallResponse::success(format!(
			"Recorded retained review repair completion for issue `{}` on PR `{}`. Decodex will persist the updated review lineage after service validation passes.",
			self.issue.identifier, pull_request.url
		))
	}

	pub(super) fn handle_closeout_complete(&self, arguments: Value) -> DynamicToolCallResponse {
		let parsed = match serde_json::from_value::<ReviewHandoffArgs>(arguments) {
			Ok(parsed) => parsed,
			Err(error) => {
				return DynamicToolCallResponse::failure(format!(
					"Invalid `issue.closeout_complete` arguments: {error}"
				));
			},
		};

		if let Err(error) = self.ensure_issue_scope(&parsed.scope) {
			return DynamicToolCallResponse::failure(error);
		}

		let Some(review_context) = self.review_context.as_ref() else {
			return DynamicToolCallResponse::failure(String::from(
				"`issue_closeout_complete` is unavailable for this run.",
			));
		};

		if review_context.mode != ReviewExecutionMode::Closeout {
			return DynamicToolCallResponse::failure(String::from(
				"`issue_closeout_complete` is unavailable before a retained post-review closeout run starts.",
			));
		}

		let pr_url = parsed.pr_url.trim();

		if pr_url.is_empty() {
			return DynamicToolCallResponse::failure(String::from(
				"`issue_closeout_complete` requires a non-empty `pr_url`.",
			));
		}

		let summary = tracker_tool_bridge::normalize_summary(&parsed.summary);

		if summary.is_empty() {
			return DynamicToolCallResponse::failure(String::from(
				"`issue_closeout_complete` requires a non-empty `summary`.",
			));
		}

		let pull_request = match self.validate_closeout_pr(review_context, pr_url) {
			Ok(pull_request) => pull_request,
			Err(error) => return DynamicToolCallResponse::failure(error),
		};

		if let Err(error) = self.validate_closeout_issue_completed_state() {
			return DynamicToolCallResponse::failure(error);
		}
		if let Err(error) = self.append_review_completion_intent(
			review_context,
			RunCompletionDisposition::Closeout,
			&pull_request,
			&summary,
		) {
			return DynamicToolCallResponse::failure(error);
		}

		self.pending_review_completion.borrow_mut().replace(PendingReviewCompletion::Closeout(
			PendingReviewAction { pr_url: pull_request.url.clone(), summary },
		));

		DynamicToolCallResponse::success(format!(
			"Recorded retained closeout completion for issue `{}` on merged PR `{}`. Decodex will validate the merged lineage and terminal tracker state before cleaning up the lane.",
			self.issue.identifier, pull_request.url
		))
	}

	pub(super) fn handle_add_label(&self, arguments: Value) -> DynamicToolCallResponse {
		let parsed = match serde_json::from_value::<LabelArgs>(arguments) {
			Ok(parsed) => parsed,
			Err(error) => {
				return DynamicToolCallResponse::failure(format!(
					"Invalid `issue.label.add` arguments: {error}"
				));
			},
		};

		if let Err(error) = self.ensure_issue_scope(&parsed.scope) {
			return DynamicToolCallResponse::failure(error);
		}

		let allowed_labels = [
			self.workflow.frontmatter().tracker().opt_out_label(),
			self.workflow.frontmatter().tracker().needs_attention_label(),
		];

		if !allowed_labels.iter().any(|label| label == &parsed.label) {
			return DynamicToolCallResponse::failure(format!(
				"Label `{}` is outside the allowed tracker tool policy.",
				parsed.label
			));
		}

		let manual_attention_label =
			parsed.label == self.workflow.frontmatter().tracker().needs_attention_label();

		if manual_attention_label {
			self.manual_attention_requested.replace(true);

			return DynamicToolCallResponse::success(format!(
				"Manual-attention label intent recorded for issue `{}`. Call `{ISSUE_COMMENT_TOOL_NAME}` kind `{COMMENT_KIND_MANUAL_ATTENTION}` next so Decodex can validate the blocker and apply label `{}`.",
				self.issue.identifier, parsed.label
			));
		}

		let current_issue = match self.refreshed_issue_snapshot() {
			Ok(Some(issue)) => issue,
			Ok(None) => {
				return DynamicToolCallResponse::failure(format!(
					"Failed to refresh issue `{}` before updating labels: tracker returned no current snapshot.",
					self.issue.identifier
				));
			},
			Err(error) => {
				return DynamicToolCallResponse::failure(format!(
					"Failed to refresh issue `{}` before updating labels: {error}",
					self.issue.identifier
				));
			},
		};
		let label_added = match tracker::set_issue_label_presence(
			self.tracker,
			&current_issue,
			&parsed.label,
			true,
		) {
			Ok(label_added) => label_added,
			Err(error) => {
				return DynamicToolCallResponse::failure(format!(
					"Failed to add label `{}` to issue `{}`: {error}",
					parsed.label, self.issue.identifier
				));
			},
		};

		if !label_added {
			self.record_label_add_local_effects(&parsed.label, manual_attention_label);

			return DynamicToolCallResponse::success(format!(
				"Issue `{}` already has label `{}`.",
				self.issue.identifier, parsed.label
			));
		}

		self.record_label_add_local_effects(&parsed.label, manual_attention_label);

		DynamicToolCallResponse::success(format!(
			"Label `{}` added to issue `{}`.",
			parsed.label, self.issue.identifier
		))
	}

	fn record_label_add_local_effects(&self, label: &str, manual_attention_label: bool) {
		if manual_attention_label {
			self.manual_attention_requested.replace(true);
		} else if label == self.workflow.frontmatter().tracker().opt_out_label() {
			self.local_opt_out_requested.replace(true);
			self.record_continuation_blocking_write(format!(
				"`{ISSUE_LABEL_ADD_TOOL_NAME}` with label `{label}`",
			));
		}
	}

	pub(super) fn handle_terminal_finalize(&self, arguments: Value) -> DynamicToolCallResponse {
		let parsed = match serde_json::from_value::<TerminalFinalizeArgs>(arguments) {
			Ok(parsed) => parsed,
			Err(error) => {
				return DynamicToolCallResponse::failure(format!(
					"Invalid `issue.terminal_finalize` arguments: {error}"
				));
			},
		};

		if let Err(error) = self.ensure_issue_scope(&parsed.scope) {
			return DynamicToolCallResponse::failure(error);
		}

		let requested_path = match parsed.path.as_str() {
			"review_handoff" => RunCompletionDisposition::ReviewHandoff,
			"review_repair" => RunCompletionDisposition::ReviewRepair,
			"closeout" => RunCompletionDisposition::Closeout,
			"manual_attention" => RunCompletionDisposition::ManualAttention,
			other => {
				return DynamicToolCallResponse::failure(format!(
					"`{ISSUE_TERMINAL_FINALIZE_TOOL_NAME}` path must be `review_handoff`, `review_repair`, `closeout`, or `manual_attention`, not `{other}`."
				));
			},
		};
		let actual_path = match self.completion_disposition() {
			Ok(actual_path) => actual_path,
			Err(error) => return DynamicToolCallResponse::failure(error.to_string()),
		};

		if requested_path != actual_path {
			return DynamicToolCallResponse::failure(format!(
				"`{ISSUE_TERMINAL_FINALIZE_TOOL_NAME}` requested path `{}`, but the recorded terminal path is `{}`.",
				requested_path.as_str(),
				actual_path.as_str()
			));
		}

		let Some(review_context) = self.review_context.as_ref() else {
			return DynamicToolCallResponse::failure(format!(
				"`{ISSUE_TERMINAL_FINALIZE_TOOL_NAME}` is unavailable for this run."
			));
		};

		if let Err(error) = self.ensure_docs_impact_checkpoint(review_context, actual_path) {
			return DynamicToolCallResponse::failure(error);
		}
		if let Err(error) = self.append_terminal_finalize_event(review_context, actual_path) {
			return DynamicToolCallResponse::failure(error);
		}

		self.finalized_completion_path.replace(Some(actual_path));

		DynamicToolCallResponse::success(format!(
			"Finalized terminal path `{}` for issue `{}`. You can only finish the turn after this succeeds.",
			actual_path.as_str(),
			self.issue.identifier
		))
	}
}

fn review_checkpoint_reviewer_schema() -> Value {
	serde_json::json!({
		"type": "string",
		"enum": ["independent_fresh_context"]
	})
}

fn review_checkpoint_status_schema() -> Value {
	serde_json::json!({
		"type": "string",
		"enum": ["clean", "findings", "needs_architecture_review", "blocked"]
	})
}

fn review_checkpoint_contract_schema() -> Value {
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

fn review_cost_control_schema() -> Value {
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

fn review_checkpoint_checks_schema() -> Value {
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

fn review_checkpoint_findings_array_schema(rejected: bool) -> Value {
	serde_json::json!({
		"type": "array",
		"items": review_checkpoint_finding_schema(rejected)
	})
}

fn review_checkpoint_finding_routes_schema() -> Value {
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

fn non_empty_string_array_schema() -> Value {
	serde_json::json!({
		"type": "array",
		"items": { "type": "string" },
		"minItems": 1
	})
}

fn normalize_required_comment_field(
	value: Option<String>,
	field_name: &str,
) -> Result<String, String> {
	let value = tracker_tool_bridge::normalize_optional_progress_field(value).ok_or_else(|| {
		format!(
			"`{ISSUE_COMMENT_TOOL_NAME}` kind `{COMMENT_KIND_MANUAL_ATTENTION}` requires `{field_name}`."
		)
	})?;

	Ok(value)
}

fn normalize_required_decision_request_field(
	value: Option<String>,
	field_name: &str,
) -> Result<String, String> {
	tracker_tool_bridge::normalize_optional_progress_field(value)
		.ok_or_else(|| format!("`decision_request.{field_name}` must be present and non-empty."))
}

fn validate_public_decision_request_text(
	decision_request_id: &str,
	proposed_change: &str,
	why_exceeds_authority: &str,
	options: &[NormalizedAuthorityDecisionOption],
	recommendation: &str,
	resume_condition: &str,
) -> Result<(), String> {
	validate_public_decision_request_field(
		"decision_request.decision_request_id",
		decision_request_id,
	)?;
	validate_public_decision_request_field("decision_request.proposed_change", proposed_change)?;
	validate_public_decision_request_field(
		"decision_request.why_exceeds_authority",
		why_exceeds_authority,
	)?;
	validate_public_decision_request_field("decision_request.recommendation", recommendation)?;
	validate_public_decision_request_field("decision_request.resume_condition", resume_condition)?;

	for option in options {
		validate_public_decision_request_field("decision_request.option.label", &option.label)?;
		validate_public_decision_request_field(
			"decision_request.option.description",
			&option.description,
		)?;
	}

	Ok(())
}

fn validate_public_decision_request_field(field_name: &str, value: &str) -> Result<(), String> {
	public_text::validate_public_text_field(field_name, value).map_err(|error| error.to_string())
}

fn normalize_review_checkpoint_payload(
	parsed: ReviewCheckpointArgs,
	review_policy_phase: ReviewPolicyPhase,
	status: ReviewPolicyStatus,
	head_sha: &str,
	local_repo: &LocalRepoDetails,
) -> Result<NormalizedReviewCheckpointPayload, String> {
	let reviewer = parsed
		.reviewer
		.map(|reviewer| reviewer.trim().to_owned())
		.filter(|reviewer| !reviewer.is_empty())
		.ok_or_else(|| {
			format!(
				"`{ISSUE_REVIEW_CHECKPOINT_TOOL_NAME}` requires `reviewer` set to `{INDEPENDENT_FRESH_CONTEXT_REVIEWER}`."
			)
		})?;

	if reviewer != INDEPENDENT_FRESH_CONTEXT_REVIEWER {
		return Err(format!(
			"`{ISSUE_REVIEW_CHECKPOINT_TOOL_NAME}` reviewer must be `{INDEPENDENT_FRESH_CONTEXT_REVIEWER}`, not `{reviewer}`."
		));
	}

	let review_contract = normalize_review_checkpoint_contract(
		parsed.review_contract.ok_or_else(|| {
			format!("`{ISSUE_REVIEW_CHECKPOINT_TOOL_NAME}` requires `review_contract`.")
		})?,
		review_policy_phase,
	)?;
	let review_contract_hash = review_checkpoint_contract_hash(&review_contract)?;
	let review_cost_control =
		normalize_review_cost_control(parsed.review_cost_control, &review_contract)?;
	let checks = normalize_review_checkpoint_checks(
		parsed
			.checks
			.ok_or_else(|| format!("`{ISSUE_REVIEW_CHECKPOINT_TOOL_NAME}` requires `checks`."))?,
	)?;
	let evidence = normalize_required_review_evidence_list(parsed.evidence, "evidence")?;
	let accepted_findings = parsed
		.accepted_findings
		.into_iter()
		.map(|finding| normalize_review_checkpoint_finding(finding, review_policy_phase))
		.collect::<Result<Vec<_>, _>>()?;
	let rejected_findings = parsed
		.rejected_findings
		.into_iter()
		.map(normalize_rejected_review_checkpoint_finding)
		.collect::<Result<Vec<_>, _>>()?;
	let finding_routes = normalize_review_checkpoint_finding_routes(
		parsed.finding_routes,
		&accepted_findings,
		&rejected_findings,
	)?;
	let finding_route_summary = summarize_review_checkpoint_finding_routes(&finding_routes);

	validate_review_cost_control_for_checkpoint(
		&review_cost_control,
		review_policy_phase,
		status,
		&review_contract,
		&accepted_findings,
		&finding_routes,
	)?;

	if status == ReviewPolicyStatus::Findings
		&& !current_review_blocker_routes(&finding_routes).any(|route| {
			route.finding_source == REVIEW_ROUTE_SOURCE_ACCEPTED
				&& route.finding_fingerprint.is_some()
		}) {
		return Err(String::from(
			"`issue_review_checkpoint` status `findings` requires at least one accepted finding routed as `current_blocker`. Route non-current comments through `finding_routes` and use `clean` when no current repair remains.",
		));
	}
	if status == ReviewPolicyStatus::Clean && !accepted_findings.is_empty() {
		return Err(String::from(
			"`issue_review_checkpoint` status `clean` cannot include accepted findings. Reject non-actionable comments explicitly or use status `findings` for accepted repair work.",
		));
	}
	if status == ReviewPolicyStatus::Clean
		&& finding_routes.iter().any(|route| {
			route.route == REVIEW_ROUTE_CURRENT_BLOCKER || review_route_blocks_landing(route)
		}) {
		return Err(String::from(
			"`issue_review_checkpoint` status `clean` can record only non-blocking `finding_routes` such as `follow_up`, `risk_note`, `reviewer_rubric_gap`, or `invalid_or_unsubstantiated`.",
		));
	}
	if matches!(status, ReviewPolicyStatus::Blocked | ReviewPolicyStatus::NeedsArchitectureReview)
		&& !finding_routes.iter().any(review_route_blocks_landing)
	{
		return Err(String::from(
			"`issue_review_checkpoint` status `blocked` or `needs_architecture_review` requires at least one landing-blocking `finding_routes` item with evidence, resolver, and machine-actionable next_action.",
		));
	}

	Ok(NormalizedReviewCheckpointPayload {
		reviewer,
		review_contract,
		review_contract_hash,
		review_cost_control,
		reviewed_head: ReviewCheckpointHeadBinding {
			head_sha: head_sha.to_owned(),
			head_tree_oid: local_repo.head_tree_oid.clone(),
			review_worktree_clean: local_repo.review_worktree_clean(),
		},
		checks,
		evidence,
		accepted_findings,
		rejected_findings,
		finding_routes,
		finding_route_summary,
		finding_policy: empty_review_finding_policy(review_policy_phase, status, head_sha),
	})
}

fn normalize_review_checkpoint_contract(
	contract: ReviewCheckpointContractArgs,
	review_policy_phase: ReviewPolicyPhase,
) -> Result<NormalizedReviewCheckpointContract, String> {
	let workflow_policy_source = normalize_required_review_text(
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
		normalize_required_review_text(contract.objective, "review_contract.objective")?;
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

fn normalize_review_risk_tier(risk_tier: String) -> Result<String, String> {
	let risk_tier = risk_tier.trim().to_ascii_lowercase().replace([' ', '-'], "_");

	match risk_tier.as_str() {
		"low" | "localized" | "high" => Ok(risk_tier),
		other => Err(format!(
			"`{ISSUE_REVIEW_CHECKPOINT_TOOL_NAME}` requires `review_contract.risk_tier` to be `low`, `localized`, or `high`, not `{other}`."
		)),
	}
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

fn review_checkpoint_contract_hash(
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

fn normalize_review_cost_control(
	cost_control: Option<ReviewCostControlArgs>,
	review_contract: &NormalizedReviewCheckpointContract,
) -> Result<NormalizedReviewCostControl, String> {
	let Some(cost_control) = cost_control else {
		return Ok(NormalizedReviewCostControl {
			review_class: String::from(REVIEW_CLASS_FULL_CURRENT_HEAD),
			risk_class: review_contract.risk_tier.clone(),
			compact_eligible: false,
			changed_surface_count: 0,
			changed_surface_summary: vec![String::from(
				"Review cost-control metadata was not supplied; standard full review remains required.",
			)],
			high_risk_surfaces: Vec::new(),
			current_head_evidence: false,
			validation_backed: false,
			reviewer_judgment: String::from(
				"No compact-review judgment was recorded; defaulting to full independent review.",
			),
			fallback_reason: Some(String::from(REVIEW_COST_CONTROL_NOT_PROVIDED)),
		});
	};
	let review_class = normalize_review_class(cost_control.review_class)?;
	let risk_class = normalize_review_risk_tier(cost_control.risk_class)?;
	let changed_surface_summary = normalize_review_cost_control_list(
		cost_control.changed_surface_summary,
		"review_cost_control.changed_surface_summary",
		true,
	)?;
	let high_risk_surfaces = normalize_review_cost_control_list(
		cost_control.high_risk_surfaces,
		"review_cost_control.high_risk_surfaces",
		false,
	)?;
	let reviewer_judgment = normalize_public_review_cost_control_text(
		cost_control.reviewer_judgment,
		"review_cost_control.reviewer_judgment",
	)?;
	let fallback_reason = normalize_optional_public_review_cost_control_reason(
		cost_control.fallback_reason,
		"review_cost_control.fallback_reason",
	)?;
	let compact_eligible = review_class == REVIEW_CLASS_COMPACT_CURRENT_HEAD;

	if !compact_eligible && fallback_reason.is_none() {
		return Err(String::from(
			"`issue_review_checkpoint` requires `review_cost_control.fallback_reason` when `review_class` is `full_current_head_review`.",
		));
	}

	Ok(NormalizedReviewCostControl {
		review_class,
		risk_class,
		compact_eligible,
		changed_surface_count: cost_control.changed_surface_count,
		changed_surface_summary,
		high_risk_surfaces,
		current_head_evidence: cost_control.current_head_evidence,
		validation_backed: cost_control.validation_backed,
		reviewer_judgment,
		fallback_reason,
	})
}

fn normalize_review_class(review_class: String) -> Result<String, String> {
	let review_class = review_class.trim().to_ascii_lowercase().replace([' ', '-'], "_");
	let review_class = match review_class.as_str() {
		"compact" => REVIEW_CLASS_COMPACT_CURRENT_HEAD,
		"full" | "standard" => REVIEW_CLASS_FULL_CURRENT_HEAD,
		other => other,
	};

	match review_class {
		REVIEW_CLASS_COMPACT_CURRENT_HEAD | REVIEW_CLASS_FULL_CURRENT_HEAD =>
			Ok(review_class.to_owned()),
		other => Err(format!(
			"`{ISSUE_REVIEW_CHECKPOINT_TOOL_NAME}` requires `review_cost_control.review_class` to be `{REVIEW_CLASS_COMPACT_CURRENT_HEAD}` or `{REVIEW_CLASS_FULL_CURRENT_HEAD}`, not `{other}`."
		)),
	}
}

fn normalize_review_cost_control_list(
	values: Vec<String>,
	field_name: &str,
	required: bool,
) -> Result<Vec<String>, String> {
	let values = tracker_tool_bridge::normalize_progress_list(values)
		.into_iter()
		.map(|value| normalize_public_review_cost_control_text(value, field_name))
		.collect::<Result<Vec<_>, _>>()?;

	if required && values.is_empty() {
		return Err(format!("`{ISSUE_REVIEW_CHECKPOINT_TOOL_NAME}` requires `{field_name}`."));
	}

	Ok(values)
}

fn normalize_public_review_cost_control_text(
	value: String,
	field_name: &str,
) -> Result<String, String> {
	let value = normalize_required_review_text(value, field_name)?;

	public_text::validate_public_text_field(field_name, &value)
		.map_err(|error| error.to_string())?;

	Ok(value)
}

fn normalize_optional_public_review_cost_control_reason(
	value: Option<String>,
	field_name: &str,
) -> Result<Option<String>, String> {
	let Some(value) = tracker_tool_bridge::normalize_optional_progress_field(value) else {
		return Ok(None);
	};
	let value = normalize_public_review_cost_control_text(value, field_name)?;

	Ok(Some(value))
}

fn validate_review_cost_control_for_checkpoint(
	cost_control: &NormalizedReviewCostControl,
	review_policy_phase: ReviewPolicyPhase,
	status: ReviewPolicyStatus,
	review_contract: &NormalizedReviewCheckpointContract,
	accepted_findings: &[NormalizedReviewCheckpointFinding],
	finding_routes: &[NormalizedReviewCheckpointFindingRoute],
) -> Result<(), String> {
	if cost_control.review_class == REVIEW_CLASS_FULL_CURRENT_HEAD {
		return Ok(());
	}

	let mut forced_full_reasons = compact_review_forced_full_reasons(
		cost_control,
		review_policy_phase,
		status,
		review_contract,
		accepted_findings,
		finding_routes,
	);

	if forced_full_reasons.is_empty() {
		return Ok(());
	}

	forced_full_reasons.sort();
	forced_full_reasons.dedup();

	Err(format!(
		"`issue_review_checkpoint` cannot record `review_cost_control.review_class = {REVIEW_CLASS_COMPACT_CURRENT_HEAD}` because full review is required: {}.",
		forced_full_reasons.join(", ")
	))
}

fn compact_review_forced_full_reasons(
	cost_control: &NormalizedReviewCostControl,
	review_policy_phase: ReviewPolicyPhase,
	status: ReviewPolicyStatus,
	review_contract: &NormalizedReviewCheckpointContract,
	accepted_findings: &[NormalizedReviewCheckpointFinding],
	finding_routes: &[NormalizedReviewCheckpointFindingRoute],
) -> Vec<&'static str> {
	let mut reasons = Vec::new();

	if review_policy_phase != ReviewPolicyPhase::Handoff {
		reasons.push("repair_review_phase");
	}
	if status != ReviewPolicyStatus::Clean {
		reasons.push("nonclean_review_status");
	}
	if review_contract.risk_tier != "low" {
		reasons.push("review_contract_risk_tier_not_low");
	}
	if cost_control.risk_class != "low" {
		reasons.push("review_cost_risk_class_not_low");
	}
	if cost_control.changed_surface_count == 0 {
		reasons.push("missing_changed_surface_count");
	}
	if cost_control.changed_surface_count > MAX_COMPACT_REVIEW_CHANGED_SURFACE_COUNT {
		reasons.push("changed_surface_count_exceeds_compact_limit");
	}
	if !cost_control.high_risk_surfaces.is_empty() {
		reasons.push("high_risk_surfaces_present");
	}
	if !cost_control.current_head_evidence {
		reasons.push("missing_current_head_evidence");
	}
	if !cost_control.validation_backed {
		reasons.push("missing_validation_evidence");
	}
	if !accepted_findings.is_empty() {
		reasons.push("accepted_findings_present");
	}
	if finding_routes.iter().any(|route| {
		route.route == REVIEW_ROUTE_CURRENT_BLOCKER || review_route_blocks_landing(route)
	}) {
		reasons.push("blocking_finding_routes_present");
	}

	reasons
}

fn validate_review_cost_control_policy_state(
	cost_control: &NormalizedReviewCostControl,
	policy_update: &ReviewFindingPolicyUpdate,
) -> Result<(), String> {
	if cost_control.review_class != REVIEW_CLASS_COMPACT_CURRENT_HEAD
		|| policy_update.previous_nonclean_rounds == 0
	{
		return Ok(());
	}

	Err(format!(
		"`issue_review_checkpoint` cannot record `review_cost_control.review_class = {REVIEW_CLASS_COMPACT_CURRENT_HEAD}` because full review is required: prior_nonclean_review_rounds_present."
	))
}

fn normalize_review_checkpoint_checks(
	checks: ReviewCheckpointChecksArgs,
) -> Result<ReviewCheckpointChecksArgs, String> {
	Ok(ReviewCheckpointChecksArgs {
		intended_behavior: normalize_required_review_text(
			checks.intended_behavior,
			"checks.intended_behavior",
		)?,
		regression_risk: normalize_required_review_text(
			checks.regression_risk,
			"checks.regression_risk",
		)?,
		missing_tests: normalize_required_review_text(
			checks.missing_tests,
			"checks.missing_tests",
		)?,
		docs_config_drift: normalize_required_review_text(
			checks.docs_config_drift,
			"checks.docs_config_drift",
		)?,
		migration_fallout: normalize_required_review_text(
			checks.migration_fallout,
			"checks.migration_fallout",
		)?,
		operator_facing_fallout: normalize_required_review_text(
			checks.operator_facing_fallout,
			"checks.operator_facing_fallout",
		)?,
		loop_decision_contract: normalize_required_review_text(
			checks.loop_decision_contract,
			"checks.loop_decision_contract",
		)?,
	})
}

fn normalize_review_checkpoint_finding(
	finding: ReviewCheckpointFindingArgs,
	review_policy_phase: ReviewPolicyPhase,
) -> Result<NormalizedReviewCheckpointFinding, String> {
	let severity = normalize_review_severity(finding.severity, "accepted_findings.severity")?;
	let summary = normalize_required_review_text(finding.summary, "accepted_findings.summary")?;
	let guidance = normalize_required_review_text(finding.guidance, "accepted_findings.guidance")?;
	let kind = normalize_optional_review_kind(finding.kind, "accepted_findings.kind")?
		.unwrap_or_else(|| String::from("accepted_finding"));
	let file = normalize_optional_review_file(finding.file)?;
	let line = normalize_optional_review_line(finding.line)?;
	let line_range = normalize_optional_review_line_range(
		line,
		finding.line_range,
		"accepted_findings.line_range",
	)?;
	let fingerprint = review_finding_fingerprint(
		review_policy_phase,
		&kind,
		&summary,
		&guidance,
		file.as_deref(),
		line_range.as_ref(),
	);

	Ok(NormalizedReviewCheckpointFinding {
		severity,
		summary,
		evidence: normalize_required_review_evidence_list(
			finding.evidence,
			"accepted_findings.evidence",
		)?,
		kind,
		file,
		line,
		line_range,
		guidance,
		fingerprint,
	})
}

fn normalize_rejected_review_checkpoint_finding(
	finding: ReviewCheckpointRejectedFindingArgs,
) -> Result<NormalizedRejectedReviewCheckpointFinding, String> {
	let severity = normalize_review_severity(finding.severity, "rejected_findings.severity")?;
	let summary = normalize_required_review_text(finding.summary, "rejected_findings.summary")?;
	let rejection_reason = normalize_required_review_text(
		finding.rejection_reason,
		"rejected_findings.rejection_reason",
	)?;
	let kind = normalize_optional_review_kind(finding.kind, "rejected_findings.kind")?
		.unwrap_or_else(|| String::from("rejected_finding"));
	let file = normalize_optional_review_file(finding.file)?;
	let line = normalize_optional_review_line(finding.line)?;
	let line_range = normalize_optional_review_line_range(
		line,
		finding.line_range,
		"rejected_findings.line_range",
	)?;

	Ok(NormalizedRejectedReviewCheckpointFinding {
		severity,
		summary,
		rejection_reason,
		evidence: normalize_required_review_evidence_list(
			finding.evidence,
			"rejected_findings.evidence",
		)?,
		kind,
		file,
		line,
		line_range,
	})
}

fn normalize_review_checkpoint_finding_routes(
	explicit_routes: Vec<ReviewCheckpointFindingRouteArgs>,
	accepted_findings: &[NormalizedReviewCheckpointFinding],
	rejected_findings: &[NormalizedRejectedReviewCheckpointFinding],
) -> Result<Vec<NormalizedReviewCheckpointFindingRoute>, String> {
	let mut routes = Vec::new();
	let mut explicitly_routed_accepted = BTreeSet::new();
	let mut explicitly_routed_rejected = BTreeSet::new();

	for route in explicit_routes {
		let route =
			normalize_review_checkpoint_finding_route(route, accepted_findings, rejected_findings)?;

		if route.finding_source == REVIEW_ROUTE_SOURCE_ACCEPTED
			&& let Some(index) = route.finding_index
		{
			explicitly_routed_accepted.insert(index);
		} else if route.finding_source == REVIEW_ROUTE_SOURCE_REJECTED
			&& let Some(index) = route.finding_index
		{
			explicitly_routed_rejected.insert(index);
		}

		routes.push(route);
	}
	for (index, finding) in accepted_findings.iter().enumerate() {
		let index = u64::try_from(index).map_err(|error| {
			format!("Failed to normalize accepted finding route index: {error}")
		})?;

		if !explicitly_routed_accepted.contains(&index) {
			routes.push(default_current_blocker_route(index, finding));
		}
	}
	for (index, finding) in rejected_findings.iter().enumerate() {
		let index = u64::try_from(index).map_err(|error| {
			format!("Failed to normalize rejected finding route index: {error}")
		})?;

		if !explicitly_routed_rejected.contains(&index) {
			routes.push(default_reviewer_rubric_gap_route(index, finding));
		}
	}

	Ok(routes)
}

fn normalize_review_checkpoint_finding_route(
	route: ReviewCheckpointFindingRouteArgs,
	accepted_findings: &[NormalizedReviewCheckpointFinding],
	rejected_findings: &[NormalizedRejectedReviewCheckpointFinding],
) -> Result<NormalizedReviewCheckpointFindingRoute, String> {
	let route_name = normalize_review_finding_route_name(route.route)?;
	let severity = normalize_review_severity(route.severity, "finding_routes.severity")?;
	let risk_tier = normalize_review_route_risk_tier(route.risk_tier)?;
	let summary = normalize_required_review_text(route.summary, "finding_routes.summary")?;
	let evidence =
		normalize_required_review_evidence_list(route.evidence, "finding_routes.evidence")?;
	let resolver = normalize_required_review_text(route.resolver, "finding_routes.resolver")?;
	let next_action =
		normalize_required_review_text(route.next_action, "finding_routes.next_action")?;
	let finding_source = normalize_review_route_source(route.finding_source)?;
	let (finding_index, finding_fingerprint) = normalize_review_route_binding(
		&finding_source,
		route.finding_index,
		accepted_findings,
		rejected_findings,
	)?;
	let bound_finding_high_severity = review_route_bound_finding_severity(
		&finding_source,
		finding_index,
		accepted_findings,
		rejected_findings,
	)
	.is_some_and(review_severity_blocks_invalid_route);

	if route_name == REVIEW_ROUTE_CURRENT_BLOCKER
		&& (finding_source != REVIEW_ROUTE_SOURCE_ACCEPTED || finding_fingerprint.is_none())
	{
		return Err(String::from(
			"`finding_routes.route` `current_blocker` must bind to an `accepted_findings` item with `finding_index`.",
		));
	}
	if route_name == REVIEW_ROUTE_INVALID_OR_UNSUBSTANTIATED
		&& (review_severity_blocks_invalid_route(severity.as_str())
			|| bound_finding_high_severity
			|| risk_tier == REVIEW_ROUTE_RISK_HIGH)
	{
		return Err(String::from(
			"`issue_review_checkpoint` cannot route high-severity or high-risk `finding_routes` items to `invalid_or_unsubstantiated`; use `needs_evidence` or a landing-blocking route.",
		));
	}

	Ok(NormalizedReviewCheckpointFindingRoute {
		route: route_name,
		severity,
		risk_tier,
		summary,
		evidence,
		resolver,
		next_action,
		finding_source,
		finding_index,
		finding_fingerprint,
	})
}

fn default_current_blocker_route(
	index: u64,
	finding: &NormalizedReviewCheckpointFinding,
) -> NormalizedReviewCheckpointFindingRoute {
	NormalizedReviewCheckpointFindingRoute {
		route: String::from(REVIEW_ROUTE_CURRENT_BLOCKER),
		severity: finding.severity.clone(),
		risk_tier: String::from("medium"),
		summary: finding.summary.clone(),
		evidence: finding.evidence.clone(),
		resolver: String::from("agent"),
		next_action: finding.guidance.clone(),
		finding_source: String::from(REVIEW_ROUTE_SOURCE_ACCEPTED),
		finding_index: Some(index),
		finding_fingerprint: Some(finding.fingerprint.clone()),
	}
}

fn default_reviewer_rubric_gap_route(
	index: u64,
	finding: &NormalizedRejectedReviewCheckpointFinding,
) -> NormalizedReviewCheckpointFindingRoute {
	NormalizedReviewCheckpointFindingRoute {
		route: String::from(REVIEW_ROUTE_REVIEWER_RUBRIC_GAP),
		severity: finding.severity.clone(),
		risk_tier: String::from("low"),
		summary: finding.summary.clone(),
		evidence: finding.evidence.clone(),
		resolver: String::from("reviewer"),
		next_action: finding.rejection_reason.clone(),
		finding_source: String::from(REVIEW_ROUTE_SOURCE_REJECTED),
		finding_index: Some(index),
		finding_fingerprint: None,
	}
}

fn normalize_review_finding_route_name(route: String) -> Result<String, String> {
	let route = route.trim().to_ascii_lowercase().replace([' ', '-'], "_");

	match route.as_str() {
		REVIEW_ROUTE_CURRENT_BLOCKER
		| REVIEW_ROUTE_LANDING_BLOCKER
		| REVIEW_ROUTE_CONTRACT_OR_AUTHORITY_DECISION_REQUIRED
		| REVIEW_ROUTE_NEEDS_EVIDENCE
		| REVIEW_ROUTE_FOLLOW_UP
		| REVIEW_ROUTE_DETERMINISTIC_GATE_CANDIDATE
		| REVIEW_ROUTE_ARCHITECTURE_SIGNAL
		| REVIEW_ROUTE_ISSUE_CONTRACT_GAP
		| REVIEW_ROUTE_REVIEWER_RUBRIC_GAP
		| REVIEW_ROUTE_RISK_NOTE
		| REVIEW_ROUTE_INVALID_OR_UNSUBSTANTIATED => Ok(route),
		other => Err(format!(
			"`finding_routes.route` must be one of the supported Decodex Review route taxonomy values, not `{other}`."
		)),
	}
}

fn normalize_review_route_risk_tier(risk_tier: Option<String>) -> Result<String, String> {
	let Some(risk_tier) = tracker_tool_bridge::normalize_optional_progress_field(risk_tier) else {
		return Ok(String::from("low"));
	};
	let risk_tier = risk_tier.to_ascii_lowercase().replace([' ', '-'], "_");

	match risk_tier.as_str() {
		"low" | "medium" | REVIEW_ROUTE_RISK_HIGH => Ok(risk_tier),
		other => Err(format!(
			"`finding_routes.risk_tier` must be `low`, `medium`, or `high`, not `{other}`."
		)),
	}
}

fn normalize_review_route_source(source: Option<String>) -> Result<String, String> {
	let Some(source) = tracker_tool_bridge::normalize_optional_progress_field(source) else {
		return Ok(String::from(REVIEW_ROUTE_SOURCE_ROUTE_ONLY));
	};
	let source = source.to_ascii_lowercase().replace([' ', '-'], "_");

	match source.as_str() {
		REVIEW_ROUTE_SOURCE_ACCEPTED
		| REVIEW_ROUTE_SOURCE_REJECTED
		| REVIEW_ROUTE_SOURCE_ROUTE_ONLY => Ok(source),
		other => Err(format!(
			"`finding_routes.finding_source` must be `accepted_findings`, `rejected_findings`, or `route_only`, not `{other}`."
		)),
	}
}

fn normalize_review_route_binding(
	source: &str,
	index: Option<u64>,
	accepted_findings: &[NormalizedReviewCheckpointFinding],
	rejected_findings: &[NormalizedRejectedReviewCheckpointFinding],
) -> Result<(Option<u64>, Option<String>), String> {
	match source {
		REVIEW_ROUTE_SOURCE_ACCEPTED => {
			let index = index.ok_or_else(|| {
				String::from(
					"`finding_routes.finding_index` is required when `finding_source` is `accepted_findings`.",
				)
			})?;
			let finding = accepted_findings
				.get(usize::try_from(index).map_err(|error| {
					format!("Failed to normalize accepted finding route index: {error}")
				})?)
				.ok_or_else(|| {
					format!(
						"`finding_routes.finding_index` `{index}` does not match any accepted finding."
					)
				})?;

			Ok((Some(index), Some(finding.fingerprint.clone())))
		},
		REVIEW_ROUTE_SOURCE_REJECTED => {
			let index = index.ok_or_else(|| {
				String::from(
					"`finding_routes.finding_index` is required when `finding_source` is `rejected_findings`.",
				)
			})?;

			rejected_findings
				.get(usize::try_from(index).map_err(|error| {
					format!("Failed to normalize rejected finding route index: {error}")
				})?)
				.ok_or_else(|| {
					format!(
						"`finding_routes.finding_index` `{index}` does not match any rejected finding."
					)
				})?;

			Ok((Some(index), None))
		},
		REVIEW_ROUTE_SOURCE_ROUTE_ONLY => {
			if index.is_some() {
				return Err(String::from(
					"`finding_routes.finding_index` is only valid with `accepted_findings` or `rejected_findings` sources.",
				));
			}

			Ok((None, None))
		},
		_ => Err(String::from(
			"`finding_routes.finding_source` did not normalize to a supported source.",
		)),
	}
}

fn review_route_bound_finding_severity<'a>(
	source: &str,
	index: Option<u64>,
	accepted_findings: &'a [NormalizedReviewCheckpointFinding],
	rejected_findings: &'a [NormalizedRejectedReviewCheckpointFinding],
) -> Option<&'a str> {
	let index = usize::try_from(index?).ok()?;

	match source {
		REVIEW_ROUTE_SOURCE_ACCEPTED =>
			accepted_findings.get(index).map(|finding| finding.severity.as_str()),
		REVIEW_ROUTE_SOURCE_REJECTED =>
			rejected_findings.get(index).map(|finding| finding.severity.as_str()),
		_ => None,
	}
}

fn review_severity_blocks_invalid_route(severity: &str) -> bool {
	matches!(severity, "critical" | "high")
}

fn summarize_review_checkpoint_finding_routes(
	routes: &[NormalizedReviewCheckpointFindingRoute],
) -> ReviewCheckpointFindingRouteSummary {
	let mut counts = BTreeMap::<String, usize>::new();

	for route in routes {
		*counts.entry(route.route.clone()).or_default() += 1;
	}

	ReviewCheckpointFindingRouteSummary {
		route_counts: counts
			.into_iter()
			.map(|(route, count)| ReviewCheckpointFindingRouteCount { route, count })
			.collect(),
		next_action: review_route_next_action(routes),
	}
}

fn review_route_next_action(routes: &[NormalizedReviewCheckpointFindingRoute]) -> Option<String> {
	routes
		.iter()
		.min_by_key(|route| review_route_priority(&route.route))
		.map(|route| route.next_action.clone())
}

fn review_route_priority(route: &str) -> u8 {
	match route {
		REVIEW_ROUTE_CURRENT_BLOCKER => 0,
		REVIEW_ROUTE_LANDING_BLOCKER => 1,
		REVIEW_ROUTE_CONTRACT_OR_AUTHORITY_DECISION_REQUIRED => 2,
		REVIEW_ROUTE_NEEDS_EVIDENCE => 3,
		REVIEW_ROUTE_DETERMINISTIC_GATE_CANDIDATE => 4,
		REVIEW_ROUTE_ARCHITECTURE_SIGNAL => 5,
		REVIEW_ROUTE_ISSUE_CONTRACT_GAP => 6,
		REVIEW_ROUTE_FOLLOW_UP => 7,
		REVIEW_ROUTE_RISK_NOTE => 8,
		REVIEW_ROUTE_REVIEWER_RUBRIC_GAP => 9,
		REVIEW_ROUTE_INVALID_OR_UNSUBSTANTIATED => 10,
		_ => u8::MAX,
	}
}

fn current_review_blocker_routes(
	routes: &[NormalizedReviewCheckpointFindingRoute],
) -> impl Iterator<Item = &NormalizedReviewCheckpointFindingRoute> {
	routes.iter().filter(|route| route.route == REVIEW_ROUTE_CURRENT_BLOCKER)
}

fn current_review_blocker_findings(
	payload: &NormalizedReviewCheckpointPayload,
) -> impl Iterator<Item = &NormalizedReviewCheckpointFinding> {
	let fingerprints = current_review_blocker_routes(&payload.finding_routes)
		.filter_map(|route| route.finding_fingerprint.clone())
		.collect::<BTreeSet<_>>();

	payload
		.accepted_findings
		.iter()
		.filter(move |finding| fingerprints.contains(&finding.fingerprint))
}

fn review_route_blocks_landing(route: &NormalizedReviewCheckpointFindingRoute) -> bool {
	matches!(
		route.route.as_str(),
		REVIEW_ROUTE_LANDING_BLOCKER
			| REVIEW_ROUTE_CONTRACT_OR_AUTHORITY_DECISION_REQUIRED
			| REVIEW_ROUTE_NEEDS_EVIDENCE
			| REVIEW_ROUTE_DETERMINISTIC_GATE_CANDIDATE
			| REVIEW_ROUTE_ARCHITECTURE_SIGNAL
			| REVIEW_ROUTE_ISSUE_CONTRACT_GAP
	)
}

fn normalize_review_severity(severity: String, field_name: &str) -> Result<String, String> {
	let severity = severity.trim().to_ascii_lowercase();

	match severity.as_str() {
		"critical" | "high" | "medium" | "low" | "info" => Ok(severity),
		other => Err(format!(
			"`{field_name}` must be `critical`, `high`, `medium`, `low`, or `info`, not `{other}`."
		)),
	}
}

fn normalize_required_review_text(value: String, field_name: &str) -> Result<String, String> {
	let value = tracker_tool_bridge::normalize_summary(&value);

	if value.is_empty() {
		return Err(format!("`{ISSUE_REVIEW_CHECKPOINT_TOOL_NAME}` requires `{field_name}`."));
	}

	Ok(value)
}

fn normalize_required_review_evidence_list(
	values: Vec<String>,
	field_name: &str,
) -> Result<Vec<String>, String> {
	let values = tracker_tool_bridge::normalize_progress_list(values);

	if values.is_empty() {
		return Err(format!("`{ISSUE_REVIEW_CHECKPOINT_TOOL_NAME}` requires `{field_name}`."));
	}

	Ok(values)
}

fn normalize_optional_review_file(value: Option<String>) -> Result<Option<String>, String> {
	let Some(file) = tracker_tool_bridge::normalize_optional_progress_field(value) else {
		return Ok(None);
	};

	if file.starts_with('/') {
		return Err(String::from(
			"`issue_review_checkpoint` file references must be repository-relative paths.",
		));
	}

	Ok(Some(file))
}

fn normalize_optional_review_line(value: Option<u64>) -> Result<Option<u64>, String> {
	if matches!(value, Some(0)) {
		return Err(String::from(
			"`issue_review_checkpoint` line references must be one-based when supplied.",
		));
	}

	Ok(value)
}

fn normalize_optional_review_line_range(
	line: Option<u64>,
	line_range: Option<ReviewCheckpointLineRangeArgs>,
	field_name: &str,
) -> Result<Option<ReviewCheckpointLineRangeArgs>, String> {
	let Some(line_range) = line_range
		.or_else(|| line.map(|line| ReviewCheckpointLineRangeArgs { start: line, end: line }))
	else {
		return Ok(None);
	};

	if line_range.start == 0 || line_range.end == 0 {
		return Err(format!(
			"`{ISSUE_REVIEW_CHECKPOINT_TOOL_NAME}` requires `{field_name}` to use one-based line numbers."
		));
	}
	if line_range.end < line_range.start {
		return Err(format!(
			"`{ISSUE_REVIEW_CHECKPOINT_TOOL_NAME}` requires `{field_name}.end` to be greater than or equal to `{field_name}.start`."
		));
	}

	if let Some(line) = line
		&& (line < line_range.start || line > line_range.end)
	{
		return Err(format!(
			"`{ISSUE_REVIEW_CHECKPOINT_TOOL_NAME}` requires `line` to fall inside `{field_name}` when both are supplied."
		));
	}

	Ok(Some(line_range))
}

fn normalize_optional_review_kind(
	value: Option<String>,
	field_name: &str,
) -> Result<Option<String>, String> {
	let Some(kind) = tracker_tool_bridge::normalize_optional_progress_field(value) else {
		return Ok(None);
	};
	let kind = kind.to_ascii_lowercase().replace([' ', '-'], "_");
	let mut chars = kind.chars();
	let Some(first) = chars.next() else {
		return Ok(None);
	};

	if !first.is_ascii_lowercase()
		|| !chars.all(|character| {
			character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_'
		}) {
		return Err(format!(
			"`{ISSUE_REVIEW_CHECKPOINT_TOOL_NAME}` requires `{field_name}` to be a public snake_case identifier."
		));
	}

	Ok(Some(kind))
}

fn empty_review_finding_policy(
	review_policy_phase: ReviewPolicyPhase,
	status: ReviewPolicyStatus,
	head_sha: &str,
) -> ReviewFindingPolicyState {
	ReviewFindingPolicyState {
		schema: String::from("decodex.review_finding_policy/1"),
		phase: review_policy_phase.as_str().to_owned(),
		status: status.as_str().to_owned(),
		head_sha: head_sha.to_owned(),
		nonclean_rounds: 0,
		active_fingerprints: Vec::new(),
		stop_fingerprint: None,
		findings: Vec::new(),
	}
}

fn review_finding_fingerprint(
	review_policy_phase: ReviewPolicyPhase,
	kind: &str,
	title: &str,
	body: &str,
	file: Option<&str>,
	line_range: Option<&ReviewCheckpointLineRangeArgs>,
) -> String {
	let line_range = line_range
		.map_or_else(|| String::from("none"), |range| format!("{}-{}", range.start, range.end));
	let input = [
		("phase", review_policy_phase.as_str()),
		("kind", kind),
		("title", title),
		("body", body),
		("file", file.unwrap_or("none")),
		("line_range", line_range.as_str()),
	]
	.into_iter()
	.map(|(key, value)| format!("{key}={value}"))
	.collect::<Vec<_>>()
	.join("\n");
	let digest = Sha256::digest(input.as_bytes());
	let hash = digest.iter().map(|byte| format!("{byte:02x}")).collect::<String>();

	format!("review_finding:{hash}")
}

fn review_finding_policy_update(
	previous: ReviewFindingPolicyState,
	previous_nonclean_rounds: i64,
	review_policy_phase: ReviewPolicyPhase,
	status: ReviewPolicyStatus,
	head_sha: &str,
	checkpoint_payload: &NormalizedReviewCheckpointPayload,
) -> ReviewFindingPolicyUpdate {
	let active_fingerprints = checkpoint_payload
		.finding_routes
		.iter()
		.filter(|route| route.route == REVIEW_ROUTE_CURRENT_BLOCKER)
		.filter_map(|route| route.finding_fingerprint.clone())
		.collect::<BTreeSet<_>>();
	let current_blocker_findings = current_review_blocker_findings(checkpoint_payload)
		.map(|finding| finding.fingerprint.clone())
		.collect::<BTreeSet<_>>();
	let mut records = previous
		.findings
		.into_iter()
		.map(|record| (record.fingerprint.clone(), record))
		.collect::<BTreeMap<_, _>>();

	match status {
		ReviewPolicyStatus::Findings => {
			for finding in current_review_blocker_findings(checkpoint_payload) {
				upsert_open_review_finding_record(
					&mut records,
					finding,
					head_sha,
					&checkpoint_payload.evidence,
				);
			}

			resolve_absent_review_findings(&mut records, &active_fingerprints);
		},
		ReviewPolicyStatus::Clean => {
			resolve_all_review_findings(&mut records, &checkpoint_payload.evidence);
		},
		ReviewPolicyStatus::NeedsArchitectureReview | ReviewPolicyStatus::Blocked => {},
	}

	let nonclean_rounds = if status == ReviewPolicyStatus::Findings {
		current_blocker_findings
			.iter()
			.filter_map(|fingerprint| records.get(fingerprint))
			.map(|record| record.repeat_count)
			.max()
			.unwrap_or_default()
	} else {
		0
	};
	let stop_fingerprint = current_blocker_findings
		.iter()
		.filter_map(|fingerprint| records.get(fingerprint).map(|record| (fingerprint, record)))
		.filter(|(_fingerprint, record)| record.repeat_count >= REVIEW_POLICY_CONVERGENCE_BUDGET)
		.max_by_key(|(_fingerprint, record)| record.repeat_count)
		.map(|(fingerprint, _record)| fingerprint.clone());
	let mut finding_policy = empty_review_finding_policy(review_policy_phase, status, head_sha);

	finding_policy.nonclean_rounds = nonclean_rounds;
	finding_policy.active_fingerprints = active_fingerprints.into_iter().collect();
	finding_policy.stop_fingerprint = stop_fingerprint;
	finding_policy.findings = records.into_values().collect();

	ReviewFindingPolicyUpdate { nonclean_rounds, previous_nonclean_rounds, finding_policy }
}

fn upsert_open_review_finding_record(
	records: &mut BTreeMap<String, ReviewFindingPolicyRecord>,
	finding: &NormalizedReviewCheckpointFinding,
	head_sha: &str,
	checkpoint_evidence: &[String],
) {
	let existing_open =
		records.get(&finding.fingerprint).is_some_and(|record| record.status == "open");
	let mut record = records
		.remove(&finding.fingerprint)
		.unwrap_or_else(|| review_finding_policy_record(finding, head_sha));

	record.kind = finding.kind.clone();
	record.title = finding.summary.clone();
	record.body = finding.guidance.clone();
	record.file = finding.file.clone();
	record.line_range = finding.line_range.clone();

	if existing_open {
		record.repeat_count = record.repeat_count.saturating_add(1);
	} else {
		record.first_seen_head = head_sha.to_owned();
		record.repeat_count = 1;
	}

	record.last_seen_head = head_sha.to_owned();
	record.status = String::from("open");

	append_review_finding_repair_evidence(&mut record, checkpoint_evidence);
	append_review_finding_repair_evidence(&mut record, &finding.evidence);

	records.insert(finding.fingerprint.clone(), record);
}

fn review_finding_policy_record(
	finding: &NormalizedReviewCheckpointFinding,
	head_sha: &str,
) -> ReviewFindingPolicyRecord {
	ReviewFindingPolicyRecord {
		fingerprint: finding.fingerprint.clone(),
		kind: finding.kind.clone(),
		title: finding.summary.clone(),
		body: finding.guidance.clone(),
		file: finding.file.clone(),
		line_range: finding.line_range.clone(),
		first_seen_head: head_sha.to_owned(),
		last_seen_head: head_sha.to_owned(),
		status: String::from("open"),
		repeat_count: 0,
		repair_evidence: Vec::new(),
	}
}

fn append_review_finding_repair_evidence(
	record: &mut ReviewFindingPolicyRecord,
	evidence: &[String],
) {
	for item in evidence {
		if !record.repair_evidence.iter().any(|existing| existing == item) {
			record.repair_evidence.push(item.clone());
		}
	}
}

fn resolve_absent_review_findings(
	records: &mut BTreeMap<String, ReviewFindingPolicyRecord>,
	active_fingerprints: &BTreeSet<String>,
) {
	for (fingerprint, record) in records {
		if record.status == "open" && !active_fingerprints.contains(fingerprint) {
			record.status = String::from("resolved");
		}
	}
}

fn resolve_all_review_findings(
	records: &mut BTreeMap<String, ReviewFindingPolicyRecord>,
	checkpoint_evidence: &[String],
) {
	for record in records.values_mut().filter(|record| record.status == "open") {
		record.status = String::from("resolved");

		append_review_finding_repair_evidence(record, checkpoint_evidence);
	}
}

fn review_finding_policy_from_previous_state(
	previous_state: &ReviewPolicyState,
	review_policy_phase: ReviewPolicyPhase,
) -> Option<ReviewFindingPolicyState> {
	if previous_state.phase != review_policy_phase {
		return None;
	}

	let details = serde_json::from_str::<Value>(&previous_state.details_json).ok()?;

	details
		.get("finding_policy")
		.cloned()
		.and_then(|value| serde_json::from_value::<ReviewFindingPolicyState>(value).ok())
		.or_else(|| migrate_legacy_review_finding_policy(previous_state, &details))
}

fn migrate_legacy_review_finding_policy(
	previous_state: &ReviewPolicyState,
	details: &Value,
) -> Option<ReviewFindingPolicyState> {
	let mut finding_policy = empty_review_finding_policy(
		previous_state.phase,
		previous_state.status,
		&previous_state.head_sha,
	);

	if previous_state.status != ReviewPolicyStatus::Findings {
		return Some(finding_policy);
	}

	let findings = details.get("accepted_findings")?.as_array()?;

	for finding_value in findings {
		let finding = serde_json::from_value::<ReviewCheckpointFindingArgs>(finding_value.clone())
			.ok()
			.and_then(|finding| {
				normalize_review_checkpoint_finding(finding, previous_state.phase).ok()
			})?;
		let mut record = review_finding_policy_record(&finding, &previous_state.head_sha);

		record.repeat_count = previous_state.nonclean_rounds.max(1);

		append_review_finding_repair_evidence(&mut record, &finding.evidence);

		finding_policy.active_fingerprints.push(finding.fingerprint.clone());
		finding_policy.findings.push(record);
	}

	finding_policy.nonclean_rounds = previous_state.nonclean_rounds;

	finding_policy.active_fingerprints.sort();
	finding_policy.active_fingerprints.dedup();

	finding_policy.stop_fingerprint = (previous_state.nonclean_rounds
		>= REVIEW_POLICY_CONVERGENCE_BUDGET)
		.then(|| finding_policy.active_fingerprints.first().cloned())
		.flatten();

	Some(finding_policy)
}

fn validate_public_error_class(error_class: &str) -> Result<(), String> {
	let mut chars = error_class.chars();
	let Some(first) = chars.next() else {
		return Err(String::from("`error_class` must be a public snake_case identifier."));
	};

	if !first.is_ascii_lowercase()
		|| !chars.all(|character| {
			character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_'
		}) {
		return Err(String::from("`error_class` must be a public snake_case identifier."));
	}

	Ok(())
}

fn validate_manual_attention_error_class(error_class: &str) -> Result<(), String> {
	validate_public_error_class(error_class)?;

	if is_runtime_owned_manual_attention_error_class(error_class) {
		return Err(format!(
			"`{ISSUE_COMMENT_TOOL_NAME}` kind `{COMMENT_KIND_MANUAL_ATTENTION}` cannot use runtime-owned error class `{error_class}`; keep repairing, retrying, or letting Decodex retain the lane, and use a human-owned blocker class only when automation cannot clear the blocker."
		));
	}

	Ok(())
}

fn is_runtime_owned_manual_attention_error_class(error_class: &str) -> bool {
	matches!(
		error_class,
		"retryable_execution_failure"
			| "repo_gate_canonicalize_failed"
			| "repo_gate_verify_failed"
			| "repo_gate_baseline_failed"
			| "repo_gate_preexisting_baseline_failed"
			| "repo_gate_global_baseline_failed"
			| "repo_gate_tracked_rewrites_left"
			| "repo_gate_git_lock_contention"
			| "stalled_run_detected"
			| "app_server_zero_evidence_start_failed"
			| "app_server_plugin_list_timeout"
			| "app_server_preflight_timeout"
			| "app_server_transport_disconnected"
			| "phase_goal_terminal_path_missing"
			| "app_server_dynamic_tool_protocol_failure"
			| "app_server_dynamic_tool_failed"
			| "app_server_turn_failed"
			| "app_server_usage_limit_exceeded"
	) || runtime_owned_baseline_error_class(error_class)
}

fn runtime_owned_baseline_error_class(error_class: &str) -> bool {
	[
		"baseline",
		"preexisting",
		"pre_existing",
		"repo_wide",
		"repository_wide",
		"global_baseline",
		"docs_okf",
	]
	.iter()
	.any(|pattern| error_class.contains(pattern))
}

fn format_manual_attention_comment(
	review_context: &ReviewHandoffContext,
	comment: &NormalizedManualAttentionComment,
) -> String {
	let mut lines = vec![
		String::from("decodex run needs manual attention"),
		String::new(),
		format!("- run_id: `{}`", review_context.run_id),
		format!("- attempt: `{}`", review_context.attempt_number),
		format!("- reported_at: `{}`", tracker_tool_bridge::current_timestamp()),
		format!("- branch: `{}`", review_context.branch_name),
		format!("- worktree_path: `{}`", review_context.worktree_path),
		format!("- comment_kind: `{COMMENT_KIND_MANUAL_ATTENTION}`"),
		format!("- error_class: `{}`", comment.error_class),
		format!("- next_action: {}", comment.next_action),
	];

	if let Some(summary) = comment.summary.as_deref() {
		lines.push(format!("- summary: {summary}"));
	}

	for blocker in &comment.blockers {
		lines.push(format!("- blocker: {blocker}"));
	}
	for evidence in &comment.evidence {
		lines.push(format!("- evidence: {evidence}"));
	}

	if let Some(request) = comment.decision_request.as_ref() {
		lines.push(String::from("- decision_request: authority_boundary"));
		lines.push(format!("- decision_request_id: `{}`", request.decision_request_id));
		lines.push(format!("- decision_reason: `{}`", request.reason_code));
		lines.push(format!("- boundary: `{}`", request.boundary_type));
		lines.push(format!("- proposed_change: {}", request.proposed_change));
		lines.push(format!("- why_exceeds_authority: {}", request.why_exceeds_authority));

		for option in &request.options {
			lines.push(format!("- decision_option: `{}` - {}", option.label, option.description));
		}

		lines.push(format!("- recommendation: {}", request.recommendation));
		lines.push(format!("- resume_condition: {}", request.resume_condition));
	}
	if let Some(failed_command) = comment.failed_command.as_deref() {
		lines.push(format!("- failed_command: {failed_command}"));
	}
	if let Some(raw_error) = comment.raw_error.as_deref() {
		lines.push(format!("- raw_error: {raw_error}"));
	}

	lines.join("\n")
}
