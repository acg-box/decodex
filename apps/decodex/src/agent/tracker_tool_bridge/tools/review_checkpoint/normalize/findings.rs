use sha2::{Digest as _, Sha256};

use crate::agent::tracker_tool_bridge::{
	self, ISSUE_REVIEW_CHECKPOINT_TOOL_NAME, NormalizedRejectedReviewCheckpointFinding,
	NormalizedReviewCheckpointFinding, ReviewCheckpointFindingArgs, ReviewCheckpointLineRangeArgs,
	ReviewCheckpointRejectedFindingArgs, ReviewPolicyPhase,
	tools::review_checkpoint::normalize::shared,
};

pub(in crate::agent::tracker_tool_bridge::tools::review_checkpoint) fn normalize_review_checkpoint_finding(
	finding: ReviewCheckpointFindingArgs,
	review_policy_phase: ReviewPolicyPhase,
) -> Result<NormalizedReviewCheckpointFinding, String> {
	let severity =
		shared::normalize_review_severity(finding.severity, "accepted_findings.severity")?;
	let summary =
		shared::normalize_required_review_text(finding.summary, "accepted_findings.summary")?;
	let guidance =
		shared::normalize_required_review_text(finding.guidance, "accepted_findings.guidance")?;
	let kind = normalize_optional_review_kind(finding.kind, "accepted_findings.kind")?
		.unwrap_or_else(|| String::from("accepted_finding"));
	let file = normalize_optional_review_file(finding.file)?;
	let line = normalize_optional_review_line(finding.line)?;
	let line_range = normalize_optional_review_line_range(
		line,
		finding.line_range,
		"accepted_findings.line_range",
	)?;
	let fingerprint = review_finding_fingerprint(
		review_policy_phase,
		&kind,
		&summary,
		&guidance,
		file.as_deref(),
		line_range.as_ref(),
	);

	Ok(NormalizedReviewCheckpointFinding {
		severity,
		summary,
		evidence: shared::normalize_required_review_evidence_list(
			finding.evidence,
			"accepted_findings.evidence",
		)?,
		kind,
		file,
		line,
		line_range,
		guidance,
		fingerprint,
	})
}

pub(super) fn normalize_rejected_review_checkpoint_finding(
	finding: ReviewCheckpointRejectedFindingArgs,
) -> Result<NormalizedRejectedReviewCheckpointFinding, String> {
	let severity =
		shared::normalize_review_severity(finding.severity, "rejected_findings.severity")?;
	let summary =
		shared::normalize_required_review_text(finding.summary, "rejected_findings.summary")?;
	let rejection_reason = shared::normalize_required_review_text(
		finding.rejection_reason,
		"rejected_findings.rejection_reason",
	)?;
	let kind = normalize_optional_review_kind(finding.kind, "rejected_findings.kind")?
		.unwrap_or_else(|| String::from("rejected_finding"));
	let file = normalize_optional_review_file(finding.file)?;
	let line = normalize_optional_review_line(finding.line)?;
	let line_range = normalize_optional_review_line_range(
		line,
		finding.line_range,
		"rejected_findings.line_range",
	)?;

	Ok(NormalizedRejectedReviewCheckpointFinding {
		severity,
		summary,
		rejection_reason,
		evidence: shared::normalize_required_review_evidence_list(
			finding.evidence,
			"rejected_findings.evidence",
		)?,
		kind,
		file,
		line,
		line_range,
	})
}

fn normalize_optional_review_file(value: Option<String>) -> Result<Option<String>, String> {
	let Some(file) = tracker_tool_bridge::normalize_optional_progress_field(value) else {
		return Ok(None);
	};

	if file.starts_with('/') {
		return Err(String::from(
			"`issue_review_checkpoint` file references must be repository-relative paths.",
		));
	}

	Ok(Some(file))
}

fn normalize_optional_review_line(value: Option<u64>) -> Result<Option<u64>, String> {
	if matches!(value, Some(0)) {
		return Err(String::from(
			"`issue_review_checkpoint` line references must be one-based when supplied.",
		));
	}

	Ok(value)
}

fn normalize_optional_review_line_range(
	line: Option<u64>,
	line_range: Option<ReviewCheckpointLineRangeArgs>,
	field_name: &str,
) -> Result<Option<ReviewCheckpointLineRangeArgs>, String> {
	let Some(line_range) = line_range
		.or_else(|| line.map(|line| ReviewCheckpointLineRangeArgs { start: line, end: line }))
	else {
		return Ok(None);
	};

	if line_range.start == 0 || line_range.end == 0 {
		return Err(format!(
			"`{ISSUE_REVIEW_CHECKPOINT_TOOL_NAME}` requires `{field_name}` to use one-based line numbers."
		));
	}
	if line_range.end < line_range.start {
		return Err(format!(
			"`{ISSUE_REVIEW_CHECKPOINT_TOOL_NAME}` requires `{field_name}.end` to be greater than or equal to `{field_name}.start`."
		));
	}

	if let Some(line) = line
		&& (line < line_range.start || line > line_range.end)
	{
		return Err(format!(
			"`{ISSUE_REVIEW_CHECKPOINT_TOOL_NAME}` requires `line` to fall inside `{field_name}` when both are supplied."
		));
	}

	Ok(Some(line_range))
}

fn normalize_optional_review_kind(
	value: Option<String>,
	field_name: &str,
) -> Result<Option<String>, String> {
	let Some(kind) = tracker_tool_bridge::normalize_optional_progress_field(value) else {
		return Ok(None);
	};
	let kind = kind.to_ascii_lowercase().replace([' ', '-'], "_");
	let mut chars = kind.chars();
	let Some(first) = chars.next() else {
		return Ok(None);
	};

	if !first.is_ascii_lowercase()
		|| !chars.all(|character| {
			character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_'
		}) {
		return Err(format!(
			"`{ISSUE_REVIEW_CHECKPOINT_TOOL_NAME}` requires `{field_name}` to be a public snake_case identifier."
		));
	}

	Ok(Some(kind))
}

fn review_finding_fingerprint(
	review_policy_phase: ReviewPolicyPhase,
	kind: &str,
	title: &str,
	body: &str,
	file: Option<&str>,
	line_range: Option<&ReviewCheckpointLineRangeArgs>,
) -> String {
	let line_range = line_range
		.map_or_else(|| String::from("none"), |range| format!("{}-{}", range.start, range.end));
	let input = [
		("phase", review_policy_phase.as_str()),
		("kind", kind),
		("title", title),
		("body", body),
		("file", file.unwrap_or("none")),
		("line_range", line_range.as_str()),
	]
	.into_iter()
	.map(|(key, value)| format!("{key}={value}"))
	.collect::<Vec<_>>()
	.join("\n");
	let digest = Sha256::digest(input.as_bytes());
	let hash = digest.iter().map(|byte| format!("{byte:02x}")).collect::<String>();

	format!("review_finding:{hash}")
}
