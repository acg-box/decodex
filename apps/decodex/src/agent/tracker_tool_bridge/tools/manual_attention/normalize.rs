use crate::agent::tracker_tool_bridge::tools::{
	COMMENT_KIND_MANUAL_ATTENTION,
	manual_attention::{
		NormalizedAuthorityDecisionOption, NormalizedAuthorityDecisionRequest,
		NormalizedManualAttentionComment,
	},
};
use crate::agent::tracker_tool_bridge::{
	self, AuthorityDecisionOptionArgs, AuthorityDecisionRequestArgs, CommentArgs,
	ISSUE_COMMENT_TOOL_NAME,
};
use crate::tracker::public_text;

pub(super) fn normalize_manual_attention_comment(
	parsed: CommentArgs,
) -> Result<NormalizedManualAttentionComment, String> {
	let error_class = normalize_required_comment_field(parsed.error_class, "error_class")?;
	let next_action = normalize_required_comment_field(parsed.next_action, "next_action")?;
	let blockers = tracker_tool_bridge::normalize_progress_list(parsed.blockers);
	let evidence = tracker_tool_bridge::normalize_progress_list(parsed.evidence);
	let failed_command =
		tracker_tool_bridge::normalize_optional_progress_field(parsed.failed_command);
	let raw_error = tracker_tool_bridge::normalize_optional_progress_field(parsed.raw_error);
	let summary = tracker_tool_bridge::normalize_optional_progress_field(parsed.summary);
	let decision_request =
		parsed.decision_request.map(normalize_authority_decision_request).transpose()?;

	validate_manual_attention_error_class(&error_class)?;

	if blockers.is_empty() {
		return Err(format!(
			"`{ISSUE_COMMENT_TOOL_NAME}` kind `{COMMENT_KIND_MANUAL_ATTENTION}` requires at least one public `blockers` item."
		));
	}
	if evidence.is_empty() {
		return Err(format!(
			"`{ISSUE_COMMENT_TOOL_NAME}` kind `{COMMENT_KIND_MANUAL_ATTENTION}` requires at least one public `evidence` item."
		));
	}

	Ok(NormalizedManualAttentionComment {
		error_class,
		next_action,
		blockers,
		evidence,
		failed_command,
		raw_error,
		summary,
		decision_request,
	})
}

fn normalize_authority_decision_request(
	parsed: AuthorityDecisionRequestArgs,
) -> Result<NormalizedAuthorityDecisionRequest, String> {
	let decision_request_id = normalize_required_decision_request_field(
		Some(parsed.decision_request_id),
		"decision_request_id",
	)?;
	let reason_code =
		normalize_required_decision_request_field(Some(parsed.reason_code), "reason_code")?;
	let boundary_type =
		normalize_required_decision_request_field(Some(parsed.boundary_type), "boundary_type")?;
	let proposed_change =
		normalize_required_decision_request_field(Some(parsed.proposed_change), "proposed_change")?;
	let why_exceeds_authority = normalize_required_decision_request_field(
		Some(parsed.why_exceeds_authority),
		"why_exceeds_authority",
	)?;
	let recommendation =
		normalize_required_decision_request_field(Some(parsed.recommendation), "recommendation")?;
	let resume_condition = normalize_required_decision_request_field(
		Some(parsed.resume_condition),
		"resume_condition",
	)?;
	let options = parsed
		.options
		.into_iter()
		.map(normalize_authority_decision_option)
		.collect::<Result<Vec<_>, _>>()?;
	let retained_worktree_evidence =
		tracker_tool_bridge::normalize_progress_list(parsed.retained_worktree_evidence);
	let retained_diff_evidence =
		tracker_tool_bridge::normalize_progress_list(parsed.retained_diff_evidence);
	let recovery_attempt_context =
		tracker_tool_bridge::normalize_progress_list(parsed.recovery_attempt_context);

	if parsed.boundary_check_id < 1 {
		return Err(String::from(
			"`decision_request.boundary_check_id` must be a positive private evidence record id.",
		));
	}

	validate_public_error_class(&reason_code)?;
	validate_public_error_class(&boundary_type)?;

	if options.is_empty() {
		return Err(String::from(
			"`decision_request.options` must include at least one public option.",
		));
	}

	validate_public_decision_request_text(
		&decision_request_id,
		&proposed_change,
		&why_exceeds_authority,
		&options,
		&recommendation,
		&resume_condition,
	)?;

	Ok(NormalizedAuthorityDecisionRequest {
		boundary_check_id: parsed.boundary_check_id,
		decision_request_id,
		reason_code,
		boundary_type,
		proposed_change,
		why_exceeds_authority,
		options,
		recommendation,
		resume_condition,
		retained_worktree_evidence,
		retained_diff_evidence,
		recovery_attempt_context,
	})
}

