use crate::workflow::tests::shared;

#[test]
fn rejects_incomplete_gate_profiles() {
	for (case_name, paths, commands, expected) in [
		(
			"missing paths",
			"[]",
			r#"verify_commands = ["python3 -c 'print(\"ok\")'"]"#,
			"`execution.gate_profiles.config_subset.paths` must not be empty",
		),
		(
			"missing commands",
			r#"["config/**"]"#,
			"verify_commands = []",
			"`execution.gate_profiles.config_subset` must declare at least one canonicalize or verify command",
		),
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
paths = {paths}
canonicalize_commands = []
{commands}

"#,
				),
			);
		});
		let error = result.expect_err(case_name);

		assert!(
			error.to_string().contains(expected),
			"unexpected error for `{case_name}`: {error:?}"
		);
	}
}
