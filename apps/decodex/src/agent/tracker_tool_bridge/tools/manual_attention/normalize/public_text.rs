use crate::{
	agent::tracker_tool_bridge::{
		ISSUE_COMMENT_TOOL_NAME,
		tools::{
			COMMENT_KIND_MANUAL_ATTENTION,
			manual_attention::{NormalizedAuthorityDecisionOption, normalize::runtime_owned},
		},
	},
	tracker::public_text,
};

pub(in crate::agent::tracker_tool_bridge::tools::manual_attention::normalize) fn validate_public_decision_request_text(
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

pub(in crate::agent::tracker_tool_bridge::tools::manual_attention::normalize) fn validate_public_decision_request_field(
	field_name: &str,
	value: &str,
) -> Result<(), String> {
	public_text::validate_public_text_field(field_name, value).map_err(|error| error.to_string())
}

pub(in crate::agent::tracker_tool_bridge::tools::manual_attention::normalize) fn validate_public_error_class(
	error_class: &str,
) -> Result<(), String> {
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

pub(in crate::agent::tracker_tool_bridge::tools::manual_attention::normalize) fn validate_manual_attention_error_class(
	error_class: &str,
) -> Result<(), String> {
	validate_public_error_class(error_class)?;

	if runtime_owned::is_runtime_owned_manual_attention_error_class(error_class) {
		return Err(format!(
			"`{ISSUE_COMMENT_TOOL_NAME}` kind `{COMMENT_KIND_MANUAL_ATTENTION}` cannot use runtime-owned error class `{error_class}`; keep repairing, retrying, or letting Decodex retain the lane, and use a human-owned blocker class only when automation cannot clear the blocker."
		));
	}

	Ok(())
}
