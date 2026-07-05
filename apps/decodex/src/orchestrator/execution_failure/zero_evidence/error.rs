#[derive(Debug)]
pub(crate) struct AppServerZeroEvidenceStartFailure {
	issue_identifier: String,
	run_id: String,
}
impl AppServerZeroEvidenceStartFailure {
	pub(crate) fn new(issue_identifier: String, run_id: String) -> Self {
		Self { issue_identifier, run_id }
	}

	pub(crate) fn error_class(&self) -> &'static str {
		"app_server_zero_evidence_start_failed"
	}

	pub(crate) fn terminal_next_action(&self, recovery_gate: &str) -> String {
		format!(
			"inspect local app-server startup logs and Decodex account/runtime state for run `{}`, verify `decodex probe stdio://`, restart `decodex serve` if needed, {recovery_gate}",
			self.run_id
		)
	}

	pub(crate) fn retry_next_action(&self) -> String {
		format!(
			"restart the app-server and retry automatically for run `{}`; inspect private startup diagnostics if the retry budget exhausts",
			self.run_id
		)
	}
}

impl std::fmt::Display for AppServerZeroEvidenceStartFailure {
	fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(
			formatter,
			"App-server run `{}` for issue `{}` failed before Decodex recorded a thread, turn, protocol event, or private execution event.",
			self.run_id, self.issue_identifier
		)
	}
}

impl std::error::Error for AppServerZeroEvidenceStartFailure {}
