use std::{
	error::Error,
	fmt::{Display, Formatter},
};

use crate::{agent::PhaseGoalKind, orchestrator::RepoGateTrackedRewriteDecision};

#[derive(Debug)]
pub(crate) struct PhaseAcceptanceCheckFailure {
	reason_code: String,
}
impl PhaseAcceptanceCheckFailure {
	pub(crate) fn new(reason_code: impl Into<String>) -> Self {
		Self { reason_code: reason_code.into() }
	}

	pub(crate) fn error_class(&self) -> &'static str {
		"phase_acceptance_check_failed"
	}
}

impl Display for PhaseAcceptanceCheckFailure {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		write!(formatter, "Phase acceptance check failed: {}", self.reason_code)
	}
}

impl Error for PhaseAcceptanceCheckFailure {}

pub(crate) struct PhaseAcceptanceCheck {
	pub(crate) phase: PhaseGoalKind,
	pub(crate) decision: PhaseAcceptanceDecision,
	pub(crate) reason_code: &'static str,
	pub(crate) objective_covered: bool,
	pub(crate) effective_delta_present: bool,
	pub(crate) changed_surfaces: Vec<String>,
	pub(crate) non_goal_passed: bool,
	pub(crate) validation_passed: bool,
	pub(crate) repo_gate_profile: Option<String>,
	pub(crate) canonicalize_commands: Vec<String>,
	pub(crate) verify_commands: Vec<String>,
	pub(crate) repo_gate_tracked_rewrites: Option<RepoGateTrackedRewriteDecision>,
	pub(crate) checkpoint_record_id: Option<i64>,
	pub(crate) checkpoint_head_sha: Option<String>,
	pub(crate) worktree_head_sha: Option<String>,
	pub(crate) blocker_count: usize,
}
impl PhaseAcceptanceCheck {
	pub(crate) fn next_action(&self) -> &'static str {
		match self.reason_code {
			"accepted" => "continue to handoff evidence",
			"missing_progress_checkpoint" => {
				"record a current-HEAD issue_progress_checkpoint with docs_impact before completing the phase goal again"
			},
			"stale_progress_checkpoint" => {
				"record a fresh issue_progress_checkpoint for the current worktree HEAD before completing the phase goal again"
			},
			"docs_impact_missing" => {
				"record parseable docs_impact in the current-HEAD issue_progress_checkpoint"
			},
			"no_effective_delta" => {
				"produce an issue-scoped effective delta before completing the phase goal again"
			},
			"non_goal_violation" => {
				"remove or explicitly resolve the non-goal or scope violation before handoff"
			},
			"progress_blockers_present" => {
				"clear recorded progress blockers or route to manual attention before handoff"
			},
			_ => "inspect phase_acceptance_check evidence before selecting the next phase",
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PhaseAcceptanceDecision {
	Pass,
	Fail,
}
impl PhaseAcceptanceDecision {
	pub(crate) fn as_str(self) -> &'static str {
		match self {
			Self::Pass => "pass",
			Self::Fail => "fail",
		}
	}
}
