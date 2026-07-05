use crate::{
	agent::tracker_tool_bridge::{
		ReviewHandoffContext, ReviewPolicyPhase, ReviewPolicyState, ReviewPolicyStatus,
		TrackerToolBridge,
	},
	prelude::{Result, eyre},
	state::{ReviewCheckpointArtifactLookup, ReviewPolicyCheckpoint, ReviewPolicyCheckpointInput},
};

impl<'a> TrackerToolBridge<'a> {
	pub(in crate::agent::tracker_tool_bridge) fn persist_review_policy_state(
		&self,
		review_context: &ReviewHandoffContext,
		review_policy_phase: ReviewPolicyPhase,
		review_policy_status: ReviewPolicyStatus,
		head_sha: &str,
		nonclean_rounds: i64,
		details_json: &str,
	) -> Result<(), String> {
		let state_store = self.state_store.ok_or_else(|| {
			format!(
				"Runtime state store is required to persist review checkpoint state for issue `{}`.",
				self.issue.identifier
			)
		})?;

		state_store
			.upsert_review_policy_checkpoint(ReviewPolicyCheckpointInput {
				project_id: &review_context.service_id,
				issue_id: &self.issue.id,
				run_id: &review_context.run_id,
				attempt_number: review_context.attempt_number,
				phase: review_policy_phase.as_str(),
				review_level: review_context.review_level.as_str(),
				status: review_policy_status.as_str(),
				head_sha,
				nonclean_rounds,
				details_json,
			})
			.map_err(|error| {
				format!(
					"Failed to persist review checkpoint state for issue `{}`: {error}",
					self.issue.identifier
				)
			})?;

		Ok(())
	}

	fn review_policy_state_from_checkpoint(
		&self,
		checkpoint: ReviewPolicyCheckpoint,
	) -> Result<ReviewPolicyState> {
		let phase =
			ReviewPolicyPhase::parse(checkpoint.phase()).map_err(|error| eyre::eyre!(error))?;
		let status =
			ReviewPolicyStatus::parse(checkpoint.status()).map_err(|error| eyre::eyre!(error))?;

		Ok(ReviewPolicyState {
			phase,
			status,
			head_sha: checkpoint.head_sha().to_owned(),
			nonclean_rounds: checkpoint.nonclean_rounds(),
			details_json: checkpoint.details_json().to_owned(),
		})
	}

	pub(in crate::agent::tracker_tool_bridge) fn review_policy_state_for_current_head(
		&self,
		review_context: &ReviewHandoffContext,
	) -> Result<Option<ReviewPolicyState>> {
		let Some(current_phase) = ReviewPolicyPhase::for_mode(review_context.mode) else {
			return Ok(None);
		};
		let local_repo =
			self.current_local_repo_details(review_context).map_err(|error| eyre::eyre!(error))?;

		self.review_policy_artifact_for_head(review_context, current_phase, &local_repo.head_oid)
	}

	pub(in crate::agent::tracker_tool_bridge) fn review_policy_artifact_for_head(
		&self,
		review_context: &ReviewHandoffContext,
		review_policy_phase: ReviewPolicyPhase,
		head_sha: &str,
	) -> Result<Option<ReviewPolicyState>> {
		let Some(state_store) = self.state_store else {
			return Ok(None);
		};
		let Some(checkpoint) =
			state_store.review_checkpoint_artifact(ReviewCheckpointArtifactLookup {
				project_id: &review_context.service_id,
				issue_id: &self.issue.id,
				phase: review_policy_phase.as_str(),
				review_level: review_context.review_level.as_str(),
				head_sha,
			})?
		else {
			return Ok(None);
		};

		self.review_policy_state_from_checkpoint(checkpoint).map(Some)
	}
}
