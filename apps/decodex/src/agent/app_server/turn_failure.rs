use std::{
	error::Error,
	fmt::{Display, Formatter},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AppServerTurnFailure {
	thread_id: String,
	turn_id: Option<String>,
	status: String,
	message: String,
	codex_error_info: Option<String>,
}
impl AppServerTurnFailure {
	pub(crate) fn new(
		thread_id: impl Into<String>,
		turn_id: Option<String>,
		status: impl Into<String>,
		message: impl Into<String>,
		codex_error_info: Option<String>,
	) -> Self {
		Self {
			thread_id: thread_id.into(),
			turn_id,
			status: status.into(),
			message: message.into(),
			codex_error_info,
		}
	}

	pub(super) fn from_system_error(thread_id: &str) -> Self {
		Self::new(
			thread_id,
			None,
			"systemError",
			"thread entered systemError before a turn error was reported",
			None,
		)
	}

	pub(crate) fn requires_operator_attention(&self) -> bool {
		matches!(self.codex_error_info.as_deref(), Some("operatorAttentionRequired"))
	}

	pub(crate) fn should_stop_current_turn(&self) -> bool {
		self.is_retryable_capacity_failure()
	}

	pub(crate) fn is_retryable_capacity_failure(&self) -> bool {
		matches!(self.codex_error_info.as_deref(), Some("usageLimitExceeded"))
	}

	pub(crate) fn error_class(&self) -> &'static str {
		match self.codex_error_info.as_deref() {
			Some("usageLimitExceeded") => "app_server_usage_limit_exceeded",
			_ => "app_server_turn_failed",
		}
	}

	pub(crate) fn retry_next_action(&self) -> &'static str {
		match self.codex_error_info.as_deref() {
			Some("usageLimitExceeded") => {
				"decodex will retry automatically and reselect or refresh the Codex account before the next attempt"
			},
			_ => "decodex will retry automatically",
		}
	}

	pub(crate) fn terminal_next_action(&self, recovery_gate: &str) -> String {
		match self.codex_error_info.as_deref() {
			Some("usageLimitExceeded") => format!(
				"inspect Codex account usage and retry after credits or the usage reset are available, {recovery_gate}"
			),
			_ => format!(
				"inspect the app-server turn error and worktree, resolve the blocker manually, {recovery_gate}"
			),
		}
	}
}
impl Display for AppServerTurnFailure {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		write!(formatter, "Codex app-server turn failed on thread `{}`", self.thread_id)?;

		if let Some(turn_id) = self.turn_id.as_deref() {
			write!(formatter, ", turn `{turn_id}`")?;
		}

		write!(formatter, " with status `{}`: {}", self.status, self.message)?;

		if let Some(codex_error_info) = self.codex_error_info.as_deref() {
			write!(formatter, " ({codex_error_info})")?;
		}

		Ok(())
	}
}

impl Error for AppServerTurnFailure {}
