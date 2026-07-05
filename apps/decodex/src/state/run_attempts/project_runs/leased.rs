use crate::{
	prelude::Result,
	state::{ProjectRunStatus, StateStore},
};

impl StateStore {
	/// List all leased run attempts for one project without applying the recent-run limit.
	pub fn list_leased_runs(&self, project_id: &str) -> Result<Vec<ProjectRunStatus>> {
		let mut state = self.lock_without_refresh()?;

		self.refresh_project_run_metadata_state_locked(&mut state, project_id)?;

		let mut runs = state
			.run_attempts
			.values()
			.filter_map(|attempt| {
				let status = state.project_run_status(project_id, attempt)?;

				status.run_lease.then_some(status)
			})
			.collect::<Vec<_>>();
		let mut run_ids = runs.iter().map(|run| run.run_id().to_owned()).collect::<Vec<_>>();

		run_ids.sort();
		run_ids.dedup();
		self.refresh_protocol_event_summaries_for_runs_locked(&mut state, &run_ids)?;

		runs = state
			.run_attempts
			.values()
			.filter(|attempt| run_ids.contains(&attempt.run_id))
			.filter_map(|attempt| {
				let status = state.project_run_status(project_id, attempt)?;

				status.run_lease.then_some(status)
			})
			.collect::<Vec<_>>();

		runs.sort_by(crate::state::compare_project_run_status);

		Ok(runs)
	}
}
