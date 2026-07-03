/// Project-scoped external connector backoff retained in the runtime store.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConnectorBackoff {
	pub(in crate::state) project_id: String,
	pub(in crate::state) connector: String,
	pub(in crate::state) sync_phase: String,
	pub(in crate::state) quota_class: String,
	pub(in crate::state) reset_unix_epoch: i64,
	pub(in crate::state) reset_source: String,
	pub(in crate::state) warning: String,
	pub(in crate::state) updated_at: String,
	pub(in crate::state) updated_at_unix: i64,
}
#[allow(dead_code)]
impl ConnectorBackoff {
	/// Local project identifier affected by this connector backoff.
	pub fn project_id(&self) -> &str {
		&self.project_id
	}

	/// Connector name, such as `linear`.
	pub fn connector(&self) -> &str {
		&self.connector
	}

	/// Runtime phase that last observed the connector backoff.
	pub fn sync_phase(&self) -> &str {
		&self.sync_phase
	}

	/// Quota class backing the pause.
	pub fn quota_class(&self) -> &str {
		&self.quota_class
	}

	/// Unix epoch when Decodex may retry the connector.
	pub fn reset_unix_epoch(&self) -> i64 {
		self.reset_unix_epoch
	}

	/// Source for the reset time.
	pub fn reset_source(&self) -> &str {
		&self.reset_source
	}

	/// Snapshot warning represented by this backoff.
	pub fn warning(&self) -> &str {
		&self.warning
	}

	/// Timestamp when Decodex stored the backoff.
	pub fn updated_at(&self) -> &str {
		&self.updated_at
	}

	/// Unix timestamp when Decodex stored the backoff.
	pub fn updated_at_unix(&self) -> i64 {
		self.updated_at_unix
	}
}
