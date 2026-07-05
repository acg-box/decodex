use std::{ffi::OsStr, process::Command};

use crate::github;

#[test]
fn configure_gh_command_disables_prompt_for_explicit_token_auth() {
	let mut command = Command::new("gh");

	github::configure_gh_command(&mut command, "ghp_example");

	assert!(
		command
			.get_envs()
			.find_map(|(key, value)| (key == OsStr::new("GH_PROMPT_DISABLED")).then_some(value))
			.flatten()
			.is_some_and(|value| value == OsStr::new("1")),
		"configure_gh_command should disable interactive gh prompts"
	);
	assert!(
		command
			.get_envs()
			.find_map(|(key, value)| (key == OsStr::new("GIT_TERMINAL_PROMPT")).then_some(value))
			.flatten()
			.is_some_and(|value| value == OsStr::new("0")),
		"configure_gh_command should disable interactive git prompts"
	);
	assert!(
		command
			.get_envs()
			.find_map(|(key, value)| (key == OsStr::new("GCM_INTERACTIVE")).then_some(value))
			.flatten()
			.is_some_and(|value| value == OsStr::new("never")),
		"configure_gh_command should disable credential-manager prompts"
	);
}
