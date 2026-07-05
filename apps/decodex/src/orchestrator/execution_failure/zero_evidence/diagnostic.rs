use std::env;

use crate::orchestrator::execution_failure::Report;

pub(super) struct ZeroEvidenceAppServerStartFailureDiagnostic {
	pub(super) source_error_summary: String,
	pub(super) source_error_chain: Vec<String>,
}

pub(crate) fn truncate_private_diagnostic_text(text: &str) -> String {
	const MAX_PRIVATE_DIAGNOSTIC_TEXT_CHARS: usize = 2_000;

	if text.chars().count() <= MAX_PRIVATE_DIAGNOSTIC_TEXT_CHARS {
		return text.to_owned();
	}

	let mut truncated = text.chars().take(MAX_PRIVATE_DIAGNOSTIC_TEXT_CHARS).collect::<String>();

	truncated.push_str("...<truncated>");

	truncated
}

pub(super) fn zero_evidence_app_server_start_failure_diagnostic(
	error: &Report,
) -> ZeroEvidenceAppServerStartFailureDiagnostic {
	let source_error_chain = error
		.chain()
		.map(|cause| sanitize_private_diagnostic_text(&cause.to_string()))
		.collect::<Vec<_>>();
	let source_error_summary = source_error_chain
		.first()
		.cloned()
		.unwrap_or_else(|| String::from("unknown app-server startup failure"));

	ZeroEvidenceAppServerStartFailureDiagnostic { source_error_summary, source_error_chain }
}

fn sanitize_private_diagnostic_text(text: &str) -> String {
	let mut sanitized = text.to_owned();

	for (name, value) in env::vars() {
		if !diagnostic_env_var_name_is_sensitive(&name) || value.len() < 6 {
			continue;
		}

		let replacement = format!("<redacted env:{name}>");

		sanitized = sanitized.replace(&value, &replacement);
	}

	truncate_private_diagnostic_text(&sanitized)
}

fn diagnostic_env_var_name_is_sensitive(name: &str) -> bool {
	let normalized = name.to_ascii_lowercase();

	normalized.contains("token")
		|| normalized.contains("secret")
		|| normalized.contains("password")
		|| normalized.contains("credential")
		|| normalized.contains("api_key")
		|| normalized.contains("apikey")
		|| normalized.ends_with("_pat")
		|| normalized.starts_with("pat_")
		|| normalized.contains("_pat_")
		|| normalized.contains("auth")
}
