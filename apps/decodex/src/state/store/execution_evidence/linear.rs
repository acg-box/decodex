use crate::{
	prelude::{Result, eyre},
	state::{
		runtime_records::LinearExecutionEventRuntimeRecord,
		store::{self, StateStore},
	},
	tracker::records::{self, LinearExecutionEventRecord},
};

impl StateStore {
	/// Persist a locally known Linear execution event in the runtime store.
	pub(crate) fn record_linear_execution_event(
		&self,
		record: &LinearExecutionEventRecord,
	) -> Result<bool> {
		records::validate_linear_execution_event_record(record)
			.map_err(|error| eyre::eyre!(error))?;

		let now = store::timestamp_parts();
		let idempotency_key = record.idempotency_key.clone();
		let mut state = self.lock_without_refresh()?;

		if state.linear_execution_events.contains_key(&idempotency_key) {
			return Ok(false);
		}

		let runtime_record = LinearExecutionEventRuntimeRecord {
			record: record.clone(),
			event_unix: store::parse_linear_execution_event_unix(record),
			recorded_at: now.text,
			recorded_at_unix: now.unix,
		};
		let is_new = self.insert_linear_execution_event_if_absent_locked(&runtime_record)?;

		if is_new {
			state.linear_execution_events.insert(idempotency_key, runtime_record);
		}

		Ok(is_new)
	}

	pub(crate) fn forget_linear_execution_event(&self, idempotency_key: &str) -> Result<()> {
		let mut state = self.lock_without_refresh()?;

		state.linear_execution_events.remove(idempotency_key);

		self.delete_linear_execution_event_locked(idempotency_key)
	}

	/// List locally cached Linear execution events for one issue lane.
	pub(crate) fn list_linear_execution_events(
		&self,
		service_id: &str,
		issue_id: &str,
	) -> Result<Vec<LinearExecutionEventRecord>> {
		let mut records = match self.list_persisted_linear_execution_events(service_id, issue_id)? {
			Some(records) => records,
			None => {
				let state = self.lock_without_refresh()?;

				state
					.linear_execution_events
					.values()
					.filter(|record| {
						record.record.service_id == service_id && record.record.issue_id == issue_id
					})
					.cloned()
					.collect::<Vec<_>>()
			},
		};

		records.sort_by(store::compare_linear_execution_event_runtime_records);

		Ok(records.into_iter().map(|record| record.record).collect())
	}
}
