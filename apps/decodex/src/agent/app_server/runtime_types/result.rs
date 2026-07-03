use crate::agent::app_server::{AppServerCapabilityPreflightReport, PhaseGoalRunStatus};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AppServerThreadArchiveOutcome {
	Archived,
	DiscardedMissingThread,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AppServerRunResult {
	pub(crate) user_agent: String,
	pub(crate) capability_preflight: AppServerCapabilityPreflightReport,
	pub(crate) thread_id: String,
	pub(crate) turn_id: String,
	pub(crate) turn_count: u32,
	pub(crate) event_count: i64,
	pub(crate) final_output: String,
	pub(crate) continuation_pending: bool,
	pub(crate) phase_goal_status: Option<PhaseGoalRunStatus>,
}

pub(crate) struct TurnLoopResult {
	pub(crate) turn_id: String,
	pub(crate) turn_count: u32,
	pub(crate) final_output: String,
	pub(crate) continuation_pending: bool,
	pub(crate) phase_goal_status: Option<PhaseGoalRunStatus>,
}
