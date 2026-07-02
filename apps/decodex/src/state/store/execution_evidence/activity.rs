use crate::{
	prelude::Result,
	state::{
		runtime_records::RunActivitySummaryRecord,
		store::{self, ChildAgentActivitySummary, ProtocolActivitySummary, StateStore},
	},
};

impl StateStore {
	pub(crate) fn record_run_activity_summary(
		&self,
		run_id: &str,
		attempt_number: i64,
		child_agent_activity: Option<&ChildAgentActivitySummary>,
		protocol_activity: Option<&ProtocolActivitySummary>,
	) -> Result<()> {
		let now = store::timestamp_parts();
		let summary = RunActivitySummaryRecord {
			run_id: run_id.to_owned(),
			attempt_number,
			child_agent_activity: child_agent_activity
				.cloned()
				.map(ChildAgentActivitySummary::sealed_durable),
			protocol_activity: protocol_activity.cloned(),
			updated_at: now.text,
			updated_at_unix: now.unix,
		};
		let mut state = self.lock_without_refresh()?;

		state.run_activity_summaries.insert(run_id.to_owned(), summary.clone());

		self.upsert_run_activity_summary_locked(&summary)
	}

	/// Read the latest recorded activity timestamp for one run as a Unix epoch.
	pub fn last_run_activity_unix_epoch(&self, run_id: &str) -> Result<Option<i64>> {
		let state = self.lock()?;
		let last_activity = state.run_attempts.get(run_id).map(|attempt| attempt.updated_at_unix);
		let last_event = state.protocol_event_summary(run_id).last_event_at_unix;

		Ok(match (last_activity, last_event) {
			(Some(run_activity), Some(event_activity)) => Some(run_activity.max(event_activity)),
			(Some(run_activity), None) => Some(run_activity),
			(None, Some(event_activity)) => Some(event_activity),
			(None, None) => None,
		})
	}
}
