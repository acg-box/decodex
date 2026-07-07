use crate::{
	agent::tracker_tool_bridge::{ReviewHandoffContext, TrackerToolBridge, review},
	prelude::{Result, eyre},
	state::{ReviewLifecycleHandoffInput, ReviewLifecycleTransitionInput},
	tracker::records::LinearExecutionEventRecord,
};

impl<'a> TrackerToolBridge<'a> {
	pub(in crate::agent::tracker_tool_bridge::review) fn persist_linear_execution_event(
		&self,
		record: &LinearExecutionEventRecord,
	) -> Result<()> {
		if let Some(state_store) = self.state_store {
			state_store.record_linear_execution_event(record)?;
		}

		Ok(())
	}

	pub(in crate::agent::tracker_tool_bridge::review) fn persist_review_lifecycle_handoff(
		&self,
		review_context: &ReviewHandoffContext,
		input: ReviewLifecycleHandoffInput<'_>,
	) -> Result<()> {
		let state_store = self.state_store.ok_or_else(|| {
			eyre::eyre!(
				"Runtime state store is required to persist review handoff for issue `{}`.",
				self.issue.identifier
			)
		})?;

		state_store.record_review_lifecycle_handoff(
			&review_context.service_id,
			&self.issue.id,
			input,
		)
	}

	pub(in crate::agent::tracker_tool_bridge::review) fn persist_review_lifecycle_handoff_for_handoff(
		&self,
		review_context: &ReviewHandoffContext,
		input: ReviewLifecycleHandoffInput<'_>,
	) -> Result<()> {
		let state_store = self.state_store.ok_or_else(|| {
			eyre::eyre!(
				"Runtime state store is required to persist review handoff for issue `{}`.",
				self.issue.identifier
			)
		})?;

		if let Some(existing_record) = state_store.review_lifecycle_record(
			&review_context.service_id,
			&self.issue.id,
			&review_context.branch_name,
		)? {
			if !review::review_lifecycle_handoff_lineage_matches(&existing_record, &input) {
				eyre::bail!(
					"Existing review lifecycle record for issue `{}` branch `{}` points at PR `{}` head `{}`, but the current review handoff intent points at PR `{}` head `{}`. Use explicit review-handoff recovery before rebinding this lane.",
					self.issue.identifier,
					review_context.branch_name,
					existing_record.pr_url(),
					existing_record.pr_head_oid(),
					input.pr_url,
					input.head_sha
				);
			}
		}

		self.persist_review_lifecycle_handoff(review_context, input)
	}

	pub(in crate::agent::tracker_tool_bridge::review) fn persist_review_lifecycle_transition(
		&self,
		review_context: &ReviewHandoffContext,
		input: ReviewLifecycleTransitionInput<'_>,
	) -> Result<()> {
		let state_store = self.state_store.ok_or_else(|| {
			eyre::eyre!(
				"Runtime state store is required to persist review lifecycle transition for issue `{}`.",
				self.issue.identifier
			)
		})?;

		state_store.record_review_lifecycle_transition(
			&review_context.service_id,
			&self.issue.id,
			input,
		)
	}
}
