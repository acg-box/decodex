use std::process::Command;

use crate::github;

#[test]
fn admin_merge_command_includes_subject_when_provided() {
	let mut command = Command::new("gh");

	github::configure_admin_merge_command(
		&mut command,
		"https://github.com/hack-ink/decodex/pull/50",
		"deadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
		Some(r#"{"schema":"decodex/commit/1","summary":"ship fix","authority":"manual"}"#),
	);

	let args = command.get_args().map(|arg| arg.to_string_lossy().into_owned()).collect::<Vec<_>>();

	assert_eq!(
		args,
		vec![
			String::from("pr"),
			String::from("merge"),
			String::from("--admin"),
			String::from("--merge"),
			String::from("--match-head-commit"),
			String::from("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef"),
			String::from("--subject"),
			String::from(
				r#"{"schema":"decodex/commit/1","summary":"ship fix","authority":"manual"}"#
			),
			String::from("--body"),
			String::from(""),
			String::from("https://github.com/hack-ink/decodex/pull/50"),
		]
	);
}
