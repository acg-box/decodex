mod contract;
mod cost_control;

use sha2::{Digest as _, Sha256};

use crate::agent::tracker_tool_bridge::{
	self, ISSUE_REVIEW_CHECKPOINT_TOOL_NAME, LocalRepoDetails,
	NormalizedRejectedReviewCheckpointFinding, NormalizedReviewCheckpointFinding,
	NormalizedReviewCheckpointPayload, NormalizedReviewCostControl, ReviewCheckpointArgs,
	ReviewCheckpointChecksArgs, ReviewCheckpointFindingArgs, ReviewCheckpointHeadBinding,
	ReviewCheckpointLineRangeArgs, ReviewCheckpointRejectedFindingArgs, ReviewPolicyPhase,
	ReviewPolicyStatus,
	tools::review_checkpoint::{
		INDEPENDENT_FRESH_CONTEXT_REVIEWER, REVIEW_ROUTE_CURRENT_BLOCKER,
		REVIEW_ROUTE_SOURCE_ACCEPTED, finding_policy, finding_policy::ReviewFindingPolicyUpdate,
		routes,
	},
};

pub(in crate::agent::tracker_tool_bridge::tools) fn normalize_review_checkpoint_payload(
	parsed: ReviewCheckpointArgs,
	review_policy_phase: ReviewPolicyPhase,
	status: ReviewPolicyStatus,
	head_sha: &str,
	local_repo: &LocalRepoDetails,
) -> Result<NormalizedReviewCheckpointPayload, String> {
	let reviewer = parsed
		.reviewer
		.map(|reviewer| reviewer.trim().to_owned())
		.filter(|reviewer| !reviewer.is_empty())
		.ok_or_else(|| {
			format!(
				"`{ISSUE_REVIEW_CHECKPOINT_TOOL_NAME}` requires `reviewer` set to `{}`.",
				INDEPENDENT_FRESH_CONTEXT_REVIEWER
			)
		})?;

	if reviewer != INDEPENDENT_FRESH_CONTEXT_REVIEWER {
		return Err(format!(
			"`{ISSUE_REVIEW_CHECKPOINT_TOOL_NAME}` reviewer must be `{}`, not `{reviewer}`.",
			INDEPENDENT_FRESH_CONTEXT_REVIEWER
		));
	}

	let review_contract = contract::normalize_review_checkpoint_contract(
		parsed.review_contract.ok_or_else(|| {
			format!("`{ISSUE_REVIEW_CHECKPOINT_TOOL_NAME}` requires `review_contract`.")
		})?,
		review_policy_phase,
	)?;
	let review_contract_hash = contract::review_checkpoint_contract_hash(&review_contract)?;
	let review_cost_control =
		cost_control::normalize_review_cost_control(parsed.review_cost_control, &review_contract)?;
	let checks = normalize_review_checkpoint_checks(
		parsed
			.checks
			.ok_or_else(|| format!("`{ISSUE_REVIEW_CHECKPOINT_TOOL_NAME}` requires `checks`."))?,
	)?;
	let evidence = normalize_required_review_evidence_list(parsed.evidence, "evidence")?;
	let accepted_findings = parsed
		.accepted_findings
		.into_iter()
		.map(|finding| normalize_review_checkpoint_finding(finding, review_policy_phase))
		.collect::<Result<Vec<_>, _>>()?;
	let rejected_findings = parsed
		.rejected_findings
		.into_iter()
		.map(normalize_rejected_review_checkpoint_finding)
		.collect::<Result<Vec<_>, _>>()?;
	let finding_routes = routes::normalize_review_checkpoint_finding_routes(
		parsed.finding_routes,
		&accepted_findings,
		&rejected_findings,
	)?;
	let finding_route_summary = routes::summarize_review_checkpoint_finding_routes(&finding_routes);

	cost_control::validate_review_cost_control_for_checkpoint(
		&review_cost_control,
		review_policy_phase,
		status,
		&review_contract,
		&accepted_findings,
		&finding_routes,
	)?;

	if status == ReviewPolicyStatus::Findings
		&& !routes::current_review_blocker_routes(&finding_routes).any(|route| {
			route.finding_source == REVIEW_ROUTE_SOURCE_ACCEPTED
				&& route.finding_fingerprint.is_some()
		}) {
		return Err(String::from(
			"`issue_review_checkpoint` status `findings` requires at least one accepted finding routed as `current_blocker`. Route non-current comments through `finding_routes` and use `clean` when no current repair remains.",
		));
	}
	if status == ReviewPolicyStatus::Clean && !accepted_findings.is_empty() {
		return Err(String::from(
			"`issue_review_checkpoint` status `clean` cannot include accepted findings. Reject non-actionable comments explicitly or use status `findings` for accepted repair work.",
		));
	}
	if status == ReviewPolicyStatus::Clean
		&& finding_routes.iter().any(|route| {
			route.route == REVIEW_ROUTE_CURRENT_BLOCKER
				|| routes::review_route_blocks_landing(route)
		}) {
		return Err(String::from(
			"`issue_review_checkpoint` status `clean` can record only non-blocking `finding_routes` such as `follow_up`, `risk_note`, `reviewer_rubric_gap`, or `invalid_or_unsubstantiated`.",
		));
	}
	if matches!(status, ReviewPolicyStatus::Blocked | ReviewPolicyStatus::NeedsArchitectureReview)
		&& !finding_routes.iter().any(routes::review_route_blocks_landing)
	{
		return Err(String::from(
			"`issue_review_checkpoint` status `blocked` or `needs_architecture_review` requires at least one landing-blocking `finding_routes` item with evidence, resolver, and machine-actionable next_action.",
		));
	}

	Ok(NormalizedReviewCheckpointPayload {
		reviewer,
		review_contract,
		review_contract_hash,
		review_cost_control,
		reviewed_head: ReviewCheckpointHeadBinding {
			head_sha: head_sha.to_owned(),
			head_tree_oid: local_repo.head_tree_oid.clone(),
			review_worktree_clean: local_repo.review_worktree_clean(),
		},
		checks,
		evidence,
		accepted_findings,
		rejected_findings,
		finding_routes,
		finding_route_summary,
		finding_policy: finding_policy::empty_review_finding_policy(
			review_policy_phase,
			status,
			head_sha,
		),
	})
}

