use crate::{
	agent::tracker_tool_bridge::{
		review,
		review::{
			PendingReviewCompletion, REVIEW_REPAIR_PUBLIC_SUMMARY_FALLBACK, TrackerToolBridge,
			eyre, tracker_tool_bridge,
		},
	},
	prelude::Result,
	state::ReviewLifecycleTransitionInput,
	tracker,
};

impl<'a> TrackerToolBridge<'a> {
	pub(crate) fn apply_review_repair(&self) -> Result<()> {
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
		let handoff_record = review::linear_execution_review_event(
			self.issue,
			review_context,
			&pull_request,
			"repair_handoff",
			"review_repair",
			public_summary.as_ref(),
		);
		let lifecycle_handoff =
			review::review_lifecycle_handoff_from_pull_request(review_context, &pull_request);
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
		let external_round_count = state_store
			.review_lifecycle_record(
				&review_context.service_id,
				&self.issue.id,
				&review_context.branch_name,
			)?
			.as_ref()
			.map_or(0, |record| record.external_round_count());

		tracker::create_prepared_linear_execution_event_comment(
			self.tracker,
			&self.issue.id,
			&projection,
		)?;

		self.persist_linear_execution_event(&projection.record)?;
		self.persist_review_lifecycle_handoff(review_context, lifecycle_handoff)?;
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
				external_round_count,
				auto_merge_enabled_at_unix_epoch: None,
			},
		)?;
		self.pending_review_completion.borrow_mut().take();

		Ok(())
	}
}
