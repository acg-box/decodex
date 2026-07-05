use crate::workflow::tests::shared;

#[test]
fn rejects_gate_profile_paths_that_escape_repo() {
	for (path, expected) in [
		("../config/**", "must not contain `.`, `..`, root, or prefix components"),
		("/tmp/config/**", "must be repository-relative paths"),
	] {
		let result = shared::parse_valid_workflow_with(|markdown| {
			*markdown = markdown.replace(
				r#"gate_profiles = {}
canonicalize_commands = []
verify_commands = []
"#,
				&format!(
					r#"
canonicalize_commands = []
verify_commands = []

[execution.gate_profiles.config_subset]
match_mode = "only"
paths = ["{path}"]
canonicalize_commands = []
verify_commands = ["python3 -c 'print(\"ok\")'"]

"#
				),
			);
		});
		let error = result.expect_err("escaping gate profile path should fail");

		assert!(error.to_string().contains(expected), "unexpected error for `{path}`: {error:?}");
	}
}
