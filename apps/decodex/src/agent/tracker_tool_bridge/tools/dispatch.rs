use serde_json::Value;

use crate::{
	agent::tracker_tool_bridge::{
		DynamicToolCallResponse, ISSUE_COMMENT_TOOL_NAME,
		ISSUE_DELIVERY_CLOSEOUT_COMPLETE_TOOL_NAME, ISSUE_LABEL_ADD_TOOL_NAME,
		ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME, ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
		ISSUE_REVIEW_HANDOFF_TOOL_NAME, ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME,
		ISSUE_TERMINAL_FINALIZE_TOOL_NAME, ISSUE_TRANSITION_TOOL_NAME, LabelArgs,
		TrackerToolBridge, TransitionArgs,
	},
	tracker,
};

impl<'a> TrackerToolBridge<'a> {
	pub(in crate::agent::tracker_tool_bridge) fn handle_call_inner(
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
				"Manual-attention label intent recorded for issue `{}`. Call `{ISSUE_COMMENT_TOOL_NAME}` kind `{}` next so Decodex can validate the blocker and apply label `{}`.",
				self.issue.identifier,
				super::COMMENT_KIND_MANUAL_ATTENTION,
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
}
