use std::{
	error::Error,
	fmt::{Display, Formatter},
};

use crate::agent::app_server::PhaseGoalKind;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AppServerPhaseGoalFailureKind {
	Unsupported { method: &'static str },
	MissingTerminalPath { phase: PhaseGoalKind },
}

#[derive(Debug)]
pub(crate) struct AppServerPhaseGoalFailure {
	kind: AppServerPhaseGoalFailureKind,
}
impl AppServerPhaseGoalFailure {
	pub(crate) fn unsupported(method: &'static str) -> Self {
		Self { kind: AppServerPhaseGoalFailureKind::Unsupported { method } }
	}

	#[cfg(test)]
	pub(crate) fn unsupported_for_test(method: &'static str) -> Self {
		Self::unsupported(method)
	}

	pub(crate) fn missing_terminal_path(phase: PhaseGoalKind) -> Self {
		Self { kind: AppServerPhaseGoalFailureKind::MissingTerminalPath { phase } }
	}

	#[cfg(test)]
	pub(crate) fn missing_terminal_path_for_test(phase: PhaseGoalKind) -> Self {
		Self::missing_terminal_path(phase)
	}

	pub(crate) fn is_terminal_path_missing(&self) -> bool {
		matches!(self.kind, AppServerPhaseGoalFailureKind::MissingTerminalPath { .. })
	}

	pub(crate) fn error_class(&self) -> &'static str {
		match self.kind {
			AppServerPhaseGoalFailureKind::Unsupported { .. } =>
				"app_server_phase_goal_unsupported",
			AppServerPhaseGoalFailureKind::MissingTerminalPath { .. } =>
				"phase_goal_terminal_path_missing",
		}
	}

	pub(crate) fn retry_next_action(&self) -> String {
		match self.kind {
			AppServerPhaseGoalFailureKind::Unsupported { method } => format!(
				"select or upgrade to a Codex app-server that supports required phase-goal method `{method}`"
			),
			AppServerPhaseGoalFailureKind::MissingTerminalPath { phase } => format!(
				"decodex will retry `{}` terminal-path recovery automatically; the next attempt must run the required review, handoff, closeout, or manual-attention terminal tool instead of treating phase-goal completion as issue completion",
				phase.as_str()
			),
		}
	}

	pub(crate) fn terminal_next_action(&self, recovery_gate: &str) -> String {
		match self.kind {
			AppServerPhaseGoalFailureKind::Unsupported { method } => format!(
				"select or upgrade to a Codex app-server that supports required phase-goal method `{method}`, confirm with `decodex probe stdio://`, restart `decodex serve`, {recovery_gate}"
			),
			AppServerPhaseGoalFailureKind::MissingTerminalPath { phase } => format!(
				"inspect the retained lane after phase goal `{}` completed without a terminal Decodex path, finish validation/review/handoff or route manual attention, {recovery_gate}",
				phase.as_str()
			),
		}
	}
}

impl Display for AppServerPhaseGoalFailure {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		match self.kind {
			AppServerPhaseGoalFailureKind::Unsupported { method } => {
				write!(
					formatter,
					"Unsupported Codex app-server: required phase-goal method `{method}` is unavailable."
				)
			},
			AppServerPhaseGoalFailureKind::MissingTerminalPath { phase } => write!(
				formatter,
				"Phase goal `{}` completed without a Decodex terminal completion path.",
				phase.as_str()
			),
		}
	}
}

impl Error for AppServerPhaseGoalFailure {}
