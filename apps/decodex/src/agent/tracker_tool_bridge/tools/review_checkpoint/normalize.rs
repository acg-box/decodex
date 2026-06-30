use serde_json;
use sha2::{Digest as _, Sha256};

use crate::{
	agent::tracker_tool_bridge::{
		self, ISSUE_REVIEW_CHECKPOINT_TOOL_NAME, LocalRepoDetails,
		NormalizedRejectedReviewCheckpointFinding, NormalizedReviewCheckpointContract,
		NormalizedReviewCheckpointFinding, NormalizedReviewCheckpointFindingRoute,
		NormalizedReviewCheckpointPayload, NormalizedReviewCostControl, ReviewCheckpointArgs,
		ReviewCheckpointChecksArgs, ReviewCheckpointContractArgs, ReviewCheckpointFindingArgs,
		ReviewCheckpointHeadBinding, ReviewCheckpointLineRangeArgs,
		ReviewCheckpointRejectedFindingArgs, ReviewCostControlArgs, ReviewPolicyPhase,
		ReviewPolicyStatus,
	},
	tracker::public_text,
};

use super::{
	INDEPENDENT_FRESH_CONTEXT_REVIEWER, MAX_COMPACT_REVIEW_CHANGED_SURFACE_COUNT,
	REVIEW_CLASS_COMPACT_CURRENT_HEAD, REVIEW_CLASS_FULL_CURRENT_HEAD,
	REVIEW_COST_CONTROL_NOT_PROVIDED, REVIEW_ROUTE_CURRENT_BLOCKER, REVIEW_ROUTE_SOURCE_ACCEPTED,
	finding_policy::{ReviewFindingPolicyUpdate, empty_review_finding_policy},
	routes::{
		current_review_blocker_routes, normalize_review_checkpoint_finding_routes,
		review_route_blocks_landing, summarize_review_checkpoint_finding_routes,
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
				"`{ISSUE_REVIEW_CHECKPOINT_TOOL_NAME}` requires `reviewer` set to `{INDEPENDENT_FRESH_CONTEXT_REVIEWER}`."
			)
		})?;

	if reviewer != INDEPENDENT_FRESH_CONTEXT_REVIEWER {
		return Err(format!(
			"`{ISSUE_REVIEW_CHECKPOINT_TOOL_NAME}` reviewer must be `{INDEPENDENT_FRESH_CONTEXT_REVIEWER}`, not `{reviewer}`."
		));
	}

	let review_contract = normalize_review_checkpoint_contract(
		parsed.review_contract.ok_or_else(|| {
			format!("`{ISSUE_REVIEW_CHECKPOINT_TOOL_NAME}` requires `review_contract`.")
		})?,
		review_policy_phase,
	)?;
	let review_contract_hash = review_checkpoint_contract_hash(&review_contract)?;
	let review_cost_control =
		normalize_review_cost_control(parsed.review_cost_control, &review_contract)?;
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
	let finding_routes = normalize_review_checkpoint_finding_routes(
		parsed.finding_routes,
		&accepted_findings,
		&rejected_findings,
	)?;
	let finding_route_summary = summarize_review_checkpoint_finding_routes(&finding_routes);

	validate_review_cost_control_for_checkpoint(
		&review_cost_control,
		review_policy_phase,
		status,
		&review_contract,
		&accepted_findings,
		&finding_routes,
	)?;

	if status == ReviewPolicyStatus::Findings
		&& !current_review_blocker_routes(&finding_routes).any(|route| {
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
			route.route == REVIEW_ROUTE_CURRENT_BLOCKER || review_route_blocks_landing(route)
		}) {
		return Err(String::from(
			"`issue_review_checkpoint` status `clean` can record only non-blocking `finding_routes` such as `follow_up`, `risk_note`, `reviewer_rubric_gap`, or `invalid_or_unsubstantiated`.",
		));
	}
	if matches!(status, ReviewPolicyStatus::Blocked | ReviewPolicyStatus::NeedsArchitectureReview)
		&& !finding_routes.iter().any(review_route_blocks_landing)
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
		finding_policy: empty_review_finding_policy(review_policy_phase, status, head_sha),
	})
}

