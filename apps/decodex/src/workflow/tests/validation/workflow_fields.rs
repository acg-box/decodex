use crate::workflow::tests::shared::{self, Edit};

#[test]
fn rejects_unknown_workflow_fields() {
	for (case_name, edit, field) in [
		(
			"nested tracker field",
			Edit::Replace(
				"needs_attention_label = \"decodex:needs-attention\"",
				"needs_attention_label = \"decodex:needs-attention\"\nunexpected_tracker_key = \"pubfi\"",
			),
			"unexpected_tracker_key",
		),
		(
			"execution field",
			Edit::Replace(
				"verify_commands = []",
				"verify_commands = []\nunexpected_execution_field = [\"cargo make test\"]",
			),
			"unexpected_execution_field",
		),
		(
			"top-level table",
			Edit::Replace(
				"[context]\nread_first = []",
				"[context]\nread_first = []\n\n[unexpected]\nenabled = true",
			),
			"unexpected",
		),
	] {
		let result = shared::parse_valid_workflow_with(|markdown| edit.apply(markdown));
		let error = result.expect_err(case_name);

		assert!(error.to_string().contains(&format!("unknown field `{field}`")));
	}
}

#[test]
fn rejects_invalid_gate_command_entries() {
	for (case_name, edit, expected) in [
		(
			"blank canonicalize command",
			Edit::Replace("canonicalize_commands = []", "canonicalize_commands = [\"\"]"),
			"`execution.canonicalize_commands` entries",
		),
		(
			"untrimmed verify command",
			Edit::Replace("verify_commands = []", "verify_commands = [\"  cargo make test  \"]"),
			"`execution.verify_commands` entries",
		),
		(
			"blank profile canonicalize command",
			Edit::Replace(
				r#"gate_profiles = {}
canonicalize_commands = []
verify_commands = []
"#,
				r#"
canonicalize_commands = []
verify_commands = []

[execution.gate_profiles.config_subset]
match_mode = "only"
paths = ["config/**"]
canonicalize_commands = [" "]
verify_commands = ["python3 -c 'print(\"ok\")'"]

"#,
			),
			"`execution.gate_profiles.config_subset.canonicalize_commands` entries",
		),
	] {
		let result = shared::parse_valid_workflow_with(|markdown| edit.apply(markdown));
		let error = result.expect_err(case_name);

		assert!(
			error.to_string().contains(expected),
			"unexpected error for `{case_name}`: {error:?}"
		);
	}
}

#[test]
fn rejects_invalid_context_read_first_entries() {
	for (case_name, replacement, expected) in [
		(
			"blank read_first entry",
			"read_first = [\"\"]",
			"`context.read_first` entries must not be empty",
		),
		(
			"parent traversal read_first path",
			"read_first = [\"../secret.md\"]",
			"must not contain `.`, `..`, root, or prefix components",
		),
		(
			"absolute read_first path",
			"read_first = [\"/tmp/secret.md\"]",
			"must be repository-relative paths",
		),
	] {
		let result = shared::parse_valid_workflow_with(|markdown| {
			*markdown = markdown.replace("read_first = []", replacement);
		});
		let error = result.expect_err(case_name);

		assert!(
			error.to_string().contains(expected),
			"unexpected error for `{case_name}`: {error:?}"
		);
	}
}
