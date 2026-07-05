use serde_json::Value;

use crate::{
	agent::tracker_tool_bridge::{
		DynamicToolCallResponse, ISSUE_COMMENT_TOOL_NAME, ISSUE_LABEL_ADD_TOOL_NAME, LabelArgs,
		TrackerToolBridge, tools::COMMENT_KIND_MANUAL_ATTENTION,
	},
	tracker,
};

impl<'a> TrackerToolBridge<'a> {
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
}
