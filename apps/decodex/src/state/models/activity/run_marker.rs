use crate::state::{
	ChildAgentActivitySummary, CodexAccountActivitySummary, ProtocolActivitySummary,
	RunActivityMarker,
};

impl RunActivityMarker {
	pub(crate) fn run_id(&self) -> &str {
		&self.run_id
	}

	pub(crate) fn attempt_number(&self) -> i64 {
		self.attempt_number
	}

	pub(crate) fn process_id(&self) -> Option<u32> {
		self.process_id
	}

	pub(crate) fn host_boot_id(&self) -> Option<&str> {
		self.host_boot_id.as_deref()
	}

	pub(crate) fn process_start_identity(&self) -> Option<&str> {
		self.process_start_identity.as_deref()
	}

	pub(crate) fn last_activity_unix_epoch(&self) -> Option<i64> {
		self.last_activity_unix_epoch
	}

	pub(crate) fn last_protocol_activity_unix_epoch(&self) -> Option<i64> {
		self.last_protocol_activity_unix_epoch
	}

	pub(crate) fn last_progress_unix_epoch(&self) -> Option<i64> {
		self.last_progress_unix_epoch
	}

	pub(crate) fn current_operation(&self) -> Option<&str> {
		self.current_operation.as_deref()
	}

	pub(crate) fn thread_id(&self) -> Option<&str> {
		self.thread_id.as_deref()
	}

	pub(crate) fn turn_id(&self) -> Option<&str> {
		self.turn_id.as_deref()
	}

	pub(crate) fn thread_status(&self) -> Option<&str> {
		self.thread_status.as_deref()
	}

	pub(crate) fn thread_active_flags(&self) -> &[String] {
		&self.thread_active_flags
	}

	pub(crate) fn event_count(&self) -> i64 {
		self.event_count.unwrap_or(0)
	}

	pub(crate) fn last_event_type(&self) -> Option<&str> {
		self.last_event_type.as_deref()
	}

	pub(crate) fn effective_model(&self) -> Option<&str> {
		self.effective_model.as_deref()
	}

	pub(crate) fn effective_model_provider(&self) -> Option<&str> {
		self.effective_model_provider.as_deref()
	}

	pub(crate) fn effective_cwd(&self) -> Option<&str> {
		self.effective_cwd.as_deref()
	}

	pub(crate) fn effective_approval_policy(&self) -> Option<&str> {
		self.effective_approval_policy.as_deref()
	}

	pub(crate) fn effective_approvals_reviewer(&self) -> Option<&str> {
		self.effective_approvals_reviewer.as_deref()
	}

	pub(crate) fn effective_sandbox_mode(&self) -> Option<&str> {
		self.effective_sandbox_mode.as_deref()
	}

	pub(crate) fn child_agent_activity(&self) -> Option<&ChildAgentActivitySummary> {
		self.child_agent_activity.as_ref()
	}

	pub(crate) fn protocol_activity(&self) -> Option<&ProtocolActivitySummary> {
		self.protocol_activity.as_ref()
	}

	pub(crate) fn account(&self) -> Option<&CodexAccountActivitySummary> {
		self.account.as_ref()
	}

	pub(crate) fn accounts(&self) -> &[CodexAccountActivitySummary] {
		&self.accounts
	}

	pub(crate) fn retry_kind(&self) -> Option<&str> {
		self.retry_kind.as_deref()
	}

	pub(crate) fn retry_ready_at_unix_epoch(&self) -> Option<i64> {
		self.retry_ready_at_unix_epoch
	}

	pub(crate) fn retry_budget_attempt_count(&self) -> Option<i64> {
		self.retry_budget_attempt_count
	}
}
