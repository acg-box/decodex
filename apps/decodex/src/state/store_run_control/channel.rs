use std::path::Path;

use crate::{
	lane_authority::LaneId,
	prelude::Result,
	state::{
		self, RUN_CONTROL_CHANNEL_STATUS_ACTIVE, RunControlChannel, RunControlChannelRecord,
		StateStore, store_run_control::validation,
	},
};

impl StateStore {
	/// Publish the local control channel for an active attempt when the runtime owns it.
	pub(crate) fn publish_run_control_channel_for_active_attempt(
		&self,
		run_id: &str,
		attempt_number: i64,
		channel_path: &Path,
		transport: &str,
	) -> Result<Option<RunControlChannel>> {
		validation::validate_run_control_channel_inputs(
			run_id,
			attempt_number,
			channel_path,
			transport,
		)?;

		let now = state::timestamp_parts();
		let mut state = self.lock_without_refresh()?;
		let Some(attempt) = state.run_attempts.get(run_id).cloned() else {
			return Ok(None);
		};

		if attempt.attempt_number != attempt_number {
			return Ok(None);
		}

		let project_id = match attempt.project_id.as_deref() {
			Some(project_id) => project_id.to_owned(),
			None => {
				#[cfg(not(test))]
				return Ok(None);
				#[cfg(test)]
				match state.leases.get(&attempt.issue_id) {
					Some(lease) if lease.run_id == run_id => lease.project_id.clone(),
					_ => return Ok(None),
				}
			},
		};
		let lane_id = LaneId::new(&project_id, &attempt.issue_id)?;
		if let Some(lane) = state.lanes.get(&lane_id) {
			if lane.intake_authority_id().is_none() || lane.claim_run_id() != Some(run_id) {
				return Ok(None);
			}
		} else {
			#[cfg(not(test))]
			return Ok(None);
		}

		let (published_at, published_at_unix) = state
			.control_channels
			.get(run_id)
			.filter(|channel| channel.attempt_number == attempt_number)
			.map_or_else(
				|| (now.text.clone(), now.unix),
				|channel| (channel.published_at.clone(), channel.published_at_unix),
			);
		let channel = RunControlChannelRecord {
			project_id,
			issue_id: attempt.issue_id.clone(),
			run_id: run_id.to_owned(),
			attempt_number,
			transport: transport.to_owned(),
			channel_path: channel_path.to_path_buf(),
			status: RUN_CONTROL_CHANNEL_STATUS_ACTIVE.to_owned(),
			published_at,
			published_at_unix,
			updated_at: now.text,
			updated_at_unix: now.unix,
		};

		state.control_channels.insert(run_id.to_owned(), channel.clone());
		self.upsert_run_control_channel_locked(&channel)?;

		Ok(Some(channel.as_public()))
	}

	/// Mark a run-control channel as no longer active for an attempt.
	pub(crate) fn retire_run_control_channel_for_attempt(
		&self,
		run_id: &str,
		attempt_number: i64,
		status: &str,
	) -> Result<()> {
		validation::validate_run_control_channel_status(status)?;

		let now = state::timestamp_parts();
		let mut state = self.lock_without_refresh()?;
		let Some(channel) = state.control_channels.get_mut(run_id) else {
			return Ok(());
		};

		if channel.attempt_number != attempt_number {
			return Ok(());
		}

		channel.status = status.to_owned();
		channel.updated_at = now.text;
		channel.updated_at_unix = now.unix;

		let channel = channel.clone();

		self.upsert_run_control_channel_locked(&channel)
	}
}
