use crate::{
	agent::tracker_tool_bridge::{
		review,
		review::{
			LinearExecutionEventPublicProjection, PendingReviewAction, PendingReviewCompletion,
			PullRequestDetails, REVIEW_HANDOFF_PUBLIC_SUMMARY_FALLBACK, Report,
			ReviewHandoffContext, ReviewHandoffWritebackFailed, TrackerToolBridge, eyre,
			tracker_tool_bridge,
		},
	},
	prelude::Result,
	state::ReviewLifecycleTransitionInput,
	tracker,
};

impl<'a> TrackerToolBridge<'a> {
	pub(crate) fn apply_review_handoff(&self) -> Result<()> {
		let Some(review_context) = self.review_context.as_ref() else {
			eyre::bail!(
				"Review handoff context is unavailable for issue `{}`.",
				self.issue.identifier
			);
		};
		let pending_review_handoff = {
			let pending_review_handoff = self.pending_review_completion.borrow();
			let Some(PendingReviewCompletion::Handoff(pending_review_handoff)) =
				pending_review_handoff.as_ref()
			else {
				eyre::bail!(
					"Run `{}` completed, but issue `{}` never recorded a PR-backed review handoff.",
					review_context.run_id,
					self.issue.identifier
				);
			};

			pending_review_handoff.clone()
		};
		let pull_request = self
			.validate_review_action_pr(review_context, &pending_review_handoff.pr_url)
			.map_err(|error| eyre::eyre!(error))?;
		let success_state = self.workflow.frontmatter().tracker().success_state();
		let success_state_id = self.issue.state_id_for_name(success_state).ok_or_else(|| {
			eyre::eyre!(
				"State `{success_state}` does not exist on issue `{}`.",
				self.issue.identifier
			)
		})?;
		let projection = self.prepare_review_handoff_projection(
			review_context,
			&pending_review_handoff,
			&pull_request,
			success_state,
		)?;
		let lifecycle_handoff =
			review::review_lifecycle_handoff_from_pull_request(review_context, &pull_request);

		self.persist_review_lifecycle_handoff_for_handoff(review_context, lifecycle_handoff)?;
		self.persist_review_lifecycle_transition(
			review_context,
			ReviewLifecycleTransitionInput {
				run_id: &review_context.run_id,
				attempt_number: review_context.attempt_number,
				branch_name: &review_context.branch_name,
				pr_url: &pull_request.url,
				head_sha: &pull_request.head_ref_oid,
				phase: "request_pending",
				request_comment_database_id: None,
				request_created_at_unix_epoch: None,
				request_description_thumbs_up_count: None,
				request_retry_count: 0,
				external_round_count: 0,
				auto_merge_enabled_at_unix_epoch: None,
			},
		)?;

		if let Err(error) = tracker::create_prepared_linear_execution_event_comment(
			self.tracker,
			&self.issue.id,
			&projection,
		) {
			return Err(Report::new(ReviewHandoffWritebackFailed {
				issue_identifier: self.issue.identifier.clone(),
				run_id: review_context.run_id.clone(),
				pr_url: pending_review_handoff.pr_url,
				success_state: success_state.to_owned(),
				source: format!("failed to persist the tracker review handoff record: {error}"),
			}));
		}

		self.persist_linear_execution_event(&projection.record)?;

		if let Err(error) = self.tracker.update_issue_state(&self.issue.id, success_state_id) {
			return Err(Report::new(ReviewHandoffWritebackFailed {
				issue_identifier: self.issue.identifier.clone(),
				run_id: review_context.run_id.clone(),
				pr_url: pull_request.url.clone(),
				success_state: success_state.to_owned(),
				source: format!("failed to move the tracker issue to `{success_state}`: {error}"),
			}));
		}

		self.pending_review_completion.borrow_mut().take();

		Ok(())
	}

	fn prepare_review_handoff_projection(
		&self,
		review_context: &ReviewHandoffContext,
		pending_review_handoff: &PendingReviewAction,
		pull_request: &PullRequestDetails,
		success_state: &str,
	) -> Result<LinearExecutionEventPublicProjection> {
		let public_summary = tracker_tool_bridge::public_summary_or_fallback(
			&pending_review_handoff.summary,
			REVIEW_HANDOFF_PUBLIC_SUMMARY_FALLBACK,
		);
		let completion_comment = tracker_tool_bridge::format_review_handoff_comment(
			review_context,
			pending_review_handoff,
			public_summary.as_ref(),
		);
		let handoff_record = review::linear_execution_review_event(
			self.issue,
			review_context,
			pull_request,
			"review_handoff",
			"review_handoff",
			public_summary.as_ref(),
		);

		tracker::prepare_linear_execution_event_comment(
			&completion_comment,
			&handoff_record,
			self.public_projection_privacy_classifier,
		)
		.map_err(|error| {
			Report::new(ReviewHandoffWritebackFailed {
				issue_identifier: self.issue.identifier.clone(),
				run_id: review_context.run_id.clone(),
				pr_url: pull_request.url.clone(),
				success_state: success_state.to_owned(),
				source: format!("failed to prepare the tracker review handoff record: {error}"),
			})
		})
	}
}
