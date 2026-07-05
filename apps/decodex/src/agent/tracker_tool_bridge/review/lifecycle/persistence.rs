use crate::{
	agent::tracker_tool_bridge::{ReviewHandoffContext, TrackerToolBridge, review},
	prelude::{Result, eyre},
	state::{ReviewHandoffMarker, ReviewOrchestrationMarker},
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

	pub(in crate::agent::tracker_tool_bridge::review) fn persist_review_handoff_marker(
		&self,
		review_context: &ReviewHandoffContext,
		marker: &ReviewHandoffMarker,
	) -> Result<()> {
		let state_store = self.state_store.ok_or_else(|| {
			eyre::eyre!(
				"Runtime state store is required to persist review handoff for issue `{}`.",
				self.issue.identifier
			)
		})?;

		state_store.upsert_review_handoff_marker(&review_context.service_id, &self.issue.id, marker)
	}

	pub(in crate::agent::tracker_tool_bridge::review) fn persist_review_handoff_marker_for_handoff(
		&self,
		review_context: &ReviewHandoffContext,
		marker: &ReviewHandoffMarker,
	) -> Result<()> {
		let state_store = self.state_store.ok_or_else(|| {
			eyre::eyre!(
				"Runtime state store is required to persist review handoff for issue `{}`.",
				self.issue.identifier
			)
		})?;

		if let Some(existing) = state_store.review_handoff_marker(
			&review_context.service_id,
			&self.issue.id,
			&review_context.branch_name,
		)? && !review::review_handoff_marker_lineage_matches(&existing, marker)
		{
			eyre::bail!(
				"Existing review lifecycle record for issue `{}` branch `{}` points at PR `{}` head `{}`, but the current review handoff intent points at PR `{}` head `{}`. Use explicit review-handoff recovery before rebinding this lane.",
				self.issue.identifier,
				review_context.branch_name,
				existing.pr_url(),
				existing.pr_head_oid(),
				marker.pr_url(),
				marker.pr_head_oid()
			);
		}

		self.persist_review_handoff_marker(review_context, marker)
	}

	pub(in crate::agent::tracker_tool_bridge::review) fn persist_review_orchestration_marker(
		&self,
		review_context: &ReviewHandoffContext,
		marker: &ReviewOrchestrationMarker,
	) -> Result<()> {
		let state_store = self.state_store.ok_or_else(|| {
			eyre::eyre!(
				"Runtime state store is required to persist review orchestration for issue `{}`.",
				self.issue.identifier
			)
		})?;

		state_store.upsert_review_orchestration_marker(
			&review_context.service_id,
			&self.issue.id,
			marker,
		)
	}
}
