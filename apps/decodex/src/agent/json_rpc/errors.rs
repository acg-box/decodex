use std::{
	error::Error,
	fmt::{Display, Formatter},
};

#[derive(Debug)]
pub(crate) struct AppServerHomePreflightFailure {
	details: String,
	kind: AppServerHomePreflightFailureKind,
}
impl AppServerHomePreflightFailure {
	pub(crate) fn resolution_failed(details: String) -> Self {
		Self { details, kind: AppServerHomePreflightFailureKind::ResolutionFailed }
	}

	pub(crate) fn initialize_mismatch(resolved_home: String, expected_home: String) -> Self {
		Self {
			details: format!(
				"app_server_protocol_failure: initialize codexHome `{resolved_home}` did not match expected shared Codex home `{expected_home}`; Decodex blocked dispatch before thread/start so Codex state is not split across homes."
			),
			kind: AppServerHomePreflightFailureKind::InitializeMismatch,
		}
	}

	pub(crate) fn error_class(&self) -> &'static str {
		match self.kind {
			AppServerHomePreflightFailureKind::ResolutionFailed => {
				"app_server_codex_home_preflight_failed"
			},
			AppServerHomePreflightFailureKind::InitializeMismatch => {
				"app_server_codex_home_mismatch"
			},
		}
	}

	pub(crate) fn terminal_next_action(&self, recovery_gate: &str) -> String {
		format!(
			"inspect the local Decodex and Codex home sharing, restart `decodex serve`, {recovery_gate}"
		)
	}
}

impl Display for AppServerHomePreflightFailure {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str(&self.details)
	}
}

impl Error for AppServerHomePreflightFailure {}

#[derive(Debug)]
pub(crate) struct AppServerOutputTimeout;
impl Display for AppServerOutputTimeout {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str("Timed out while waiting for app-server output.")
	}
}

impl Error for AppServerOutputTimeout {}

#[derive(Debug)]
pub(crate) struct AppServerTransportFailure {
	details: String,
	phase: Option<&'static str>,
	retryable_startup: bool,
}
impl AppServerTransportFailure {
	pub(crate) fn new(details: String) -> Self {
		Self { details, phase: None, retryable_startup: false }
	}

	pub(crate) fn with_phase(
		details: String,
		phase: &'static str,
		retryable_startup: bool,
	) -> Self {
		Self { details, phase: Some(phase), retryable_startup }
	}

	pub(crate) fn error_class(&self) -> &'static str {
		"app_server_transport_disconnected"
	}

	pub(crate) fn is_retryable_startup(&self) -> bool {
		self.retryable_startup
	}

	pub(crate) fn retry_next_action(&self) -> String {
		if let Some(phase) = self.phase {
			format!(
				"app-server transport disconnected during `{phase}` before a durable turn was running; decodex will restart the app-server and retry automatically"
			)
		} else {
			String::from("decodex will retry the app-server transport failure automatically")
		}
	}

	pub(crate) fn terminal_next_action(&self, recovery_gate: &str) -> String {
		let phase = self.phase.map_or_else(String::new, |phase| format!(" during `{phase}`"));

		format!(
			"inspect the local app-server stderr tail and process exit status, resolve the Codex app-server transport failure{phase} manually, {recovery_gate}"
		)
	}
}

impl Display for AppServerTransportFailure {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str(&self.details)
	}
}

impl Error for AppServerTransportFailure {}

#[derive(Debug)]
enum AppServerHomePreflightFailureKind {
	ResolutionFailed,
	InitializeMismatch,
}
