//! Phase-goal protocol state, failures, and app-server runtime helpers.

mod failure;
mod model;
mod runtime;

pub(crate) use self::{
	failure::AppServerPhaseGoalFailure,
	model::{
		PhaseGoalController, PhaseGoalKind, PhaseGoalRunStatus, PhaseGoalSpec, PhaseGoalTransition,
	},
	runtime::{
		PhaseGoalRuntime, app_server_method_not_found, clear_thread_phase_goal_best_effort,
		get_thread_phase_goal, initialize_phase_goal_runtime, record_phase_goal_completed,
		set_thread_phase_goal,
	},
};
