use super::{
	LinearExecutionEventPublicProjection, PendingReviewAction, PendingReviewCompletion,
	PullRequestDetails, REVIEW_HANDOFF_PUBLIC_SUMMARY_FALLBACK, Report, ReviewHandoffContext,
	ReviewHandoffWritebackFailed, ReviewOrchestrationMarker, TrackerToolBridge, eyre,
	linear_execution_review_event, review_handoff_marker_from_pull_request, tracker,
	tracker_tool_bridge,
};

impl<'a> TrackerToolBridge<'a> {
	pub(crate) fn apply_review_handoff(&self) -> crate::prelude::Result<()> {
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
		let handoff_marker = review_handoff_marker_from_pull_request(review_context, &pull_request);
		let orchestration_marker = ReviewOrchestrationMarker::new(
			review_context.run_id.clone(),
			review_context.attempt_number,
			review_context.branch_name.clone(),
			pull_request.url.clone(),
			pull_request.head_ref_oid.clone(),
			"request_pending",
			None,
			None,
			None,
			0,
			0,
			None,
		);

		self.persist_review_handoff_marker_for_handoff(review_context, &handoff_marker)?;
		self.persist_review_orchestration_marker(review_context, &orchestration_marker)?;

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
	) -> crate::prelude::Result<LinearExecutionEventPublicProjection> {
		let public_summary = tracker_tool_bridge::public_summary_or_fallback(
			&pending_review_handoff.summary,
			REVIEW_HANDOFF_PUBLIC_SUMMARY_FALLBACK,
		);
		let completion_comment = tracker_tool_bridge::format_review_handoff_comment(
			review_context,
			pending_review_handoff,
			public_summary.as_ref(),
		);
		let handoff_record = linear_execution_review_event(
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
