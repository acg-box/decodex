use std::{
	collections::BTreeSet,
	error::Error,
	fmt::{Display, Formatter},
	path::Path,
};

use serde_json::Value;

use crate::{
	agent::PhaseGoalKind,
	orchestrator::{self, RepoGateTrackedRewriteDecision},
	state,
};

#[derive(Debug)]
pub(crate) struct PhaseAcceptanceCheckFailure {
	reason_code: String,
}
impl PhaseAcceptanceCheckFailure {
	pub(super) fn new(reason_code: impl Into<String>) -> Self {
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

pub(super) struct PhaseAcceptanceCheck {
	pub(super) phase: PhaseGoalKind,
	pub(super) decision: PhaseAcceptanceDecision,
	pub(super) reason_code: &'static str,
	pub(super) objective_covered: bool,
	pub(super) effective_delta_present: bool,
	pub(super) changed_surfaces: Vec<String>,
	pub(super) non_goal_passed: bool,
	pub(super) validation_passed: bool,
	pub(super) repo_gate_profile: Option<String>,
	pub(super) canonicalize_commands: Vec<String>,
	pub(super) verify_commands: Vec<String>,
	pub(super) repo_gate_tracked_rewrites: Option<RepoGateTrackedRewriteDecision>,
	pub(super) checkpoint_record_id: Option<i64>,
	pub(super) checkpoint_head_sha: Option<String>,
	pub(super) worktree_head_sha: Option<String>,
	pub(super) blocker_count: usize,
}
impl PhaseAcceptanceCheck {
	pub(super) fn next_action(&self) -> &'static str {
		match self.reason_code {
			"accepted" => "continue to handoff evidence",
			"missing_progress_checkpoint" =>
				"record a current-HEAD issue_progress_checkpoint with docs_impact before completing the phase goal again",
			"stale_progress_checkpoint" =>
				"record a fresh issue_progress_checkpoint for the current worktree HEAD before completing the phase goal again",
			"docs_impact_missing" =>
				"record parseable docs_impact in the current-HEAD issue_progress_checkpoint",
			"no_effective_delta" =>
				"produce an issue-scoped effective delta before completing the phase goal again",
			"non_goal_violation" =>
				"remove or explicitly resolve the non-goal or scope violation before handoff",
			"progress_blockers_present" =>
				"clear recorded progress blockers or route to manual attention before handoff",
			_ => "inspect phase_acceptance_check evidence before selecting the next phase",
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PhaseAcceptanceDecision {
	Pass,
	Fail,
}
impl PhaseAcceptanceDecision {
	pub(super) fn as_str(self) -> &'static str {
		match self {
			Self::Pass => "pass",
			Self::Fail => "fail",
		}
	}
}

pub(super) fn phase_acceptance_reason_code(
	checkpoint_present: bool,
	checkpoint_matches_head: bool,
	docs_impact_valid: bool,
	effective_delta_present: bool,
	non_goal_passed: bool,
	blocker_count: usize,
) -> &'static str {
	if !checkpoint_present {
		return "missing_progress_checkpoint";
	}
	if !checkpoint_matches_head {
		return "stale_progress_checkpoint";
	}
	if !docs_impact_valid {
		return "docs_impact_missing";
	}
	if !effective_delta_present {
		return "no_effective_delta";
	}
	if !non_goal_passed {
		return "non_goal_violation";
	}
	if blocker_count > 0 {
		return "progress_blockers_present";
	}

	"accepted"
}

pub(super) fn phase_acceptance_repair_phase(phase: PhaseGoalKind) -> PhaseGoalKind {
	match phase {
		PhaseGoalKind::RepairAcceptedReviewFindings => PhaseGoalKind::RepairAcceptedReviewFindings,
		PhaseGoalKind::ImplementToValidationReady
		| PhaseGoalKind::RepairValidationFailures
		| PhaseGoalKind::ReviewRepairEvidence
		| PhaseGoalKind::HandoffEvidence => PhaseGoalKind::RepairValidationFailures,
	}
}

pub(super) fn phase_validation_pass_next_phase(phase: PhaseGoalKind) -> PhaseGoalKind {
	match phase {
		PhaseGoalKind::RepairAcceptedReviewFindings => PhaseGoalKind::ReviewRepairEvidence,
		PhaseGoalKind::ImplementToValidationReady | PhaseGoalKind::RepairValidationFailures =>
			PhaseGoalKind::HandoffEvidence,
		PhaseGoalKind::ReviewRepairEvidence | PhaseGoalKind::HandoffEvidence => phase,
	}
}

pub(super) fn phase_terminal_goal_complete_signal(phase: PhaseGoalKind) -> &'static str {
	match phase {
		PhaseGoalKind::ReviewRepairEvidence => "review_repair_evidence_goal_complete",
		PhaseGoalKind::HandoffEvidence => "handoff_evidence_goal_complete",
		PhaseGoalKind::ImplementToValidationReady
		| PhaseGoalKind::RepairValidationFailures
		| PhaseGoalKind::RepairAcceptedReviewFindings => "phase_goal_complete",
	}
}

pub(super) fn phase_tracked_rewrite_handoff_detail(
	next_phase: PhaseGoalKind,
	decision: &RepoGateTrackedRewriteDecision,
) -> String {
	let terminal_context = match next_phase {
		PhaseGoalKind::ReviewRepairEvidence => "review repair completion",
		_ => "review handoff",
	};

	format!(
		"Repo gate validation passed after rewriting owned tracked files: {}. Commit these issue-owned gate rewrites with the lane changes before {terminal_context}.",
		decision.files_display()
	)
}

pub(super) fn phase_acceptance_changed_surfaces(worktree_path: &Path) -> Vec<String> {
	let mut surfaces = BTreeSet::new();

	if let Ok(changed_files) = orchestrator::repo_gate_changed_tracked_files(worktree_path) {
		surfaces.extend(changed_files);
	}
	if let Ok(Some(diff_paths)) = orchestrator::git_guardrail_output(
		worktree_path,
		&["diff", "--name-only", "--diff-filter=ACDMRTUXB", "HEAD", "--"],
	) {
		for path in diff_paths.lines().map(str::trim).filter(|path| !path.is_empty()) {
			surfaces.insert(path.to_owned());
		}
	}
	if let Ok(Some(status)) =
		orchestrator::git_guardrail_output(worktree_path, &["status", "--porcelain"])
	{
		for surface in status.lines().filter_map(phase_acceptance_status_surface) {
			surfaces.insert(surface);
		}
	}

	surfaces.into_iter().collect()
}

pub(super) fn phase_acceptance_blocker_count(payload: &Value) -> usize {
	payload.get("blockers").and_then(Value::as_array).map_or(0, Vec::len)
}

pub(super) fn phase_acceptance_docs_impact_valid(value: &str) -> bool {
	matches!(value, "none" | "update_required" | "research_required" | "drift_required")
}

pub(super) fn phase_acceptance_has_non_goal_violation(payload: &Value) -> bool {
	payload
		.get("blockers")
		.and_then(Value::as_array)
		.into_iter()
		.flatten()
		.filter_map(Value::as_str)
		.any(|blocker| {
			let normalized = blocker.to_ascii_lowercase();

			normalized.contains("non-goal")
				|| normalized.contains("non_goal")
				|| normalized.contains("out of scope")
				|| normalized.contains("scope violation")
		})
}

fn phase_acceptance_status_surface(line: &str) -> Option<String> {
	let line = line.trim_end();

	if line.is_empty() || state::is_untracked_decodex_runtime_artifact_status_line(line) {
		return None;
	}

	let path = line.get(3..)?.trim();
	let path = path.rsplit_once(" -> ").map_or(path, |(_, renamed_path)| renamed_path.trim());

	(!path.is_empty()).then(|| path.to_owned())
}
