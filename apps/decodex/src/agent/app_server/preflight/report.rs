use crate::agent::app_server::preflight::{
	BTreeMap, Display, Error, Formatter, PREFLIGHT_CHECK_CONFIG, PREFLIGHT_CHECK_MCP,
	PREFLIGHT_CHECK_MODEL, PREFLIGHT_CHECK_MODEL_PROVIDER, PREFLIGHT_CHECK_PLUGINS,
	PREFLIGHT_CHECK_SKILLS, Serialize, fmt::Result,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AppServerCapabilityPreflightStatus {
	Ok,
	Blocked,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum AppServerCapabilityPreflightFailureKind {
	MethodFailed { method: &'static str, error: String, timed_out: bool },
	BlockedState,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct AppServerCapabilityPreflightReport {
	checks: Vec<AppServerCapabilityPreflightCheck>,
}
impl AppServerCapabilityPreflightReport {
	pub(crate) fn new() -> Self {
		Self { checks: Vec::new() }
	}

	#[cfg(test)]
	pub(crate) fn checks(&self) -> &[AppServerCapabilityPreflightCheck] {
		&self.checks
	}

	pub(crate) fn check_count(&self) -> usize {
		self.checks.len()
	}

	pub(crate) fn push_ok(
		&mut self,
		name: &'static str,
		summary: impl Into<String>,
		details: BTreeMap<String, String>,
	) {
		self.checks.push(AppServerCapabilityPreflightCheck {
			name,
			status: AppServerCapabilityPreflightStatus::Ok,
			summary: summary.into(),
			details,
		});
	}

	pub(crate) fn push_blocked(
		&mut self,
		name: &'static str,
		summary: impl Into<String>,
		details: BTreeMap<String, String>,
	) {
		self.checks.push(AppServerCapabilityPreflightCheck {
			name,
			status: AppServerCapabilityPreflightStatus::Blocked,
			summary: summary.into(),
			details,
		});
	}

	pub(crate) fn has_blockers(&self) -> bool {
		self.checks.iter().any(|check| check.status == AppServerCapabilityPreflightStatus::Blocked)
	}

	pub(crate) fn blocker_summary(&self) -> String {
		let blockers = self
			.checks
			.iter()
			.filter(|check| check.status == AppServerCapabilityPreflightStatus::Blocked)
			.map(preflight_check_blocker_summary)
			.collect::<Vec<_>>();

		if blockers.is_empty() { String::from("no blockers recorded") } else { blockers.join("; ") }
	}
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
			check_name_for_method(method),
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
			AppServerCapabilityPreflightFailureKind::MethodFailed { timed_out: true, .. } => {
				"app_server_preflight_timeout"
			},
			AppServerCapabilityPreflightFailureKind::MethodFailed { .. } => {
				"app_server_introspection_method_failed"
			},
			AppServerCapabilityPreflightFailureKind::BlockedState => {
				"app_server_runtime_preflight_failed"
			},
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
			| AppServerCapabilityPreflightFailureKind::BlockedState => {
				String::from("app-server preflight requires operator recovery")
			},
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
				check_name_for_method(method)
			),
			AppServerCapabilityPreflightFailureKind::MethodFailed { method, error, .. } => {
				format!("{}: `{method}` returned {error}", check_name_for_method(method))
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct AppServerCapabilityPreflightCheck {
	pub(crate) name: &'static str,
	pub(crate) status: AppServerCapabilityPreflightStatus,
	pub(crate) summary: String,
	#[serde(skip_serializing_if = "BTreeMap::is_empty")]
	pub(crate) details: BTreeMap<String, String>,
}

pub(crate) fn check_name_for_method(method: &str) -> &'static str {
	match method {
		"config/read" => PREFLIGHT_CHECK_CONFIG,
		"model/list" => PREFLIGHT_CHECK_MODEL,
		"modelProvider/capabilities/read" => PREFLIGHT_CHECK_MODEL_PROVIDER,
		"skills/list" => PREFLIGHT_CHECK_SKILLS,
		"plugin/list" => PREFLIGHT_CHECK_PLUGINS,
		"mcpServerStatus/list" => PREFLIGHT_CHECK_MCP,
		_ => "introspection",
	}
}

fn preflight_check_blocker_summary(check: &AppServerCapabilityPreflightCheck) -> String {
	let first_error_path = check.details.get("first_error_path");
	let first_error = check.details.get("first_error");
	let mut summary = format!("{}: {}", check.name, check.summary);

	if first_error_path.is_some() || first_error.is_some() {
		let path = first_error_path.map_or("unknown", String::as_str);
		let error = first_error.map_or("unknown", String::as_str);

		summary.push_str(" first_error_path=");
		summary.push_str(path);
		summary.push_str("; first_error=");
		summary.push_str(error);
	}

	summary
}
