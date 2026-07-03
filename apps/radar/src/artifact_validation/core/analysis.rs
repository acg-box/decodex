use std::path::Path;

use serde_json::Value;

use crate::{
	artifact_validation::{
		self, SIGNAL_CONFIDENCE, SIGNAL_SCHEMA,
		constants::{SIGNAL_IMPACT, SIGNAL_KINDS},
		support,
	},
	prelude::{Result, eyre},
};

pub(crate) fn validate_signal_file(path: &Path, payload: &Value) -> Result<()> {
	let validation = artifact_validation::validate_artifact(payload);

	if validation.schema.as_deref() != Some(SIGNAL_SCHEMA) || !validation.errors.is_empty() {
		eyre::bail!(
			"Signal validation failed for {}:\n- {}",
			path.display(),
			validation.errors.join("\n- ")
		);
	}

	Ok(())
}

pub(crate) fn validate_analysis_draft(value: &Value) -> Result<()> {
	let Some(draft) = value.as_object() else {
		return Err(eyre::eyre!("Analysis draft must be an object"));
	};
	let mut errors = Vec::new();

	for field in ["kind", "title", "summary", "why_it_matters", "confidence", "impact"] {
		if !support::is_non_empty_string(draft.get(field)) {
			errors.push(format!("{field} is required in analysis draft"));
		}
	}

	if !support::matches_one_of(draft.get("kind"), SIGNAL_KINDS) {
		errors.push(format!("kind must be one of {}", support::choices(SIGNAL_KINDS)));
	}
	if !support::matches_one_of(draft.get("confidence"), SIGNAL_CONFIDENCE) {
		errors.push(format!("confidence must be one of {}", support::choices(SIGNAL_CONFIDENCE)));
	}
	if !support::matches_one_of(draft.get("impact"), SIGNAL_IMPACT) {
		errors.push(format!("impact must be one of {}", support::choices(SIGNAL_IMPACT)));
	}
	if support::non_empty_array(draft.get("proof_points")).is_none() {
		errors.push("proof_points must be a non-empty list".into());
	}
	if support::string_field(draft, "kind") == Some("try_now")
		&& !support::is_truthy_json_value(draft.get("how_to_try"))
	{
		errors.push("how_to_try is required when kind is try_now".into());
	}
	if support::is_truthy_json_value(draft.get("how_to_try"))
		&& !support::is_truthy_json_value(draft.get("expected_effect"))
	{
		errors.push("expected_effect is required when how_to_try is present".into());
	}
	if errors.is_empty() {
		Ok(())
	} else {
		Err(eyre::eyre!("Analysis draft validation failed:\n- {}", errors.join("\n- ")))
	}
}
