mod manual_attention;
mod progress_checkpoint;
mod review_checkpoint;
mod review_checkpoint_flow;
mod tool_specs;

use self::review_checkpoint::{
	ReviewFindingPolicyUpdate, current_review_blocker_findings, non_empty_string_array_schema,
	normalize_review_checkpoint_payload, review_checkpoint_checks_schema,
	review_checkpoint_contract_schema, review_checkpoint_finding_routes_schema,
	review_checkpoint_findings_array_schema, review_checkpoint_reviewer_schema,
	review_checkpoint_status_schema, review_cost_control_schema,
	review_finding_policy_from_previous_state, review_finding_policy_update,
	validate_review_cost_control_policy_state,
};
use crate::{
	agent::tracker_tool_bridge::{
		self, AuthorityDecisionOptionArgs, AuthorityDecisionRequestArgs, DocsImpact,
		DynamicToolCallResponse, DynamicToolSpec, ExecutionProgressPhase, ISSUE_COMMENT_TOOL_NAME,
		ISSUE_DELIVERY_CLOSEOUT_COMPLETE_TOOL_NAME, ISSUE_LABEL_ADD_TOOL_NAME,
		ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME, ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
		ISSUE_REVIEW_HANDOFF_TOOL_NAME, ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME,
		ISSUE_TERMINAL_FINALIZE_TOOL_NAME, ISSUE_TRANSITION_TOOL_NAME, LabelArgs, LocalRepoDetails,
		NormalizedProgressCheckpoint, NormalizedReviewCheckpointPayload, PendingReviewAction,
		PendingReviewCompletion, ProgressCheckpointArgs, PullRequestDetails,
		REVIEW_POLICY_CONVERGENCE_BUDGET, ReviewCheckpointArgs, ReviewExecutionMode,
		ReviewHandoffArgs, ReviewHandoffContext, ReviewPolicyPhase, ReviewPolicyStatus,
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
use serde_json::{self, Value};

pub(super) const REVIEW_COMPLETION_INTENT_EVENT_TYPE: &str = "review_completion_intent";

const COMMENT_KIND_MANUAL_ATTENTION: &str = "manual_attention";
const MANUAL_ATTENTION_TERMINAL_PATH: &str = "manual_attention";
const TERMINAL_FINALIZE_EVENT_TYPE: &str = "terminal_finalize";

impl<'a> TrackerToolBridge<'a> {
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

		if actual_path == RunCompletionDisposition::ReviewHandoff
			&& let Err(error) = self.persist_terminal_review_handoff_marker(review_context)
		{
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
