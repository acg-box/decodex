use std::path::PathBuf;

use crate::state::{RunControlChannel, Value};

#[cfg_attr(not(test), allow(dead_code))]
pub(in crate::state::store_run_control) struct RunControlActionResolution {
	pub(in crate::state::store_run_control) audit_target: RunControlAuditTarget,
	pub(in crate::state::store_run_control) outcome: String,
	pub(in crate::state::store_run_control) reason: String,
	pub(in crate::state::store_run_control) channel: Option<RunControlChannel>,
}

#[derive(Clone)]
#[cfg_attr(not(test), allow(dead_code))]
pub(in crate::state::store_run_control) struct RunControlAuditTarget {
	pub(in crate::state::store_run_control) project_id: String,
	pub(in crate::state::store_run_control) issue_id: String,
	pub(in crate::state::store_run_control) run_id: String,
	pub(in crate::state::store_run_control) attempt_number: i64,
	pub(in crate::state::store_run_control) attempt_status: Option<String>,
	pub(in crate::state::store_run_control) thread_id: Option<String>,
	pub(in crate::state::store_run_control) turn_id: Option<String>,
	pub(in crate::state::store_run_control) source: String,
	pub(in crate::state::store_run_control) action: String,
	pub(in crate::state::store_run_control) timeout_ms: Option<i64>,
	pub(in crate::state::store_run_control) current_thread_id: Option<String>,
	pub(in crate::state::store_run_control) current_turn_id: Option<String>,
	pub(in crate::state::store_run_control) metadata: Option<Value>,
	pub(in crate::state::store_run_control) context: Option<Value>,
	pub(in crate::state::store_run_control) branch_name: Option<String>,
	pub(in crate::state::store_run_control) worktree_path: Option<PathBuf>,
	pub(in crate::state::store_run_control) run_lease: Option<bool>,
	pub(in crate::state::store_run_control) event_count: Option<i64>,
	pub(in crate::state::store_run_control) last_event_type: Option<String>,
	pub(in crate::state::store_run_control) last_event_at: Option<String>,
	pub(in crate::state::store_run_control) channel: Option<RunControlChannel>,
}
