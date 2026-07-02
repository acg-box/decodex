use serde_json::{Map, Value};

use crate::{
	SIGNAL_CONFIDENCE,
	artifact_validation::{constants::UPSTREAM_IMPACT_KINDS, support},
};

pub(in crate::artifact_validation) fn validate_upstream_impact(
	entry: &Map<String, Value>,
	errors: &mut Vec<String>,
) {
	for field in ["slug", "repo", "observed_change"] {
		if !support::is_non_empty_string(entry.get(field)) {
			errors.push(format!("{field} must be a non-empty string"));
		}
	}

	if support::string_field(entry, "repo").is_some_and(|repo| !repo.contains('/')) {
		errors.push("repo must be owner/name".into());
	}

	validate_upstream_impact_source_refs(entry.get("source_refs"), errors);

	if !support::matches_one_of(entry.get("public_signal_decision"), &["defer", "publish", "skip"])
	{
		errors.push("public_signal_decision must be one of ['defer', 'publish', 'skip']".into());
	}
	if !support::matches_one_of(
		entry.get("control_plane_impact"),
		&["adopt_now", "candidate", "compat_risk", "none", "watch"],
	) {
		errors.push("control_plane_impact must be one of ['adopt_now', 'candidate', 'compat_risk', 'none', 'watch']".into());
	}
	if !support::matches_one_of(
		entry.get("publisher_angle"),
		&["none", "operator_impact", "practical_explainer", "release_pulse", "watch_note"],
	) {
		errors.push("publisher_angle must be one of ['none', 'operator_impact', 'practical_explainer', 'release_pulse', 'watch_note']".into());
	}
	if !support::matches_one_of(entry.get("confidence"), SIGNAL_CONFIDENCE) {
		errors.push(format!("confidence must be one of {}", support::choices(SIGNAL_CONFIDENCE)));
	}

	support::validate_non_empty_string_list(entry.get("evidence"), "evidence", errors);

	for field in ["candidate_followups", "social_notes", "caveats"] {
		support::validate_optional_string_list(entry.get(field), field, errors);
	}
}

fn validate_upstream_impact_source_refs(refs: Option<&Value>, errors: &mut Vec<String>) {
	let Some(refs) = refs.and_then(Value::as_object) else {
		errors.push("source_refs must be an object".into());

		return;
	};
	let valid = support::non_empty_array(refs.get("items")).is_some_and(|items| {
		items.iter().all(|item| {
			item.as_object().is_some_and(|item| {
				support::matches_one_of(item.get("kind"), UPSTREAM_IMPACT_KINDS)
					&& support::is_non_empty_string(item.get("title"))
					&& support::is_https_string(item.get("url"))
					&& item.get("meta").is_none_or(|meta| support::is_non_empty_string(Some(meta)))
			})
		})
	});

	if !valid {
		errors.push(
			"source_refs.items must be a non-empty list of titled https source entries".into(),
		);
	}
}