fn normalize_authority_decision_option(
	parsed: AuthorityDecisionOptionArgs,
) -> Result<NormalizedAuthorityDecisionOption, String> {
	let label = normalize_required_decision_request_field(Some(parsed.label), "option.label")?;
	let description =
		normalize_required_decision_request_field(Some(parsed.description), "option.description")?;

	validate_public_decision_request_field("decision_request.option.label", &label)?;
	validate_public_decision_request_field("decision_request.option.description", &description)?;

	Ok(NormalizedAuthorityDecisionOption { label, description })
}

fn normalize_required_comment_field(
	value: Option<String>,
	field_name: &str,
) -> Result<String, String> {
	let value = tracker_tool_bridge::normalize_optional_progress_field(value).ok_or_else(|| {
		format!(
			"`{ISSUE_COMMENT_TOOL_NAME}` kind `{COMMENT_KIND_MANUAL_ATTENTION}` requires `{field_name}`."
		)
	})?;

	Ok(value)
}

fn normalize_required_decision_request_field(
	value: Option<String>,
	field_name: &str,
) -> Result<String, String> {
	tracker_tool_bridge::normalize_optional_progress_field(value)
		.ok_or_else(|| format!("`decision_request.{field_name}` must be present and non-empty."))
}

fn validate_public_decision_request_text(
	decision_request_id: &str,
	proposed_change: &str,
	why_exceeds_authority: &str,
	options: &[NormalizedAuthorityDecisionOption],
	recommendation: &str,
	resume_condition: &str,
) -> Result<(), String> {
	validate_public_decision_request_field(
		"decision_request.decision_request_id",
		decision_request_id,
	)?;
	validate_public_decision_request_field("decision_request.proposed_change", proposed_change)?;
	validate_public_decision_request_field(
		"decision_request.why_exceeds_authority",
		why_exceeds_authority,
	)?;
	validate_public_decision_request_field("decision_request.recommendation", recommendation)?;
	validate_public_decision_request_field("decision_request.resume_condition", resume_condition)?;

	for option in options {
		validate_public_decision_request_field("decision_request.option.label", &option.label)?;
		validate_public_decision_request_field(
			"decision_request.option.description",
			&option.description,
		)?;
	}

	Ok(())
}

fn validate_public_decision_request_field(field_name: &str, value: &str) -> Result<(), String> {
	public_text::validate_public_text_field(field_name, value).map_err(|error| error.to_string())
}

fn validate_public_error_class(error_class: &str) -> Result<(), String> {
	let mut chars = error_class.chars();
	let Some(first) = chars.next() else {
		return Err(String::from("`error_class` must be a public snake_case identifier."));
	};

	if !first.is_ascii_lowercase()
		|| !chars.all(|character| {
			character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_'
		}) {
		return Err(String::from("`error_class` must be a public snake_case identifier."));
	}

	Ok(())
}

fn validate_manual_attention_error_class(error_class: &str) -> Result<(), String> {
	validate_public_error_class(error_class)?;

	if is_runtime_owned_manual_attention_error_class(error_class) {
		return Err(format!(
			"`{ISSUE_COMMENT_TOOL_NAME}` kind `{COMMENT_KIND_MANUAL_ATTENTION}` cannot use runtime-owned error class `{error_class}`; keep repairing, retrying, or letting Decodex retain the lane, and use a human-owned blocker class only when automation cannot clear the blocker."
		));
	}

	Ok(())
}

fn is_runtime_owned_manual_attention_error_class(error_class: &str) -> bool {
	matches!(
		error_class,
		"retryable_execution_failure"
			| "repo_gate_canonicalize_failed"
			| "repo_gate_verify_failed"
			| "repo_gate_baseline_failed"
			| "repo_gate_preexisting_baseline_failed"
			| "repo_gate_global_baseline_failed"
			| "repo_gate_tracked_rewrites_left"
			| "repo_gate_git_lock_contention"
			| "stalled_run_detected"
			| "app_server_zero_evidence_start_failed"
			| "app_server_plugin_list_timeout"
			| "app_server_preflight_timeout"
			| "app_server_transport_disconnected"
			| "phase_goal_terminal_path_missing"
			| "app_server_dynamic_tool_protocol_failure"
			| "app_server_dynamic_tool_failed"
			| "app_server_turn_failed"
			| "app_server_usage_limit_exceeded"
	) || runtime_owned_baseline_error_class(error_class)
}

fn runtime_owned_baseline_error_class(error_class: &str) -> bool {
	[
		"baseline",
		"preexisting",
		"pre_existing",
		"repo_wide",
		"repository_wide",
		"global_baseline",
		"docs_okf",
	]
	.iter()
	.any(|pattern| error_class.contains(pattern))
}
