#[cfg(test)] use crate::agent::app_server::preflight::BTreeMap;
use crate::agent::app_server::preflight::{
	Display, Error, Formatter,
	fmt::Result,
	report::{check, model::AppServerCapabilityPreflightReport},
};

#[derive(Clone, Debug, Eq, PartialEq)]
enum AppServerCapabilityPreflightFailureKind {
	MethodFailed { method: &'static str, error: String, timed_out: bool },
	BlockedState,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AppServerCapabilityPreflightFailure {
	kind: AppServerCapabilityPreflightFailureKind,
	report: AppServerCapabilityPreflightReport,
}
impl AppServerCapabilityPreflightFailure {
	pub(crate) fn blocked(report: AppServerCapabilityPreflightReport) -> Self {
		Self { kind: AppServerCapabilityPreflightFailureKind::BlockedState, report }
	}

	pub(crate) fn method_failed(
		method: &'static str,
		error: String,
		report: AppServerCapabilityPreflightReport,
	) -> Self {
		Self {
			kind: AppServerCapabilityPreflightFailureKind::MethodFailed {
				method,
				error,
				timed_out: false,
			},
			report,
		}
	}

	pub(crate) fn method_timed_out(
		method: &'static str,
		error: String,
		report: AppServerCapabilityPreflightReport,
	) -> Self {
		Self {
			kind: AppServerCapabilityPreflightFailureKind::MethodFailed {
				method,
				error,
				timed_out: true,
			},
			report,
		}
	}

	#[cfg(test)]
	pub(crate) fn blocked_for_test(check: &'static str, summary: &str) -> Self {
		let mut report = AppServerCapabilityPreflightReport::new();

		report.push_blocked(check, summary, BTreeMap::new());

		Self::blocked(report)
	}

	#[cfg(test)]
	pub(crate) fn blocked_for_test_with_details(
		check: &'static str,
		summary: &str,
		details: BTreeMap<String, String>,
	) -> Self {
		let mut report = AppServerCapabilityPreflightReport::new();

		report.push_blocked(check, summary, details);

		Self::blocked(report)
	}

	#[cfg(test)]
	pub(crate) fn method_timed_out_for_test(method: &'static str, error: String) -> Self {
		let mut report = AppServerCapabilityPreflightReport::new();

		report.push_blocked(
			check::check_name_for_method(method),
			format!("`{method}` timed out."),
			BTreeMap::new(),
		);

		Self::method_timed_out(method, error, report)
	}

	pub(crate) fn error_class(&self) -> &'static str {
		match &self.kind {
			AppServerCapabilityPreflightFailureKind::MethodFailed {
				method: "plugin/list",
				timed_out: true,
				..
			} => "app_server_plugin_list_timeout",
			AppServerCapabilityPreflightFailureKind::MethodFailed { timed_out: true, .. } =>
				"app_server_preflight_timeout",
			AppServerCapabilityPreflightFailureKind::MethodFailed { .. } =>
				"app_server_introspection_method_failed",
			AppServerCapabilityPreflightFailureKind::BlockedState =>
				"app_server_runtime_preflight_failed",
		}
	}

	pub(crate) fn is_retryable_timeout(&self) -> bool {
		matches!(
			self.kind,
			AppServerCapabilityPreflightFailureKind::MethodFailed { timed_out: true, .. }
		)
	}

	pub(crate) fn retry_next_action(&self) -> String {
		match &self.kind {
			AppServerCapabilityPreflightFailureKind::MethodFailed {
				method: "plugin/list",
				timed_out: true,
				..
			} => String::from(
				"decodex will retry app-server preflight automatically; inspect local app_server_preflight_failed evidence for the `plugin/list` timeout and restart `decodex serve` if the retry budget exhausts",
			),
			AppServerCapabilityPreflightFailureKind::MethodFailed {
				method,
				timed_out: true,
				..
			} => format!(
				"decodex will retry app-server preflight automatically; inspect local app_server_preflight_failed evidence for the `{method}` timeout and restart `decodex serve` if the retry budget exhausts"
			),
			AppServerCapabilityPreflightFailureKind::MethodFailed { .. }
			| AppServerCapabilityPreflightFailureKind::BlockedState =>
				String::from("app-server preflight requires operator recovery"),
		}
	}

	pub(crate) fn terminal_next_action(&self, recovery_gate: &str) -> String {
		match &self.kind {
			AppServerCapabilityPreflightFailureKind::MethodFailed {
				method: "plugin/list",
				timed_out: true,
				..
			} => format!(
				"inspect local app_server_preflight_failed evidence for the `plugin/list` timeout, restart `decodex serve` if the app-server is stale, run `decodex probe` to confirm plugin inventory recovers, {recovery_gate}"
			),
			AppServerCapabilityPreflightFailureKind::MethodFailed {
				method,
				timed_out: true,
				..
			} => format!(
				"inspect local app_server_preflight_failed evidence for the `{method}` timeout, restart `decodex serve` if the app-server is stale, run `decodex probe` to confirm app-server preflight recovers, {recovery_gate}"
			),
			AppServerCapabilityPreflightFailureKind::MethodFailed { .. } => format!(
				"inspect the Codex app-server preflight status, repair the local Codex runtime configuration, restart `decodex serve`, {recovery_gate}"
			),
			AppServerCapabilityPreflightFailureKind::BlockedState => {
				let blocker_summary = self.blocker_summary();

				format!(
					"inspect local app_server_preflight_failed evidence for `{blocker_summary}`, repair the local Codex runtime configuration, restart `decodex serve`, {recovery_gate}"
				)
			},
		}
	}

	fn blocker_summary(&self) -> String {
		match &self.kind {
			AppServerCapabilityPreflightFailureKind::MethodFailed {
				method,
				error,
				timed_out: true,
			} => format!(
				"{}: `{method}` timed out during preflight: {error}",
				check::check_name_for_method(method)
			),
			AppServerCapabilityPreflightFailureKind::MethodFailed { method, error, .. } => {
				format!("{}: `{method}` returned {error}", check::check_name_for_method(method))
			},
			AppServerCapabilityPreflightFailureKind::BlockedState => self.report.blocker_summary(),
		}
	}

	#[cfg(test)]
	pub(crate) fn report(&self) -> &AppServerCapabilityPreflightReport {
		&self.report
	}
}

impl Display for AppServerCapabilityPreflightFailure {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> Result {
		write!(formatter, "app_server_preflight_failed: {}", self.blocker_summary())
	}
}

impl Error for AppServerCapabilityPreflightFailure {}
