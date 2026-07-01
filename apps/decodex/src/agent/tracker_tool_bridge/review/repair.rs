#[allow(clippy::wildcard_imports)] use super::*;

impl<'a> TrackerToolBridge<'a> {
	pub(crate) fn apply_review_repair(&self) -> crate::prelude::Result<()> {
		let Some(review_context) = self.review_context.as_ref() else {
			eyre::bail!(
				"Review handoff context is unavailable for issue `{}`.",
				self.issue.identifier
			);
		};
		let pending_review_repair = {
			let pending_review_repair = self.pending_review_completion.borrow();
			let Some(PendingReviewCompletion::Repair(pending_review_repair)) =
				pending_review_repair.as_ref()
			else {
				eyre::bail!(
					"Run `{}` completed, but issue `{}` never recorded retained review repair completion.",
					review_context.run_id,
					self.issue.identifier
				);
			};

			pending_review_repair.clone()
		};
		let pull_request = self
			.validate_review_action_pr(review_context, &pending_review_repair.pr_url)
			.map_err(|error| eyre::eyre!(error))?;
		let public_summary = tracker_tool_bridge::public_summary_or_fallback(
			&pending_review_repair.summary,
			REVIEW_REPAIR_PUBLIC_SUMMARY_FALLBACK,
		);
		let completion_comment = tracker_tool_bridge::format_review_repair_comment(
			review_context,
			&pending_review_repair,
			public_summary.as_ref(),
		);
		let handoff_record = linear_execution_review_event(
			self.issue,
			review_context,
			&pull_request,
			"repair_handoff",
			"review_repair",
			public_summary.as_ref(),
		);
		let review_handoff = ReviewHandoffMarker::new(
			review_context.run_id.clone(),
			review_context.attempt_number,
			review_context.branch_name.clone(),
			pull_request.url.clone(),
			pull_request.base_ref_name.clone(),
			pull_request.head_ref_name.clone(),
			pull_request.head_ref_oid.clone(),
		);
		let projection = tracker::prepare_linear_execution_event_comment(
			&completion_comment,
			&handoff_record,
			self.public_projection_privacy_classifier,
		)?;
		let state_store = self.state_store.ok_or_else(|| {
			eyre::eyre!(
				"Runtime state store is required to read review orchestration for issue `{}`.",
				self.issue.identifier
			)
		})?;
		let previous_review_handoff = state_store.review_handoff_marker(
			&review_context.service_id,
			&self.issue.id,
			&review_context.branch_name,
		)?;
		let persisted_orchestration = previous_review_handoff
			.as_ref()
			.map(|marker| {
				state_store.review_orchestration_marker(
					&review_context.service_id,
					&self.issue.id,
					marker,
				)
			})
			.transpose()?
			.flatten();
		let external_round_count =
			persisted_orchestration.map_or(0, |marker| marker.external_round_count());

		tracker::create_prepared_linear_execution_event_comment(
			self.tracker,
			&self.issue.id,
			&projection,
		)?;

		self.persist_linear_execution_event(&projection.record)?;
		self.persist_review_handoff_marker(review_context, &review_handoff)?;
		self.persist_review_orchestration_marker(
			review_context,
			&ReviewOrchestrationMarker::new(
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
				external_round_count,
				None,
			),
		)?;
		self.pending_review_completion.borrow_mut().take();

		Ok(())
	}
}
