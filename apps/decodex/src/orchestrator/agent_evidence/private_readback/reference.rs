use std::path::Path;

use crate::orchestrator::agent_evidence::{AgentPrivateEvidenceRef, OperatorRunStatus};

pub(crate) fn render_private_evidence_reference(run: &OperatorRunStatus) -> String {
	let private_evidence = agent_private_evidence_ref(run);

	format!(
		"ref={} source={} default_view={} read=`{}`",
		private_evidence.evidence_ref,
		private_evidence.source,
		private_evidence.default_view,
		private_evidence.read_command
	)
}

pub(crate) fn agent_private_evidence_ref(run: &OperatorRunStatus) -> AgentPrivateEvidenceRef {
	run.private_evidence.clone()
}

pub(crate) fn private_evidence_ref_for_run_fields(
	project_id: &str,
	project_config_path: &Path,
	issue_id: &str,
	issue_identifier: Option<&str>,
	run_id: &str,
	attempt_number: i64,
) -> AgentPrivateEvidenceRef {
	AgentPrivateEvidenceRef {
		evidence_ref: private_evidence_ref_for_parts(project_id, issue_id, run_id, attempt_number),
		source: String::from("runtime_sqlite"),
		default_view: String::from("summarized_payloads"),
		read_command: private_evidence_read_command(
			project_config_path,
			issue_identifier.unwrap_or(issue_id),
			Some(run_id),
			Some(attempt_number),
			true,
			false,
		),
	}
}

pub(crate) fn private_evidence_ref_for_parts(
	project_id: &str,
	issue_id: &str,
	run_id: &str,
	attempt_number: i64,
) -> String {
	format!("private-evidence:{project_id}/{issue_id}/{run_id}/{attempt_number}")
}

pub(crate) fn private_evidence_read_command(
	project_config_path: &Path,
	issue_selector: &str,
	run_id: Option<&str>,
	attempt_number: Option<i64>,
	json: bool,
	include_payload: bool,
) -> String {
	let mut command = format!(
		"decodex evidence --config {} {}",
		shell_quote(&project_config_path.display().to_string()),
		shell_quote(issue_selector)
	);

	if let Some(run_id) = run_id {
		command.push_str(&format!(" --run-id {}", shell_quote(run_id)));
	}
	if let Some(attempt_number) = attempt_number {
		command.push_str(&format!(" --attempt {attempt_number}"));
	}

	if json {
		command.push_str(" --json");
	}
	if include_payload {
		command.push_str(" --include-payload");
	}

	command
}

fn shell_quote(raw: &str) -> String {
	if !raw.is_empty()
		&& raw.bytes().all(|byte| {
			byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'/' | b':')
		}) {
		return raw.to_owned();
	}

	format!("'{}'", raw.replace('\'', "'\\''"))
}
