use std::{
	error::Error,
	fmt::{Display, Formatter},
};

use crate::{
	agent::PhaseGoalKind,
	lane_authority::NoEffectiveDeltaFacts,
	orchestrator::RepoGateTrackedRewriteDecision,
};

#[derive(Debug)]
pub(crate) struct ValidationEvidenceFailure {
	reason_code: String,
}
impl ValidationEvidenceFailure {
	pub(crate) fn new(reason_code: impl Into<String>) -> Self {
		Self { reason_code: reason_code.into() }
	}

	pub(crate) fn error_class(&self) -> &'static str {
		"validation_evidence_failed"
	}
}

impl Display for ValidationEvidenceFailure {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		write!(formatter, "Validation evidence failed: {}", self.reason_code)
	}
}

impl Error for ValidationEvidenceFailure {}

pub(crate) struct ValidationEvidence {
	pub(crate) phase: PhaseGoalKind,
	pub(crate) decision: ValidationDecision,
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
	pub(crate) no_effective_delta_operation_id: Option<String>,
	pub(crate) no_effective_delta_facts: Option<NoEffectiveDeltaFacts>,
}
impl ValidationEvidence {
	pub(crate) fn next_action(&self) -> &'static str {
		match self.reason_code {
			"accepted" => "continue to handoff evidence",
			"missing_progress_checkpoint" =>
				"continue validation; progress checkpoint evidence is optional until terminal finalize",
			"stale_progress_checkpoint" =>
				"ignore stale progress evidence or record a current execution checkpoint",
			"no_effective_delta" =>
				"produce an issue-scoped effective delta before completing the phase goal again",
			"non_goal_violation" =>
				"remove or explicitly resolve the non-goal or scope violation before handoff",
			"progress_blockers_present" =>
				"clear recorded progress blockers or route to manual attention before handoff",
			_ => "inspect validation evidence before selecting the next phase",
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ValidationDecision {
	Pass,
	Fail,
}
impl ValidationDecision {
	pub(crate) fn as_str(self) -> &'static str {
		match self {
			Self::Pass => "pass",
			Self::Fail => "fail",
		}
	}
}