fn normalize_review_checkpoint_contract(
	contract: ReviewCheckpointContractArgs,
	review_policy_phase: ReviewPolicyPhase,
) -> Result<NormalizedReviewCheckpointContract, String> {
	let workflow_policy_source = normalize_required_review_text(
		contract.workflow_policy_source,
		"review_contract.workflow_policy_source",
	)?;

	if workflow_policy_source != "registered_project_workflow" {
		return Err(format!(
			"`{ISSUE_REVIEW_CHECKPOINT_TOOL_NAME}` requires `review_contract.workflow_policy_source` to be `registered_project_workflow`, not `{workflow_policy_source}`."
		));
	}

	let review_type = normalize_review_type(contract.review_type, review_policy_phase)?;
	let risk_tier = normalize_review_risk_tier(contract.risk_tier)?;
	let objective =
		normalize_required_review_text(contract.objective, "review_contract.objective")?;
	let scope = normalize_required_review_contract_list(contract.scope, "review_contract.scope")?;
	let non_goals =
		normalize_required_review_contract_list(contract.non_goals, "review_contract.non_goals")?;
	let required_checks = normalize_required_review_contract_list(
		contract.required_checks,
		"review_contract.required_checks",
	)?;
	let allowed_expansion_triggers = normalize_required_review_contract_list(
		contract.allowed_expansion_triggers,
		"review_contract.allowed_expansion_triggers",
	)?;
	let validation_evidence = normalize_required_review_contract_list(
		contract.validation_evidence,
		"review_contract.validation_evidence",
	)?;

	Ok(NormalizedReviewCheckpointContract {
		workflow_policy_source,
		review_type,
		risk_tier,
		objective,
		scope,
		non_goals,
		required_checks,
		allowed_expansion_triggers,
		validation_evidence,
	})
}

fn normalize_review_type(
	review_type: String,
	review_policy_phase: ReviewPolicyPhase,
) -> Result<String, String> {
	let review_type = review_type.trim().to_ascii_lowercase().replace([' ', '-'], "_");
	let expected = match review_policy_phase {
		ReviewPolicyPhase::Handoff => "full_current_head_review",
		ReviewPolicyPhase::Repair => "repair_verification",
	};

	if review_type != expected {
		return Err(format!(
			"`{ISSUE_REVIEW_CHECKPOINT_TOOL_NAME}` requires `review_contract.review_type` to be `{expected}` for `{}` review checkpoints, not `{review_type}`.",
			review_policy_phase.as_str()
		));
	}

	Ok(review_type)
}

fn normalize_review_risk_tier(risk_tier: String) -> Result<String, String> {
	let risk_tier = risk_tier.trim().to_ascii_lowercase().replace([' ', '-'], "_");

	match risk_tier.as_str() {
		"low" | "localized" | "high" => Ok(risk_tier),
		other => Err(format!(
			"`{ISSUE_REVIEW_CHECKPOINT_TOOL_NAME}` requires `review_contract.risk_tier` to be `low`, `localized`, or `high`, not `{other}`."
		)),
	}
}

fn normalize_required_review_contract_list(
	values: Vec<String>,
	field_name: &str,
) -> Result<Vec<String>, String> {
	let values = tracker_tool_bridge::normalize_progress_list(values);

	if values.is_empty() {
		return Err(format!("`{ISSUE_REVIEW_CHECKPOINT_TOOL_NAME}` requires `{field_name}`."));
	}

	Ok(values)
}

fn review_checkpoint_contract_hash(
	contract: &NormalizedReviewCheckpointContract,
) -> Result<String, String> {
	let serialized = serde_json::to_vec(contract).map_err(|error| {
		format!(
			"Failed to serialize `review_contract` for `{ISSUE_REVIEW_CHECKPOINT_TOOL_NAME}`: {error}"
		)
	})?;
	let digest = Sha256::digest(serialized);
	let hash = digest.iter().map(|byte| format!("{byte:02x}")).collect::<String>();

	Ok(format!("review_contract:{hash}"))
}

