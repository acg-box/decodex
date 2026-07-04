mod attempts;
mod channels;
mod leases;

pub use self::{attempts::RunAttempt, channels::RunControlChannel, leases::IssueLease};

use serde_json::Value;

/// Local run-control request resolution and first audit row.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunControlActionReceipt {
	pub(in crate::state) project_id: String,
	pub(in crate::state) issue_id: String,
	pub(in crate::state) run_id: String,
	pub(in crate::state) attempt_number: i64,
	pub(in crate::state) thread_id: Option<String>,
	pub(in crate::state) turn_id: Option<String>,
	pub(in crate::state) current_thread_id: Option<String>,
	pub(in crate::state) current_turn_id: Option<String>,
	pub(in crate::state) source: String,
	pub(in crate::state) action: String,
	pub(in crate::state) outcome: String,
	pub(in crate::state) reason: String,
	pub(in crate::state) audit_record_id: i64,
	pub(in crate::state) metadata: Option<Value>,
	pub(in crate::state) context: Option<Value>,
	pub(in crate::state) channel: Option<RunControlChannel>,
}
#[allow(dead_code)]
impl RunControlActionReceipt {
	/// Project identifier used for the local audit scope.
	pub fn project_id(&self) -> &str {
		&self.project_id
	}

	/// Issue identifier used for the local audit scope.
	pub fn issue_id(&self) -> &str {
		&self.issue_id
	}

	/// Run identifier used for the local audit scope.
	pub fn run_id(&self) -> &str {
		&self.run_id
	}

	/// Attempt number used for the local audit scope.
	pub fn attempt_number(&self) -> i64 {
		self.attempt_number
	}

	/// Requested thread identifier, when supplied.
	pub fn thread_id(&self) -> Option<&str> {
		self.thread_id.as_deref()
	}

	/// Requested turn identifier, when supplied.
	pub fn turn_id(&self) -> Option<&str> {
		self.turn_id.as_deref()
	}

	/// Current thread identifier observed while resolving the request.
	pub fn current_thread_id(&self) -> Option<&str> {
		self.current_thread_id.as_deref()
	}

	/// Current turn identifier observed while resolving the request.
	pub fn current_turn_id(&self) -> Option<&str> {
		self.current_turn_id.as_deref()
	}

	/// Local source that requested the action.
	pub fn source(&self) -> &str {
		&self.source
	}

	/// Requested control action.
	pub fn action(&self) -> &str {
		&self.action
	}

	/// Normalized audit outcome for the request resolution.
	pub fn outcome(&self) -> &str {
		&self.outcome
	}

	/// Normalized reason for the request resolution.
	pub fn reason(&self) -> &str {
		&self.reason
	}

	/// Private execution event row id for the request-resolution audit.
	pub fn audit_record_id(&self) -> i64 {
		self.audit_record_id
	}

	/// Optional compact action metadata captured with the audit event.
	pub fn metadata(&self) -> Option<&Value> {
		self.metadata.as_ref()
	}

	/// Optional compact lane context captured with the audit event.
	pub fn context(&self) -> Option<&Value> {
		self.context.as_ref()
	}

	/// Control channel selected for an accepted request.
	pub fn channel(&self) -> Option<&RunControlChannel> {
		self.channel.as_ref()
	}
}

/// Unix file-descriptor handoff for a daemon-planned lease adopted by a child process.
pub struct PreacquiredLeaseGuards {
	/// The inherited issue-claim lock fd that keeps one issue single-owned across processes.
	pub issue_claim_fd: i32,
	/// The inherited dispatch-slot lock fd used for shared handoff bookkeeping.
	pub dispatch_slot_fd: i32,
	/// The inherited shared dispatch-slot index used for local guard bookkeeping.
	pub dispatch_slot_index: usize,
}

/// Foundation request for resolving a local run-control action.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct RunControlActionRequest<'a> {
	/// Requested project identifier.
	pub(crate) project_id: &'a str,
	/// Requested issue identifier.
	pub(crate) issue_id: &'a str,
	/// Requested run identifier.
	pub(crate) run_id: &'a str,
	/// Requested attempt number.
	pub(crate) attempt_number: i64,
	/// Requested app-server thread identifier, when known.
	pub(crate) thread_id: Option<&'a str>,
	/// Requested current app-server turn identifier, when known.
	pub(crate) turn_id: Option<&'a str>,
	/// Local source that requested the action.
	pub(crate) source: &'a str,
	/// Requested control action.
	pub(crate) action: &'a str,
	/// Optional caller timeout budget in milliseconds.
	pub(crate) timeout_ms: Option<i64>,
	/// Optional compact, non-secret action metadata to include in audit evidence.
	pub(crate) metadata: Option<&'a Value>,
	/// Optional compact lane context to include in audit evidence.
	pub(crate) context: Option<&'a Value>,
}

/// Follow-up outcome for a run-control action handled after initial resolution.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct RunControlActionOutcomeRequest<'a> {
	/// Project identifier used for local audit scoping.
	pub(crate) project_id: &'a str,
	/// Issue identifier used for local audit scoping.
	pub(crate) issue_id: &'a str,
	/// Run identifier used for local audit scoping.
	pub(crate) run_id: &'a str,
	/// Attempt number used for local audit scoping.
	pub(crate) attempt_number: i64,
	/// Requested app-server thread identifier, when known.
	pub(crate) thread_id: Option<&'a str>,
	/// Requested expected app-server turn identifier, when known.
	pub(crate) turn_id: Option<&'a str>,
	/// Current app-server thread identifier observed while handling the request.
	pub(crate) current_thread_id: Option<&'a str>,
	/// Current app-server turn identifier observed while handling the request.
	pub(crate) current_turn_id: Option<&'a str>,
	/// Local source that requested the action.
	pub(crate) source: &'a str,
	/// Requested control action.
	pub(crate) action: &'a str,
	/// Follow-up outcome.
	pub(crate) outcome: &'a str,
	/// Normalized outcome reason.
	pub(crate) reason: &'a str,
	/// Parent request-resolution audit record id, when known.
	pub(crate) parent_record_id: Option<i64>,
	/// Optional caller timeout budget in milliseconds.
	pub(crate) timeout_ms: Option<i64>,
	/// Optional compact, non-secret action metadata to include in audit evidence.
	pub(crate) metadata: Option<&'a Value>,
	/// Control channel that carried the request, when known.
	pub(crate) channel: Option<&'a RunControlChannel>,
}
