use std::path::Path;

use serde_json::Value;

pub(in crate::tests) fn assert_errors<const N: usize>(payload: &Value, expected: [&str; N]) {
	let validation = crate::validate_artifact(payload);

	for expected_error in expected {
		assert!(
			validation.errors.iter().any(|error| error.contains(expected_error)),
			"expected error containing {expected_error:?}, got {:?}",
			validation.errors
		);
	}

	if expected.is_empty() {
		assert_eq!(validation.errors, Vec::<String>::new());
	}
}

pub(in crate::tests) fn assert_path_errors<const N: usize>(
	path: &str,
	payload: &Value,
	expected: [&str; N],
) {
	let validation = crate::validate_artifact_for_path(Path::new(path), payload);

	for expected_error in expected {
		assert!(
			validation.errors.iter().any(|error| error.contains(expected_error)),
			"expected error containing {expected_error:?}, got {:?}",
			validation.errors
		);
	}

	if expected.is_empty() {
		assert_eq!(validation.errors, Vec::<String>::new());
	}
}
