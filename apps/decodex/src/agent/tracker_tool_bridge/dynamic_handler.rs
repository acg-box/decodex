use color_eyre::Report;
use serde_json::Value;

use crate::{
	agent::tracker_tool_bridge::{
		DynamicToolCallResponse, DynamicToolHandler, DynamicToolSpec,
		ISSUE_DELIVERY_CLOSEOUT_COMPLETE_TOOL_NAME, ISSUE_TERMINAL_FINALIZE_TOOL_NAME,
		ReviewExecutionMode, TrackerToolBridge, TurnCompletionStatus,
	},
	prelude::{Result, eyre},
};

impl DynamicToolHandler for TrackerToolBridge<'_> {
	fn tool_specs(&self) -> Vec<DynamicToolSpec> {
		self.build_tool_specs()
	}

	fn handle_call(&self, tool_name: &str, arguments: Value) -> DynamicToolCallResponse {
		self.handle_call_inner(tool_name, arguments)
	}

	fn classify_turn_completion(&self, _final_output: &str) -> Result<TurnCompletionStatus> {
		let Some(review_context) = self.review_context.as_ref() else {
			eyre::bail!(
				"Review handoff context is unavailable for issue `{}`.",
				self.issue.identifier
			);
		};
		let manual_attention_requested = *self.manual_attention_requested.borrow();
		let manual_attention_comment_recorded = *self.manual_attention_comment_recorded.borrow();
		let review_completion = self.pending_review_completion.borrow().clone();

		match (manual_attention_requested, manual_attention_comment_recorded, review_completion) {
			(false, false, None) => {
				if let Some(review_policy_stop) =
					self.review_policy_stop_requested(review_context)?
				{
					return Err(Report::new(review_policy_stop));
				}
				if let Some(reason) = self.continuation_blocking_write_reason()? {
					eyre::bail!(
						"Run `{}` changed issue `{}` via {} without recording a terminal path. Continuation turns may only yield cleanly while the leased issue remains active.",
						review_context.run_id,
						self.issue.identifier,
						reason
					);
				}

				if review_context.mode == ReviewExecutionMode::Closeout {
					eyre::bail!(
						"Run `{}` reached a clean continuation boundary for retained closeout on issue `{}`, but closeout is a deterministic tail. Finish the same turn with `{}` plus `{}` or take the manual-attention path instead of yielding another clean continuation boundary.",
						review_context.run_id,
						self.issue.identifier,
						ISSUE_DELIVERY_CLOSEOUT_COMPLETE_TOOL_NAME,
						ISSUE_TERMINAL_FINALIZE_TOOL_NAME
					);
				}

				Ok(TurnCompletionStatus::Continue)
			},
			(false, false, Some(_)) | (true, true, None) => {
				self.validate_turn_completion("")?;

				Ok(TurnCompletionStatus::Complete)
			},
			(true, false, None) => eyre::bail!(
				"Run `{}` requested human attention with label `{}`, but issue `{}` never recorded the required explanatory comment.",
				review_context.run_id,
				self.workflow.frontmatter().tracker().needs_attention_label(),
				self.issue.identifier
			),
			(true, _, Some(_)) => eyre::bail!(
				"Run `{}` recorded both `issue_review_handoff` and label `{}`. Use exactly one final handoff path.",
				review_context.run_id,
				self.workflow.frontmatter().tracker().needs_attention_label()
			),
			(false, true, None) | (false, true, Some(_)) => eyre::bail!(
				"Run `{}` recorded a human-attention comment for issue `{}`, but never recorded label `{}`.",
				review_context.run_id,
				self.issue.identifier,
				self.workflow.frontmatter().tracker().needs_attention_label()
			),
		}
	}

	fn has_terminal_completion_signal(&self) -> bool {
		self.completion_disposition().is_ok()
	}

	fn validate_turn_completion(&self, _final_output: &str) -> Result<()> {
		let completion_path = self.completion_disposition()?;
		let Some(finalized_path) = *self.finalized_completion_path.borrow() else {
			let Some(review_context) = self.review_context.as_ref() else {
				eyre::bail!(
					"Review handoff context is unavailable for issue `{}`.",
					self.issue.identifier
				);
			};

			eyre::bail!(
				"Run `{}` completed, but issue `{}` never called `{}` for terminal path `{}`.",
				review_context.run_id,
				self.issue.identifier,
				ISSUE_TERMINAL_FINALIZE_TOOL_NAME,
				completion_path.as_str()
			);
		};

		if finalized_path != completion_path {
			let Some(review_context) = self.review_context.as_ref() else {
				eyre::bail!(
					"Review handoff context is unavailable for issue `{}`.",
					self.issue.identifier
				);
			};

			eyre::bail!(
				"Run `{}` finalized terminal path `{}`, but the recorded terminal path resolved to `{}` at turn completion.",
				review_context.run_id,
				finalized_path.as_str(),
				completion_path.as_str()
			);
		}

		Ok(())
	}
}
