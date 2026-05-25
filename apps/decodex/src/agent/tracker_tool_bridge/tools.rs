use serde_json::{self, Value};

use crate::{
	agent::tracker_tool_bridge::{
		self, CommentArgs, DynamicToolCallResponse, DynamicToolSpec, ExecutionProgressPhase,
		ISSUE_COMMENT_TOOL_NAME, ISSUE_DELIVERY_CLOSEOUT_COMPLETE_TOOL_NAME,
		ISSUE_LABEL_ADD_TOOL_NAME, ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME,
		ISSUE_REVIEW_CHECKPOINT_TOOL_NAME, ISSUE_REVIEW_HANDOFF_TOOL_NAME,
		ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME, ISSUE_TERMINAL_FINALIZE_TOOL_NAME,
		ISSUE_TRANSITION_TOOL_NAME, LabelArgs, NormalizedProgressCheckpoint, PendingReviewAction,
		PendingReviewCompletion, ProgressCheckpointArgs, ReviewCheckpointArgs, ReviewExecutionMode,
		ReviewHandoffArgs, ReviewHandoffContext, ReviewPolicyPhase, ReviewPolicyStatus,
		RunCompletionDisposition, TerminalFinalizeArgs, TrackerToolBridge, TransitionArgs,
	},
	state::{self, StateStore},
	tracker::{
		self, records,
		records::{LinearExecutionEventIdentity, LinearExecutionEventRecord},
	},
};

const COMMENT_KIND_MANUAL_ATTENTION: &str = "manual_attention";
const MANUAL_ATTENTION_TERMINAL_PATH: &str = "manual_attention";

