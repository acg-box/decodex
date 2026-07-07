use std::fmt::{Display, Formatter};

use crate::orchestrator::git_ops::{RepoGateFailureDiagnostic, RepoGateTrackedRewriteDecision};
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RepoGateFailureDisposition {
	ContinueRepair,
	RetryAfterBackoff,
	NeedsHumanAttention,
}
impl RepoGateFailureDisposition {
	pub(crate) const fn as_str(self) -> &'static str {
		match self {
			Self::ContinueRepair => "continue_repair",
			Self::RetryAfterBackoff => "retry_after_backoff",
			Self::NeedsHumanAttention => "needs_human_attention",
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RepoGateFailureKind {
	CanonicalizeCommandFailed,
	VerifyCommandFailed,
	TrackedRewritesLeft,
	ScopeEnvelopeViolation,
	GitLockContention,
	CommandSpawnFailed,
	CleanlinessCheckFailed,
}
impl RepoGateFailureKind {
	fn error_class(self) -> &'static str {
		match self {
			Self::CanonicalizeCommandFailed => "repo_gate_canonicalize_failed",
			Self::VerifyCommandFailed => "repo_gate_verify_failed",
			Self::TrackedRewritesLeft => "repo_gate_tracked_rewrites_left",
			Self::ScopeEnvelopeViolation => "repo_gate_scope_envelope_violation",
			Self::GitLockContention => "repo_gate_git_lock_contention",
			Self::CommandSpawnFailed => "repo_gate_command_spawn_failed",
			Self::CleanlinessCheckFailed => "repo_gate_cleanliness_check_failed",
		}
	}

	fn disposition(self) -> RepoGateFailureDisposition {
		match self {
			Self::CanonicalizeCommandFailed | Self::VerifyCommandFailed => {
				RepoGateFailureDisposition::ContinueRepair
			},
			Self::TrackedRewritesLeft | Self::ScopeEnvelopeViolation => {
				RepoGateFailureDisposition::NeedsHumanAttention
			},
			Self::GitLockContention => RepoGateFailureDisposition::RetryAfterBackoff,
			Self::CommandSpawnFailed | Self::CleanlinessCheckFailed => {
				RepoGateFailureDisposition::NeedsHumanAttention
			},
		}
	}

	fn retry_next_action(self) -> &'static str {
		match self {
			Self::CanonicalizeCommandFailed => {
				"additional agent repair is required before repo canonicalization can pass; decodex will retry automatically"
			},
			Self::VerifyCommandFailed => {
				"additional agent repair is required before repo verification can pass; decodex will retry automatically"
			},
			Self::TrackedRewritesLeft => {
				"automatic retry is stopped because the repo gate left tracked rewrites after completing; inspect the retained worktree manually"
			},
			Self::ScopeEnvelopeViolation => {
				"automatic retry is stopped because the repo gate wrote files outside the lane scope envelope"
			},
			Self::GitLockContention => {
				"another Git process appears to hold `.git/index.lock`; decodex will wait briefly, refresh lane state, and retry automatically"
			},
			Self::CommandSpawnFailed => {
				"manual repair is required to restore repo-gate command execution"
			},
			Self::CleanlinessCheckFailed => {
				"manual repair is required to restore repo-gate tracked-file inspection"
			},
		}
	}

	fn terminal_next_action(self, recovery_gate: &str) -> String {
		match self {
			Self::CanonicalizeCommandFailed => format!(
				"inspect the worktree, repair the repo canonicalization failure manually, {recovery_gate}"
			),
			Self::VerifyCommandFailed => format!(
				"inspect the worktree, repair the repo verification failure manually, {recovery_gate}"
			),
			Self::TrackedRewritesLeft => format!(
				"inspect the retained worktree, decide whether the tracked rewrites are in scope, then finish validation and PR handoff or reset the patch manually, {recovery_gate}"
			),
			Self::ScopeEnvelopeViolation => format!(
				"inspect the retained worktree and explicitly decide whether to expand lane scope or isolate repo-wide baseline cleanup before retrying, {recovery_gate}"
			),
			Self::GitLockContention => format!(
				"inspect the worktree for an active or stale `.git/index.lock` holder, clear the Git lock contention manually, {recovery_gate}"
			),
			Self::CommandSpawnFailed => format!(
				"inspect the repo-gate runtime in the worktree, restore command execution manually, {recovery_gate}"
			),
			Self::CleanlinessCheckFailed => format!(
				"inspect the repo-gate runtime in the worktree, restore tracked-file cleanliness inspection manually, {recovery_gate}"
			),
		}
	}
}

#[derive(Debug)]
pub(crate) struct RepoGateFailure {
	kind: RepoGateFailureKind,
	message: String,
	diagnostic: Option<RepoGateFailureDiagnostic>,
	tracked_rewrite_decision: Option<RepoGateTrackedRewriteDecision>,
}
impl RepoGateFailure {
	pub(crate) fn new(kind: RepoGateFailureKind, message: String) -> Self {
		Self { kind, message, diagnostic: None, tracked_rewrite_decision: None }
	}

	pub(crate) fn with_diagnostic(mut self, diagnostic: RepoGateFailureDiagnostic) -> Self {
		self.diagnostic = Some(diagnostic);

		self
	}

	pub(crate) fn with_tracked_rewrite_decision(
		mut self,
		decision: RepoGateTrackedRewriteDecision,
	) -> Self {
		self.tracked_rewrite_decision = Some(decision);

		self
	}

	pub(crate) fn error_class(&self) -> &'static str {
		self.kind.error_class()
	}

	pub(crate) fn disposition(&self) -> RepoGateFailureDisposition {
		self.kind.disposition()
	}

	pub(crate) fn retry_next_action(&self) -> &'static str {
		self.kind.retry_next_action()
	}

	pub(crate) fn retry_schedule_kind(&self) -> Option<&'static str> {
		self.kind.retry_schedule_kind()
	}

	pub(crate) fn terminal_next_action(&self, recovery_gate: &str) -> String {
		self.kind.terminal_next_action(recovery_gate)
	}

	pub(crate) fn diagnostic(&self) -> Option<&RepoGateFailureDiagnostic> {
		self.diagnostic.as_ref()
	}

	pub(crate) fn repair_target_detail(&self) -> String {
		self.diagnostic.as_ref().map_or_else(
			|| format!("Repo gate failed with `{}`.", self.error_class()),
			RepoGateFailureDiagnostic::repair_target_detail,
		)
	}

	pub(crate) fn tracked_rewrite_decision(&self) -> Option<&RepoGateTrackedRewriteDecision> {
		self.tracked_rewrite_decision.as_ref()
	}
}
impl std::error::Error for RepoGateFailure {}

impl Display for RepoGateFailure {
	fn fmt(&self, f: &mut Formatter<'_>) -> std::result::Result<(), std::fmt::Error> {
		write!(f, "{}", self.message)
	}
}