pub(in crate::agent::tracker_tool_bridge::tools) fn validate_review_cost_control_policy_state(
	cost_control: &NormalizedReviewCostControl,
	policy_update: &ReviewFindingPolicyUpdate,
) -> Result<(), String> {
	cost_control::validate_review_cost_control_policy_state(cost_control, policy_update)
}

pub(super) fn normalize_review_checkpoint_finding(
	finding: ReviewCheckpointFindingArgs,
	review_policy_phase: ReviewPolicyPhase,
) -> Result<NormalizedReviewCheckpointFinding, String> {
	let severity = normalize_review_severity(finding.severity, "accepted_findings.severity")?;
	let summary = normalize_required_review_text(finding.summary, "accepted_findings.summary")?;
	let guidance = normalize_required_review_text(finding.guidance, "accepted_findings.guidance")?;
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
		evidence: normalize_required_review_evidence_list(
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

pub(super) fn normalize_review_severity(
	severity: String,
	field_name: &str,
) -> Result<String, String> {
	let severity = severity.trim().to_ascii_lowercase();

	match severity.as_str() {
		"critical" | "high" | "medium" | "low" | "info" => Ok(severity),
		other => Err(format!(
			"`{field_name}` must be `critical`, `high`, `medium`, `low`, or `info`, not `{other}`."
		)),
	}
}

pub(super) fn normalize_required_review_text(
	value: String,
	field_name: &str,
) -> Result<String, String> {
	let value = tracker_tool_bridge::normalize_summary(&value);

	if value.is_empty() {
		return Err(format!("`{ISSUE_REVIEW_CHECKPOINT_TOOL_NAME}` requires `{field_name}`."));
	}

	Ok(value)
}

pub(super) fn normalize_required_review_evidence_list(
	values: Vec<String>,
	field_name: &str,
) -> Result<Vec<String>, String> {
	let values = tracker_tool_bridge::normalize_progress_list(values);

	if values.is_empty() {
		return Err(format!("`{ISSUE_REVIEW_CHECKPOINT_TOOL_NAME}` requires `{field_name}`."));
	}

	Ok(values)
}

fn normalize_review_checkpoint_checks(
	checks: ReviewCheckpointChecksArgs,
) -> Result<ReviewCheckpointChecksArgs, String> {
	Ok(ReviewCheckpointChecksArgs {
		intended_behavior: normalize_required_review_text(
			checks.intended_behavior,
			"checks.intended_behavior",
		)?,
		regression_risk: normalize_required_review_text(
			checks.regression_risk,
			"checks.regression_risk",
		)?,
		missing_tests: normalize_required_review_text(
			checks.missing_tests,
			"checks.missing_tests",
		)?,
		docs_config_drift: normalize_required_review_text(
			checks.docs_config_drift,
			"checks.docs_config_drift",
		)?,
		migration_fallout: normalize_required_review_text(
			checks.migration_fallout,
			"checks.migration_fallout",
		)?,
		operator_facing_fallout: normalize_required_review_text(
			checks.operator_facing_fallout,
			"checks.operator_facing_fallout",
		)?,
		loop_decision_contract: normalize_required_review_text(
			checks.loop_decision_contract,
			"checks.loop_decision_contract",
		)?,
	})
}

fn normalize_rejected_review_checkpoint_finding(
	finding: ReviewCheckpointRejectedFindingArgs,
) -> Result<NormalizedRejectedReviewCheckpointFinding, String> {
	let severity = normalize_review_severity(finding.severity, "rejected_findings.severity")?;
	let summary = normalize_required_review_text(finding.summary, "rejected_findings.summary")?;
	let rejection_reason = normalize_required_review_text(
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
		evidence: normalize_required_review_evidence_list(
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
