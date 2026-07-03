use crate::social_validation::{self, Map, SOCIAL_POST_LIFECYCLE_STATES, Value};

pub(super) fn validate_social_post_lifecycle(entry: &Map<String, Value>, errors: &mut Vec<String>) {
	let Some(lifecycle) = entry.get("post_lifecycle") else {
		return;
	};
	let Some(lifecycle) = lifecycle.as_object() else {
		errors.push("post_lifecycle must be an object when present".into());

		return;
	};

	if !social_validation::matches_one_of(
		lifecycle.get("current_state"),
		SOCIAL_POST_LIFECYCLE_STATES,
	) {
		errors.push(format!(
			"post_lifecycle.current_state must be one of {}",
			social_validation::choices(SOCIAL_POST_LIFECYCLE_STATES)
		));
	}
	if !lifecycle.get("quote_eligible").is_some_and(Value::is_boolean) {
		errors.push("post_lifecycle.quote_eligible must be a boolean".into());
	}
	if lifecycle
		.get("reason")
		.is_some_and(|value| !social_validation::is_non_empty_string(Some(value)))
	{
		errors.push("post_lifecycle.reason must be non-empty when present".into());
	}
	if lifecycle
		.get("superseded_by_candidate")
		.is_some_and(|value| !social_validation::is_non_empty_string(Some(value)))
	{
		errors.push("post_lifecycle.superseded_by_candidate must be non-empty when present".into());
	}
	if lifecycle.get("current_state").and_then(Value::as_str) != Some("live")
		&& lifecycle.get("quote_eligible").and_then(Value::as_bool) == Some(true)
	{
		errors
			.push("post_lifecycle.quote_eligible can be true only for live published posts".into());
	}
}
