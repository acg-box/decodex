use serde_json::Value;

/// One private, local-only execution event retained in the runtime SQLite ledger.
#[derive(Clone, Debug, PartialEq)]
pub struct PrivateExecutionEvent {
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
impl PrivateExecutionEvent {
	/// Monotonic local row id assigned by the runtime store.
	pub fn record_id(&self) -> i64 {
		self.record_id
	}

	/// Local project identifier owning the evidence row.
	pub fn project_id(&self) -> &str {
		&self.project_id
	}

	/// Issue identifier for this private evidence row.
	pub fn issue_id(&self) -> &str {
		&self.issue_id
	}

	/// Run identifier for this private evidence row.
	pub fn run_id(&self) -> &str {
		&self.run_id
	}

	/// Attempt number for this private evidence row.
	pub fn attempt_number(&self) -> i64 {
		self.attempt_number
	}

	/// Private event type chosen by the runtime or issue-scoped tool path.
	pub fn event_type(&self) -> &str {
		&self.event_type
	}

	/// Structured JSON payload kept local to the runtime store.
	pub fn payload(&self) -> &Value {
		&self.payload
	}

	/// UTC timestamp when the runtime store recorded this row.
	pub fn recorded_at(&self) -> &str {
		&self.recorded_at
	}

	/// Unix timestamp when the runtime store recorded this row.
	pub fn recorded_at_unix(&self) -> i64 {
		self.recorded_at_unix
	}
}
