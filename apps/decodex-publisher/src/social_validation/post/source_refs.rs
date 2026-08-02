use crate::social_validation::{self, Value};

pub(super) fn validate_social_post_source_refs(refs: Option<&Value>, errors: &mut Vec<String>) {
	let Some(refs) = refs.and_then(Value::as_object) else {
		errors.push("source_refs must be an object".into());

		return;
	};
	social_validation::validate_exact_keys(
		refs,
		"source_refs",
		&["reservations", "social_candidates", "urls"],
		errors,
	);
	let has_refs = ["reservations", "social_candidates", "urls"].iter().any(|field| {
		refs.get(*field)
			.is_some_and(|value| !social_validation::is_empty_or_missing_array(Some(value)))
	});

	if !has_refs {
		errors.push("source_refs must include reservations, social_candidates, or urls".into());
	}
	if refs.get("urls").is_some_and(|urls| !social_validation::is_https_string_array(urls)) {
		errors.push("source_refs.urls must be a list of https URLs".into());
	}

	for field in ["reservations", "social_candidates"] {
		social_validation::validate_optional_string_list(
			refs.get(field),
			&format!("source_refs.{field}"),
			errors,
		);
	}
}
