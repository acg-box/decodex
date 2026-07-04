use crate::agent::tracker_tool_bridge::{
	self, AuthorityDecisionOptionArgs, AuthorityDecisionRequestArgs,
	tools::manual_attention::{
		NormalizedAuthorityDecisionOption, NormalizedAuthorityDecisionRequest,
		normalize::{fields, public_text},
	},
};

pub(in crate::agent::tracker_tool_bridge::tools::manual_attention::normalize) fn normalize_authority_decision_request(
	parsed: AuthorityDecisionRequestArgs,
) -> Result<NormalizedAuthorityDecisionRequest, String> {
	let decision_request_id = fields::normalize_required_decision_request_field(
		Some(parsed.decision_request_id),
		"decision_request_id",
	)?;
	let reason_code =
		fields::normalize_required_decision_request_field(Some(parsed.reason_code), "reason_code")?;
	let boundary_type = fields::normalize_required_decision_request_field(
		Some(parsed.boundary_type),
		"boundary_type",
	)?;
	let proposed_change = fields::normalize_required_decision_request_field(
		Some(parsed.proposed_change),
		"proposed_change",
	)?;
	let why_exceeds_authority = fields::normalize_required_decision_request_field(
		Some(parsed.why_exceeds_authority),
		"why_exceeds_authority",
	)?;
	let recommendation = fields::normalize_required_decision_request_field(
		Some(parsed.recommendation),
		"recommendation",
	)?;
	let resume_condition = fields::normalize_required_decision_request_field(
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

	public_text::validate_public_error_class(&reason_code)?;
	public_text::validate_public_error_class(&boundary_type)?;

	if options.is_empty() {
		return Err(String::from(
			"`decision_request.options` must include at least one public option.",
		));
	}

	public_text::validate_public_decision_request_text(
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
	let label =
		fields::normalize_required_decision_request_field(Some(parsed.label), "option.label")?;
	let description = fields::normalize_required_decision_request_field(
		Some(parsed.description),
		"option.description",
	)?;

	public_text::validate_public_decision_request_field("decision_request.option.label", &label)?;
	public_text::validate_public_decision_request_field(
		"decision_request.option.description",
		&description,
	)?;

	Ok(NormalizedAuthorityDecisionOption { label, description })
}
