use serde_json::Value;

use crate::{
	prelude::Result,
	state::{
		runtime_records::PrivateExecutionEventRuntimeRecord,
		store::{self, PrivateExecutionEvent, StateStore},
	},
};

impl StateStore {
	/// Append one private execution event to the local runtime evidence ledger.
	pub fn append_private_execution_event(
		&self,
		project_id: &str,
		issue_id: &str,
		run_id: &str,
		attempt_number: i64,
		event_type: &str,
		payload: Value,
	) -> Result<PrivateExecutionEvent> {
		store::validate_private_execution_event_inputs(
			project_id,
			issue_id,
			run_id,
			attempt_number,
			event_type,
		)?;

		let now = store::timestamp_parts();
		let mut state = self.lock_without_refresh()?;
		let mut record = PrivateExecutionEventRuntimeRecord {
			record_id: 0,
			project_id: project_id.to_owned(),
			issue_id: issue_id.to_owned(),
			run_id: run_id.to_owned(),
			attempt_number,
			event_type: event_type.to_owned(),
			payload,
			recorded_at: now.text,
			recorded_at_unix: now.unix,
		};

		record.record_id = match self.insert_private_execution_event_locked(&record)? {
			Some(record_id) => record_id,
			None => state.next_private_execution_event_id()?,
		};

		state.private_execution_events.push(record.clone());

		Ok(record.as_public())
	}

	/// List private execution events for one project/issue/run/attempt tuple.
	pub fn list_private_execution_events(
		&self,
		project_id: &str,
		issue_id: &str,
		run_id: &str,
		attempt_number: i64,
	) -> Result<Vec<PrivateExecutionEvent>> {
		let state = self.lock()?;
		let mut records = state
			.private_execution_events
			.iter()
			.filter(|record| {
				record.project_id == project_id
					&& record.issue_id == issue_id
					&& record.run_id == run_id
					&& record.attempt_number == attempt_number
			})
			.cloned()
			.collect::<Vec<_>>();

		records.sort_by(store::compare_private_execution_event_runtime_records);

		Ok(records.into_iter().map(|record| record.as_public()).collect())
	}

	/// List private execution events for one project/issue tuple.
	pub(crate) fn list_private_execution_events_for_issue(
		&self,
		project_id: &str,
		issue_id: &str,
	) -> Result<Vec<PrivateExecutionEvent>> {
		let state = self.lock()?;
		let mut records = state
			.private_execution_events
			.iter()
			.filter(|record| record.project_id == project_id && record.issue_id == issue_id)
			.cloned()
			.collect::<Vec<_>>();

		records.sort_by(store::compare_private_execution_event_runtime_records);

		Ok(records.into_iter().map(|record| record.as_public()).collect())
	}

	/// List private execution events for one project/run/attempt tuple.
	pub fn list_private_execution_events_for_run_attempt(
		&self,
		project_id: &str,
		run_id: &str,
		attempt_number: i64,
	) -> Result<Vec<PrivateExecutionEvent>> {
		let state = self.lock()?;
		let mut records = state
			.private_execution_events
			.iter()
			.filter(|record| {
				record.project_id == project_id
					&& record.run_id == run_id
					&& record.attempt_number == attempt_number
			})
			.cloned()
			.collect::<Vec<_>>();

		records.sort_by(store::compare_private_execution_event_runtime_records);

		Ok(records.into_iter().map(|record| record.as_public()).collect())
	}
}
