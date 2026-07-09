use std::path::Path;

use crate::{
	prelude::Result,
	state::{
		self, RUN_CONTROL_CHANNEL_STATUS_ACTIVE, RunControlChannel, RunControlChannelRecord,
		StateStore, store_run_control::validation,
	},
};

impl StateStore {
	/// Read one active run-control channel for an issue, when retained runtime control exists.
	pub(crate) fn active_run_control_channel_for_issue(
		&self,
		project_id: &str,
		issue_id: &str,
	) -> Result<Option<RunControlChannel>> {
		let state = self.lock()?;

		Ok(state
			.control_channels
			.values()
			.filter(|channel| {
				channel.project_id == project_id
					&& channel.issue_id == issue_id
					&& channel.status == RUN_CONTROL_CHANNEL_STATUS_ACTIVE
			})
			.max_by(|left, right| {
				left.attempt_number
					.cmp(&right.attempt_number)
					.then_with(|| left.run_id.cmp(&right.run_id))
			})
			.map(RunControlChannelRecord::as_public))
	}

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

		let Some(lease) = state.leases.get(&attempt.issue_id) else {
			return Ok(None);
		};

		if lease.run_id != run_id {
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
			project_id: lease.project_id.clone(),
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
