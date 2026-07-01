use serde_json::Value;

use crate::{state::PrivateExecutionEvent, tracker::records::LinearExecutionEventRecord};

#[derive(Clone, Debug)]
pub(in crate::state) struct LinearExecutionEventRuntimeRecord {
	pub(in crate::state) record: LinearExecutionEventRecord,
	pub(in crate::state) event_unix: Option<i64>,
	pub(in crate::state) recorded_at: String,
	pub(in crate::state) recorded_at_unix: i64,
}

#[derive(Clone, Debug)]
pub(in crate::state) struct PrivateExecutionEventRuntimeRecord {
	pub(in crate::state) record_id: i64,
	pub(in crate::state) project_id: String,
	pub(in crate::state) issue_id: String,
	pub(in crate::state) run_id: String,
	pub(in crate::state) attempt_number: i64,
	pub(in crate::state) event_type: String,
	pub(in crate::state) payload: Value,
	pub(in crate::state) recorded_at: String,
	pub(in crate::state) recorded_at_unix: i64,
}
impl PrivateExecutionEventRuntimeRecord {
	pub(in crate::state) fn as_public(&self) -> PrivateExecutionEvent {
		PrivateExecutionEvent {
			record_id: self.record_id,
			project_id: self.project_id.clone(),
			issue_id: self.issue_id.clone(),
			run_id: self.run_id.clone(),
			attempt_number: self.attempt_number,
			event_type: self.event_type.clone(),
			payload: self.payload.clone(),
			recorded_at: self.recorded_at.clone(),
			recorded_at_unix: self.recorded_at_unix,
		}
	}
}
