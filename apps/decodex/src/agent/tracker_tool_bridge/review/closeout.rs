use crate::{
	agent::tracker_tool_bridge::review::{
		self, CLOSEOUT_PUBLIC_SUMMARY_FALLBACK, ISSUE_DELIVERY_CLOSEOUT_COMPLETE_TOOL_NAME,
		ISSUE_TRANSITION_TOOL_NAME, PendingReviewCompletion, PullRequestDetails,
		ReviewHandoffContext, TrackerToolBridge, eyre, tracker_tool_bridge,
	},
	tracker,
};

enum CloseoutIssueStateValidation {
	RefreshRequired,
	AlreadyVerified,
}

impl<'a> TrackerToolBridge<'a> {
	pub(crate) fn apply_closeout(&self) -> crate::prelude::Result<()> {
		let Some(review_context) = self.review_context.as_ref() else {
			eyre::bail!(
				"Review handoff context is unavailable for issue `{}`.",
				self.issue.identifier
			);
		};
		let pending_closeout = {
			let pending_review_completion = self.pending_review_completion.borrow();
			let Some(PendingReviewCompletion::Closeout(pending_closeout)) =
				pending_review_completion.as_ref()
			else {
				eyre::bail!(
					"Run `{}` completed, but issue `{}` never recorded retained closeout completion.",
					review_context.run_id,
					self.issue.identifier
				);
			};

			pending_closeout.clone()
		};

		self.write_closeout_record(
			review_context,
			&pending_closeout.pr_url,
			CloseoutIssueStateValidation::RefreshRequired,
			&pending_closeout.summary,
		)?;
		self.pending_review_completion.borrow_mut().take();

		Ok(())
	}

	pub(crate) fn validate_deterministic_closeout_pr(
		&self,
		pr_url: &str,
	) -> crate::prelude::Result<PullRequestDetails> {
		let Some(review_context) = self.review_context.as_ref() else {
			eyre::bail!(
				"Review handoff context is unavailable for issue `{}`.",
				self.issue.identifier
			);
		};

		self.validate_closeout_pr(review_context, pr_url).map_err(|error| eyre::eyre!(error))
	}

	pub(crate) fn apply_validated_deterministic_closeout(
		&self,
		pull_request: PullRequestDetails,
	) -> crate::prelude::Result<()> {
		let Some(review_context) = self.review_context.as_ref() else {
			eyre::bail!(
				"Review handoff context is unavailable for issue `{}`.",
				self.issue.identifier
			);
		};

		self.write_validated_closeout_record(
			review_context,
			pull_request,
			CloseoutIssueStateValidation::AlreadyVerified,
			"Validated merged PR lineage and completed retained closeout.",
		)
	}

	fn write_closeout_record(
		&self,
		review_context: &ReviewHandoffContext,
		pr_url: &str,
		issue_state_validation: CloseoutIssueStateValidation,
		summary: &str,
	) -> crate::prelude::Result<()> {
		let pull_request = self
			.validate_closeout_pr(review_context, pr_url)
			.map_err(|error| eyre::eyre!(error))?;

		self.write_validated_closeout_record(
			review_context,
			pull_request,
			issue_state_validation,
			summary,
		)
	}

	fn write_validated_closeout_record(
		&self,
		review_context: &ReviewHandoffContext,
		pull_request: PullRequestDetails,
		issue_state_validation: CloseoutIssueStateValidation,
		summary: &str,
	) -> crate::prelude::Result<()> {
		if matches!(issue_state_validation, CloseoutIssueStateValidation::RefreshRequired) {
			self.validate_closeout_issue_completed_state().map_err(|error| eyre::eyre!(error))?;
		}

		let public_summary = tracker_tool_bridge::public_summary_or_fallback(
			summary,
			CLOSEOUT_PUBLIC_SUMMARY_FALLBACK,
		);
		let closeout_record = review::linear_execution_closeout_event(
			self.issue,
			review_context,
			&pull_request,
			public_summary.as_ref(),
		);
		let retry_budget_line = self
			.state_store
			.map(|state_store| {
				state_store.retry_budget_attempt_count(&self.issue.id).map(|count| {
					if count > 0 {
						format!("\n- retry_budget_attempts_consumed: `{count}`")
					} else {
						String::new()
					}
				})
			})
			.transpose()?
			.unwrap_or_default();
		let closeout_comment = format!(
			"decodex closeout completed\n\n- run_id: `{}`\n- run_sequence_attempt: `{}` (not retry-budget count){}\n- finished_at: `{}`\n- branch: `{}`\n- pr_url: `{}`\n- worktree_path: `{}`\n- summary: {}",
			review_context.run_id,
			review_context.attempt_number,
			retry_budget_line,
			tracker_tool_bridge::current_timestamp(),
			review_context.branch_name,
			pull_request.url,
			review_context.worktree_path,
			public_summary,
		);
		let projection = tracker::prepare_linear_execution_event_comment(
			&closeout_comment,
			&closeout_record,
			self.public_projection_privacy_classifier,
		)?;

		tracker::create_prepared_linear_execution_event_comment(
			self.tracker,
			&self.issue.id,
			&projection,
		)?;

		self.persist_linear_execution_event(&projection.record)?;

		Ok(())
	}

	pub(crate) fn clear_closeout_issue_scope(&self) -> crate::prelude::Result<()> {
		let Some(review_context) = self.review_context.as_ref() else {
			eyre::bail!(
				"Review handoff context is unavailable for issue `{}`.",
				self.issue.identifier
			);
		};

		tracker::clear_automation_lane_labels(self.tracker, self.issue, &review_context.service_id)
	}

	pub(in crate::agent::tracker_tool_bridge) fn validate_closeout_issue_completed_state(
		&self,
	) -> std::result::Result<(), String> {
		let completed_state = self.workflow.frontmatter().tracker().resolved_completed_state();
		let current_issue = self.refreshed_issue_snapshot().map_err(|error| error.to_string())?
			.ok_or_else(|| {
				format!(
					"Failed to refresh issue `{}` during closeout validation: tracker returned no current snapshot.",
					self.issue.identifier
				)
			})?;

		if current_issue.state.name != completed_state {
			return Err(format!(
				"Closeout for issue `{}` requires tracker state `{}`, but the refreshed issue is still `{}`. Move the issue to `{}` with `{}` before calling `{}`.",
				self.issue.identifier,
				completed_state,
				current_issue.state.name,
				completed_state,
				ISSUE_TRANSITION_TOOL_NAME,
				ISSUE_DELIVERY_CLOSEOUT_COMPLETE_TOOL_NAME
			));
		}

		Ok(())
	}
}
