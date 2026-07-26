use crate::social_validation::{self, SIGNAL_CONFIDENCE, Value};

pub(in crate::social_validation) fn validate_social_post_claims(
	claims: Option<&Value>,
	errors: &mut Vec<String>,
) {
	let Some(claims) = social_validation::non_empty_array(claims) else {
		errors.push("claims must be a non-empty list of claim objects".into());

		return;
	};

	for (index, claim) in claims.iter().enumerate() {
		let Some(claim) = claim.as_object() else {
			errors.push(format!("claims[{index}] must be an object"));

			continue;
		};
		social_validation::validate_exact_keys(
			claim,
			&format!("claims[{index}]"),
			&["confidence", "evidence", "text"],
			errors,
		);

		for field in ["text", "evidence"] {
			if !social_validation::is_non_empty_string(claim.get(field)) {
				errors.push(format!("claims[{index}].{field} must be a non-empty string"));
			}
		}

		if !social_validation::matches_one_of(claim.get("confidence"), SIGNAL_CONFIDENCE) {
			errors.push(format!(
				"claims[{index}].confidence must be one of {}",
				social_validation::choices(SIGNAL_CONFIDENCE)
			));
		}
	}
}
