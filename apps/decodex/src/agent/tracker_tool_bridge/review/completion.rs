use crate::{
	agent::tracker_tool_bridge::review::{
		PendingReviewCompletion, RunCompletionDisposition, TrackerToolBridge, eyre,
	},
	prelude::Result,
};

impl<'a> TrackerToolBridge<'a> {
	pub(crate) fn completion_disposition(&self) -> Result<RunCompletionDisposition> {
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
			(false, false, Some(PendingReviewCompletion::Handoff(_))) =>
				Ok(RunCompletionDisposition::ReviewHandoff),
			(false, false, Some(PendingReviewCompletion::Repair(_))) =>
				Ok(RunCompletionDisposition::ReviewRepair),
			(false, false, Some(PendingReviewCompletion::Closeout(_))) =>
				Ok(RunCompletionDisposition::Closeout),
			(true, true, None) => Ok(RunCompletionDisposition::ManualAttention),
			(true, false, None) => eyre::bail!(
				"Run `{}` requested human attention with label `{}`, but issue `{}` never recorded the required explanatory comment.",
				review_context.run_id,
				self.workflow.frontmatter().tracker().needs_attention_label(),
				self.issue.identifier
			),
			(true, _, Some(_)) => eyre::bail!(
				"Run `{}` recorded both `{}` and label `{}`. Use exactly one final tracker exit path.",
				review_context.run_id,
				self.required_pr_completion_tool_name(),
				self.workflow.frontmatter().tracker().needs_attention_label()
			),
			(false, false, None) => eyre::bail!(
				"Run `{}` completed, but issue `{}` recorded neither `{}` nor label `{}` for human attention.",
				review_context.run_id,
				self.issue.identifier,
				self.required_pr_completion_tool_name(),
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

	pub(crate) fn has_tracker_exit_signal(&self) -> bool {
		*self.manual_attention_requested.borrow()
			|| *self.manual_attention_comment_recorded.borrow()
			|| self.pending_review_completion.borrow().is_some()
	}

	pub(crate) fn finalized_completion_disposition(
		&self,
	) -> Result<Option<RunCompletionDisposition>> {
		let Some(finalized_path) = *self.finalized_completion_path.borrow() else {
			return Ok(None);
		};
		let completion_path = self.completion_disposition()?;

		if finalized_path != completion_path {
			let Some(review_context) = self.review_context.as_ref() else {
				eyre::bail!(
					"Review handoff context is unavailable for issue `{}`.",
					self.issue.identifier
				);
			};

			eyre::bail!(
				"Run `{}` finalized terminal path `{}`, but the recorded terminal path resolved to `{}` after app-server failure.",
				review_context.run_id,
				finalized_path.as_str(),
				completion_path.as_str()
			);
		}

		Ok(Some(finalized_path))
	}
}