fn normalize_review_cost_control(
	cost_control: Option<ReviewCostControlArgs>,
	review_contract: &NormalizedReviewCheckpointContract,
) -> Result<NormalizedReviewCostControl, String> {
	let Some(cost_control) = cost_control else {
		return Ok(NormalizedReviewCostControl {
			review_class: String::from(REVIEW_CLASS_FULL_CURRENT_HEAD),
			risk_class: review_contract.risk_tier.clone(),
			compact_eligible: false,
			changed_surface_count: 0,
			changed_surface_summary: vec![String::from(
				"Review cost-control metadata was not supplied; standard full review remains required.",
			)],
			high_risk_surfaces: Vec::new(),
			current_head_evidence: false,
			validation_backed: false,
			validation_current: false,
			evidence_sufficient: false,
			reviewer_judgment: String::from(
				"No compact-review judgment was recorded; defaulting to full independent review.",
			),
			fallback_reason: Some(String::from(REVIEW_COST_CONTROL_NOT_PROVIDED)),
		});
	};
	let review_class = normalize_review_class(cost_control.review_class)?;
	let risk_class = normalize_review_risk_tier(cost_control.risk_class)?;
	let changed_surface_summary = normalize_review_cost_control_list(
		cost_control.changed_surface_summary,
		"review_cost_control.changed_surface_summary",
		true,
	)?;
	let high_risk_surfaces = normalize_review_cost_control_list(
		cost_control.high_risk_surfaces,
		"review_cost_control.high_risk_surfaces",
		false,
	)?;
	let reviewer_judgment = normalize_public_review_cost_control_text(
		cost_control.reviewer_judgment,
		"review_cost_control.reviewer_judgment",
	)?;
	let fallback_reason = normalize_optional_public_review_cost_control_reason(
		cost_control.fallback_reason,
		"review_cost_control.fallback_reason",
	)?;
	let compact_eligible = review_class == REVIEW_CLASS_COMPACT_CURRENT_HEAD;

	if !compact_eligible && fallback_reason.is_none() {
		return Err(String::from(
			"`issue_review_checkpoint` requires `review_cost_control.fallback_reason` when `review_class` is `full_current_head_review`.",
		));
	}

	Ok(NormalizedReviewCostControl {
		review_class,
		risk_class,
		compact_eligible,
		changed_surface_count: cost_control.changed_surface_count,
		changed_surface_summary,
		high_risk_surfaces,
		current_head_evidence: cost_control.current_head_evidence,
		validation_backed: cost_control.validation_backed,
		validation_current: cost_control.validation_current,
		evidence_sufficient: cost_control.evidence_sufficient,
		reviewer_judgment,
		fallback_reason,
	})
}

fn normalize_review_class(review_class: String) -> Result<String, String> {
	let review_class = review_class.trim().to_ascii_lowercase().replace([' ', '-'], "_");
	let review_class = match review_class.as_str() {
		"compact" => REVIEW_CLASS_COMPACT_CURRENT_HEAD,
		"full" | "standard" => REVIEW_CLASS_FULL_CURRENT_HEAD,
		other => other,
	};

	match review_class {
		REVIEW_CLASS_COMPACT_CURRENT_HEAD | REVIEW_CLASS_FULL_CURRENT_HEAD =>
			Ok(review_class.to_owned()),
		other => Err(format!(
			"`{ISSUE_REVIEW_CHECKPOINT_TOOL_NAME}` requires `review_cost_control.review_class` to be `{REVIEW_CLASS_COMPACT_CURRENT_HEAD}` or `{REVIEW_CLASS_FULL_CURRENT_HEAD}`, not `{other}`."
		)),
	}
}

fn normalize_review_cost_control_list(
	values: Vec<String>,
	field_name: &str,
	required: bool,
) -> Result<Vec<String>, String> {
	let values = tracker_tool_bridge::normalize_progress_list(values)
		.into_iter()
		.map(|value| normalize_public_review_cost_control_text(value, field_name))
		.collect::<Result<Vec<_>, _>>()?;

	if required && values.is_empty() {
		return Err(format!("`{ISSUE_REVIEW_CHECKPOINT_TOOL_NAME}` requires `{field_name}`."));
	}

	Ok(values)
}

fn normalize_public_review_cost_control_text(
	value: String,
	field_name: &str,
) -> Result<String, String> {
	let value = normalize_required_review_text(value, field_name)?;

	public_text::validate_public_text_field(field_name, &value)
		.map_err(|error| error.to_string())?;

	Ok(value)
}

