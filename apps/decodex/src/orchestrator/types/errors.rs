use crate::orchestrator::types::{
	Display, Duration, Error, Formatter, LoopGuardrailReason, fmt::Result,
};

#[derive(Debug)]
pub(crate) struct ManualAttentionRequested {
	pub(crate) issue_identifier: String,
	pub(crate) label: String,
	pub(crate) run_id: String,
	pub(crate) error_class: Option<String>,
}
impl Display for ManualAttentionRequested {
	fn fmt(&self, f: &mut Formatter<'_>) -> Result {
		write!(
			f,
			"Run `{}` for issue `{}` requested human attention via label `{}`; stop automatic retries and hand off manually.",
			self.run_id, self.issue_identifier, self.label
		)
	}
}

impl Error for ManualAttentionRequested {}

#[derive(Debug)]
pub(crate) struct ReviewHandoffNeedsAttention {
	pub(crate) issue_identifier: String,
	pub(crate) pr_url: String,
	pub(crate) run_id: String,
}
impl Display for ReviewHandoffNeedsAttention {
	fn fmt(&self, f: &mut Formatter<'_>) -> Result {
		write!(
			f,
			"Run `{}` for issue `{}` partially applied review handoff writeback for PR `{}`; stop retries and repair the issue manually.",
			self.run_id, self.issue_identifier, self.pr_url
		)
	}
}

impl Error for ReviewHandoffNeedsAttention {}

#[derive(Debug)]
pub(crate) struct RetainedReviewNeedsAttention {
	pub(crate) reason: String,
}
impl Display for RetainedReviewNeedsAttention {
	fn fmt(&self, f: &mut Formatter<'_>) -> Result {
		write!(f, "Retained review orchestration requires operator attention: {}.", self.reason)
	}
}

impl Error for RetainedReviewNeedsAttention {}

#[derive(Debug)]
pub(crate) struct RetainedPartialProgress {
	pub(crate) issue_identifier: String,
	pub(crate) run_id: String,
	pub(crate) worktree_path: String,
	pub(crate) source_error_class: Option<String>,
}
impl Display for RetainedPartialProgress {
	fn fmt(&self, f: &mut Formatter<'_>) -> Result {
		write!(
			f,
			"Run `{}` for issue `{}` retained tracked worktree changes at `{}`; stop automatic retries and finish recovery manually.",
			self.run_id, self.issue_identifier, self.worktree_path
		)
	}
}

impl Error for RetainedPartialProgress {}

#[derive(Clone, Debug)]
pub(crate) struct LoopGuardrailStopRequested {
	pub(crate) issue_identifier: String,
	pub(crate) run_id: String,
	pub(crate) reason: LoopGuardrailReason,
	pub(crate) consecutive_count: i64,
	pub(crate) fingerprint: String,
	pub(crate) source_error_class: Option<String>,
	pub(crate) architecture_recovery_reason_code: Option<String>,
}
impl LoopGuardrailStopRequested {
	pub(crate) fn terminal_error_class(&self) -> &'static str {
		match self.architecture_recovery_reason_code.as_deref() {
			Some("architecture_recovery_exhausted") => "architecture_recovery_exhausted",
			Some("contract_boundary_required") => "contract_boundary_required",
			Some("external_dependency_required") => "external_dependency_required",
			Some("architecture_recovery_started") | None => self.reason.error_class(),
			Some(_) => self.reason.error_class(),
		}
	}

	pub(crate) fn terminal_next_action(&self, recovery_gate: &str) -> String {
		match self.architecture_recovery_reason_code.as_deref() {
			Some("architecture_recovery_exhausted") => format!(
				"inspect the Architecture Recovery Packet and prior recovery attempts; recovery budget is exhausted, {recovery_gate}"
			),
			Some("contract_boundary_required") => format!(
				"inspect the Authority Boundary Check and resolve the Decision Contract or authority evidence before retrying, {recovery_gate}"
			),
			Some("external_dependency_required") => format!(
				"inspect the dependency or Execution Program readiness blocker and resolve that external dependency before retrying, {recovery_gate}"
			),
			Some("architecture_recovery_started") | None => {
				self.reason.terminal_next_action(recovery_gate)
			},
			Some(_) => self.reason.terminal_next_action(recovery_gate),
		}
	}
}

impl Display for LoopGuardrailStopRequested {
	fn fmt(&self, f: &mut Formatter<'_>) -> Result {
		let source = self.source_error_class.as_deref().unwrap_or("none");
		let architecture_recovery =
			self.architecture_recovery_reason_code.as_deref().unwrap_or("none");

		write!(
			f,
			"Run `{}` for issue `{}` hit loop guardrail `{}` after {} consecutive matching observations with source `{}` and fingerprint `{}`; architecture recovery reason `{}`.",
			self.run_id,
			self.issue_identifier,
			self.reason.error_class(),
			self.consecutive_count,
			source,
			self.fingerprint,
			architecture_recovery
		)
	}
}

impl Error for LoopGuardrailStopRequested {}

#[derive(Debug)]
pub(crate) struct AgentGitCredentialsUnavailable {
	pub(crate) run_id: String,
	pub(crate) token_env_var: String,
}
impl Display for AgentGitCredentialsUnavailable {
	fn fmt(&self, f: &mut Formatter<'_>) -> Result {
		write!(
			f,
			"Run `{}` could not prepare noninteractive GitHub credentials from `{}`; stop automatic execution and repair the configured credential.",
			self.run_id, self.token_env_var
		)
	}
}

impl Error for AgentGitCredentialsUnavailable {}

#[derive(Debug)]
pub(crate) struct StalledRunNeedsAttention {
	pub(crate) issue_identifier: String,
	pub(crate) run_id: String,
	pub(crate) idle_for: Duration,
}
impl Display for StalledRunNeedsAttention {
	fn fmt(&self, f: &mut Formatter<'_>) -> Result {
		write!(
			f,
			"Run `{}` for issue `{}` stalled after {:?} without app-server activity; reconcile through the retry budget before requiring operator attention.",
			self.run_id, self.issue_identifier, self.idle_for
		)
	}
}

impl Error for StalledRunNeedsAttention {}
