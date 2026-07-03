//! Phase-goal controller, acceptance checks, and continuation recovery.

mod acceptance;
mod controller;
mod events;
mod goal_spec;
mod recovery;
mod validation;

#[cfg(test)] pub(super) use self::recovery::latest_phase_goal_recovery_candidate;
pub(super) use self::{
	acceptance::PhaseAcceptanceCheckFailure,
	controller::{RepoGatePhaseGoalController, build_phase_goal_controller},
	recovery::{
		PhaseGoalRecoveryContinuation, issue_has_blocking_lane_decision_evidence,
		latest_open_issue_phase_goal_before_attempt, maybe_continue_after_phase_goal_recovery,
		recover_phase_goal_continuation,
	},
};