fn normalize_optional_public_review_cost_control_reason(
	value: Option<String>,
	field_name: &str,
) -> Result<Option<String>, String> {
	let Some(value) = tracker_tool_bridge::normalize_optional_progress_field(value) else {
		return Ok(None);
	};
	let value = normalize_public_review_cost_control_text(value, field_name)?;

	Ok(Some(value))
}

fn validate_review_cost_control_for_checkpoint(
	cost_control: &NormalizedReviewCostControl,
	review_policy_phase: ReviewPolicyPhase,
	status: ReviewPolicyStatus,
	review_contract: &NormalizedReviewCheckpointContract,
	accepted_findings: &[NormalizedReviewCheckpointFinding],
	finding_routes: &[NormalizedReviewCheckpointFindingRoute],
) -> Result<(), String> {
	if cost_control.review_class == REVIEW_CLASS_FULL_CURRENT_HEAD {
		return Ok(());
	}

	let mut forced_full_reasons = compact_review_forced_full_reasons(
		cost_control,
		review_policy_phase,
		status,
		review_contract,
		accepted_findings,
		finding_routes,
	);

	if forced_full_reasons.is_empty() {
		return Ok(());
	}

	forced_full_reasons.sort();
	forced_full_reasons.dedup();

	Err(format!(
		"`issue_review_checkpoint` cannot record `review_cost_control.review_class = {REVIEW_CLASS_COMPACT_CURRENT_HEAD}` because full review is required: {}.",
		forced_full_reasons.join(", ")
	))
}

fn compact_review_forced_full_reasons(
	cost_control: &NormalizedReviewCostControl,
	review_policy_phase: ReviewPolicyPhase,
	status: ReviewPolicyStatus,
	review_contract: &NormalizedReviewCheckpointContract,
	accepted_findings: &[NormalizedReviewCheckpointFinding],
	finding_routes: &[NormalizedReviewCheckpointFindingRoute],
) -> Vec<&'static str> {
	let mut reasons = Vec::new();

	if review_policy_phase != ReviewPolicyPhase::Handoff {
		reasons.push("repair_review_phase");
	}
	if status != ReviewPolicyStatus::Clean {
		reasons.push("nonclean_review_status");
	}
	if review_contract.risk_tier != "low" {
		reasons.push("review_contract_risk_tier_not_low");
	}
	if cost_control.risk_class != "low" {
		reasons.push("review_cost_risk_class_not_low");
	}
	if cost_control.changed_surface_count == 0 {
		reasons.push("missing_changed_surface_count");
	}
	if cost_control.changed_surface_count > MAX_COMPACT_REVIEW_CHANGED_SURFACE_COUNT {
		reasons.push("changed_surface_count_exceeds_compact_limit");
	}
	if !cost_control.high_risk_surfaces.is_empty() {
		reasons.push("high_risk_surfaces_present");
	}
	if !cost_control.current_head_evidence {
		reasons.push("missing_current_head_evidence");
	}
	if !cost_control.validation_backed {
		reasons.push("missing_validation_evidence");
	}
	if !cost_control.validation_current {
		reasons.push("stale_validation_evidence");
	}
	if !cost_control.evidence_sufficient {
		reasons.push("weak_evidence");
	}
	if !accepted_findings.is_empty() {
		reasons.push("accepted_findings_present");
	}
	if finding_routes.iter().any(|route| {
		route.route == REVIEW_ROUTE_CURRENT_BLOCKER || review_route_blocks_landing(route)
	}) {
		reasons.push("blocking_finding_routes_present");
	}

	reasons
}

pub(in crate::agent::tracker_tool_bridge::tools) fn validate_review_cost_control_policy_state(
	cost_control: &NormalizedReviewCostControl,
	policy_update: &ReviewFindingPolicyUpdate,
) -> Result<(), String> {
	if cost_control.review_class != REVIEW_CLASS_COMPACT_CURRENT_HEAD
		|| policy_update.previous_nonclean_rounds == 0
	{
		return Ok(());
	}

	Err(format!(
		"`issue_review_checkpoint` cannot record `review_cost_control.review_class = {REVIEW_CLASS_COMPACT_CURRENT_HEAD}` because full review is required: prior_nonclean_review_rounds_present."
	))
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
