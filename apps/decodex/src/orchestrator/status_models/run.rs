pub(in crate::orchestrator) struct OperatorRunTiming {
	pub(in crate::orchestrator) process_id: Option<u32>,
	pub(in crate::orchestrator) process_alive: Option<bool>,
	pub(in crate::orchestrator) process_liveness_reason: Option<String>,
	pub(in crate::orchestrator) last_run_activity_unix_epoch: Option<i64>,
	pub(in crate::orchestrator) last_protocol_activity_unix_epoch: Option<i64>,
	pub(in crate::orchestrator) last_progress_unix_epoch: Option<i64>,
	pub(in crate::orchestrator) idle_for_seconds: Option<i64>,
	pub(in crate::orchestrator) protocol_idle_for_seconds: Option<i64>,
}

#[derive(Clone, Copy)]
pub(in crate::orchestrator) struct MarkerProcessLiveness {
	pub(in crate::orchestrator) alive: bool,
	pub(in crate::orchestrator) reason: &'static str,
}

pub(in crate::orchestrator) struct OperatorRunAppServerState {
	pub(in crate::orchestrator) thread_id: Option<String>,
	pub(in crate::orchestrator) turn_id: Option<String>,
	pub(in crate::orchestrator) thread_status: Option<String>,
	pub(in crate::orchestrator) thread_active_flags: Vec<String>,
	pub(in crate::orchestrator) interactive_requested: bool,
	pub(in crate::orchestrator) continuation_pending: bool,
	pub(in crate::orchestrator) effective_model: Option<String>,
	pub(in crate::orchestrator) effective_model_provider: Option<String>,
	pub(in crate::orchestrator) effective_cwd: Option<String>,
	pub(in crate::orchestrator) effective_approval_policy: Option<String>,
	pub(in crate::orchestrator) effective_approvals_reviewer: Option<String>,
	pub(in crate::orchestrator) effective_sandbox_mode: Option<String>,
}

pub(in crate::orchestrator) struct OperatorRunProtocolSummary {
	pub(in crate::orchestrator) last_event_type: Option<String>,
	pub(in crate::orchestrator) last_event_at: Option<String>,
	pub(in crate::orchestrator) event_count: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::orchestrator) struct OperatorTerminalFinalizeProjection {
	pub(in crate::orchestrator) status: &'static str,
	pub(in crate::orchestrator) phase: &'static str,
	pub(in crate::orchestrator) wait_reason: &'static str,
	pub(in crate::orchestrator) current_operation: &'static str,
}

pub(in crate::orchestrator) struct OperatorRunLifecycleProjection {
	pub(in crate::orchestrator) status: String,
	pub(in crate::orchestrator) status_projection_reason: Option<String>,
	pub(in crate::orchestrator) phase: String,
	pub(in crate::orchestrator) wait_reason: Option<String>,
	pub(in crate::orchestrator) current_operation: String,
	pub(in crate::orchestrator) suspected_stall: bool,
	pub(in crate::orchestrator) execution_liveness: String,
	pub(in crate::orchestrator) run_lease: bool,
	pub(in crate::orchestrator) retry_kind: Option<String>,
	pub(in crate::orchestrator) retry_ready_at_unix_epoch: Option<i64>,
}
pub(in crate::orchestrator) struct OperatorLaneControlProjection {
	pub(in crate::orchestrator) ownership_state: String,
	pub(in crate::orchestrator) liveness_state: String,
	pub(in crate::orchestrator) policy_state: String,
	pub(in crate::orchestrator) terminalization_state: String,
	pub(in crate::orchestrator) next_action: String,
	pub(in crate::orchestrator) conditions: Vec<String>,
}