#[derive(Debug)]
struct NormalizedManualAttentionComment {
	error_class: String,
	next_action: String,
	blockers: Vec<String>,
	evidence: Vec<String>,
	failed_command: Option<String>,
	raw_error: Option<String>,
	summary: Option<String>,
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
					.is_some_and(ReviewHandoffContext::internal_review_checkpoint_enabled)
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
					.is_some_and(ReviewHandoffContext::internal_review_checkpoint_enabled)
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
			"Add an allowlisted public summary comment to the currently leased issue. The supported automation kind is `manual_attention`; Decodex renders the Linear comment from structured public fields.",
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
					"summary": { "type": "string" }
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
				"required": ["phase", "focus", "next_action", "blockers", "evidence"],
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
			"Record the current repo-native bounded-review result for the leased issue so Decodex can decide whether the lane may continue or must stop for human intervention. `head_sha` must resolve to the current lane HEAD.",
			serde_json::json!({
				"type": "object",
				"properties": {
					"issue_id": { "type": "string" },
					"issue_identifier": { "type": "string" },
					"status": {
						"type": "string",
						"enum": ["clean", "findings", "needs_architecture_review", "blocked"]
					},
					"head_sha": { "type": "string" },
					"evidence": {
						"type": "array",
						"items": { "type": "string" }
					}
				},
				"required": ["status", "head_sha", "evidence"],
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
			"Add an allowed workflow label to the currently leased issue.",
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
			Err(error) =>
				return Err(format!(
					"Failed to record an execution-state checkpoint for issue `{}`: {error}",
					self.issue.identifier
				)),
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

		validate_public_error_class(&error_class)?;

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
		})
	}

	fn manual_attention_execution_event(
		&self,
		review_context: &ReviewHandoffContext,
		comment: &NormalizedManualAttentionComment,
	) -> LinearExecutionEventRecord {
		let anchor = records::stable_event_anchor(&[
			COMMENT_KIND_MANUAL_ATTENTION,
			comment.error_class.as_str(),
			comment.next_action.as_str(),
			comment.failed_command.as_deref().unwrap_or_default(),
			comment.raw_error.as_deref().unwrap_or_default(),
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

		if !review_context.internal_review_checkpoint_enabled() {
			return DynamicToolCallResponse::failure(format!(
				"`issue_review_checkpoint` is disabled because `codex.internal_review_mode = \"{}\"` for this run.",
				review_context.internal_review_mode.as_str()
			));
		}

		let Some(review_policy_phase) = ReviewPolicyPhase::for_mode(review_context.mode) else {
			return DynamicToolCallResponse::failure(String::from(
				"`issue_review_checkpoint` is unavailable for retained closeout runs.",
			));
		};
		let review_policy_status = match ReviewPolicyStatus::parse(&parsed.status) {
			Ok(status) => status,
			Err(error) => return DynamicToolCallResponse::failure(error),
		};
		let local_repo = match self.current_local_repo_details(review_context) {
			Ok(local_repo) => local_repo,
			Err(error) => return DynamicToolCallResponse::failure(error),
		};
		let head_sha = match self.canonicalize_current_lane_head_sha(
			ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
			parsed.head_sha.as_str(),
			&local_repo.head_oid,
		) {
			Ok(head_sha) => head_sha,
			Err(error) => return DynamicToolCallResponse::failure(error),
		};
		let evidence = parsed
			.evidence
			.into_iter()
			.map(|item| item.trim().to_owned())
			.filter(|item| !item.is_empty())
			.collect::<Vec<_>>();
		let nonclean_rounds = match self.review_checkpoint_nonclean_rounds(
			review_context,
			review_policy_phase,
			review_policy_status,
		) {
			Ok(nonclean_rounds) => nonclean_rounds,
			Err(error) => return DynamicToolCallResponse::failure(error),
		};

		self.cache_review_policy_state_best_effort(
			review_context,
			review_policy_phase,
			review_policy_status,
			&head_sha,
			nonclean_rounds,
		);

		let message = self.review_checkpoint_success_message(
			review_policy_phase,
			review_policy_status,
			&head_sha,
			nonclean_rounds,
			evidence.len(),
		);

		DynamicToolCallResponse::success(message)
	}

	fn review_checkpoint_nonclean_rounds(
		&self,
		review_context: &ReviewHandoffContext,
		review_policy_phase: ReviewPolicyPhase,
		review_policy_status: ReviewPolicyStatus,
	) -> Result<i64, String> {
		let previous_state = self
			.review_policy_state_for_current_phase(review_context)
			.map_err(|error| error.to_string())?;
		let phase_changed = previous_state
			.as_ref()
			.is_some_and(|previous_state| previous_state.phase != review_policy_phase);
		let previous_nonclean_rounds = if phase_changed {
			0
		} else {
			previous_state.as_ref().map_or(0, |previous_state| previous_state.nonclean_rounds)
		};

		Ok(match review_policy_status {
			ReviewPolicyStatus::Findings => previous_nonclean_rounds.saturating_add(1),
			ReviewPolicyStatus::Clean
			| ReviewPolicyStatus::NeedsArchitectureReview
			| ReviewPolicyStatus::Blocked => 0,
		})
	}

	fn review_checkpoint_success_message(
		&self,
		review_policy_phase: ReviewPolicyPhase,
		review_policy_status: ReviewPolicyStatus,
		head_sha: &str,
		nonclean_rounds: i64,
		evidence_count: usize,
	) -> String {
		let evidence_suffix = if evidence_count == 0 {
			String::from("no evidence items recorded")
		} else {
			format!("{evidence_count} evidence item(s) recorded")
		};

		match review_policy_status {
			ReviewPolicyStatus::Clean => format!(
				"Recorded a clean `{}` review checkpoint for issue `{}` at HEAD `{head_sha}`; {evidence_suffix}.",
				review_policy_phase.as_str(),
				self.issue.identifier,
			),
			ReviewPolicyStatus::Findings => format!(
				"Recorded `{}` review findings for issue `{}` at HEAD `{head_sha}`; consecutive non-clean rounds now `{nonclean_rounds}`; {evidence_suffix}.",
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

	fn clear_review_policy_state_after_completion(
		&self,
		review_context: &ReviewHandoffContext,
		tool_name: &str,
	) -> Result<(), String> {
		match state::clear_run_review_policy_state(&review_context.cwd) {
			Ok(()) => Ok(()),
			Err(error) if review_context.internal_review_checkpoint_enabled() => Err(format!(
				"Failed to clear review policy state for issue `{}` after recording `{tool_name}`: {error}",
				self.issue.identifier
			)),
			Err(error) => {
				tracing::warn!(
					?error,
					issue = self.issue.identifier,
					run_id = review_context.run_id,
					tool_name,
					worktree_path = %review_context.cwd.display(),
					"Review policy state clear failed while internal review is disabled; continuing."
				);

				Ok(())
			},
		}
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
		let manual_attention_label =
			parsed.label == self.workflow.frontmatter().tracker().needs_attention_label();
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

		self.finalized_completion_path.replace(Some(actual_path));

		DynamicToolCallResponse::success(format!(
			"Finalized terminal path `{}` for issue `{}`. You can only finish the turn after this succeeds.",
			actual_path.as_str(),
			self.issue.identifier
		))
	}
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

	if let Some(failed_command) = comment.failed_command.as_deref() {
		lines.push(format!("- failed_command: {failed_command}"));
	}
	if let Some(raw_error) = comment.raw_error.as_deref() {
		lines.push(format!("- raw_error: {raw_error}"));
	}

	lines.join("\n")
}
