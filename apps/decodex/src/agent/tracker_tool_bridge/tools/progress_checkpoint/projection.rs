use crate::{
	agent::tracker_tool_bridge::{
		self, NormalizedProgressCheckpoint, ReviewHandoffContext, TrackerToolBridge,
	},
	state::StateStore,
	tracker::{
		self,
		records::{self, LinearExecutionEventIdentity, LinearExecutionEventRecord},
	},
};

impl<'a> TrackerToolBridge<'a> {
	pub(super) fn render_progress_checkpoint_projection(
		&self,
		review_context: &ReviewHandoffContext,
		checkpoint: &NormalizedProgressCheckpoint,
	) -> LinearExecutionEventRecord {
		let branch = checkpoint.public_branch(review_context);

		records::render_progress_checkpoint_public_projection(
			LinearExecutionEventIdentity {
				service_id: &review_context.service_id,
				issue_id: &self.issue.id,
				issue_identifier: &self.issue.identifier,
				run_id: &review_context.run_id,
				attempt_number: review_context.attempt_number,
			},
			tracker_tool_bridge::current_timestamp(),
			checkpoint.phase.as_str(),
			Some(branch.as_str()),
			Some(review_context.worktree_path.as_str()),
			checkpoint.pr_url.as_deref(),
		)
	}

	pub(super) fn publish_progress_checkpoint_projection(
		&self,
		state_store: &StateStore,
		public_projection: &LinearExecutionEventRecord,
	) -> Result<bool, String> {
		let projection = tracker::prepare_linear_execution_event_comment(
			"",
			public_projection,
			self.public_projection_privacy_classifier,
		)
		.map_err(|error| {
			format!(
				"Failed to prepare the public progress projection for issue `{}`: {error}",
				self.issue.identifier
			)
		})?;

		if self.progress_checkpoint_projection_cached(state_store, &projection.record)? {
			return Ok(false);
		}

		let comment_created = match tracker::create_prepared_linear_execution_event_comment(
			self.tracker,
			&self.issue.id,
			&projection,
		) {
			Ok(comment_created) => comment_created,
			Err(error) => {
				return Err(format!(
					"Failed to record an execution-state checkpoint for issue `{}`: {error}",
					self.issue.identifier
				));
			},
		};

		state_store.record_linear_execution_event(&projection.record).map_err(|error| {
			format!(
				"Failed to persist the public progress projection cache for issue `{}`: {error}",
				self.issue.identifier
			)
		})?;

		Ok(comment_created)
	}

	fn progress_checkpoint_projection_cached(
		&self,
		state_store: &StateStore,
		public_projection: &LinearExecutionEventRecord,
	) -> Result<bool, String> {
		let records = state_store
			.list_linear_execution_events(
				&public_projection.service_id,
				&public_projection.issue_id,
			)
			.map_err(|error| {
				format!(
					"Failed to read the public progress projection cache for issue `{}`: {error}",
					self.issue.identifier
				)
			})?;

		Ok(records.iter().any(|record| record.idempotency_key == public_projection.idempotency_key))
	}
}
