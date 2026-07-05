use std::{collections::HashMap, path::Path};

use crate::{default_branch_sync::commands, git_credentials::GitCredentialEnvironment};

#[test]
fn default_branch_git_commands_use_routed_noninteractive_credentials() {
	let git_env = GitCredentialEnvironment::with_github_credentials(
		String::from("GITHUB_PAT_Y"),
		String::from("ghp_test_token"),
	);
	let command = commands::build_git_command(
		Path::new("/repo"),
		&["fetch", "origin", "refs/heads/main:refs/remotes/origin/main"],
		&git_env,
	);
	let args = command.get_args().map(|arg| arg.to_string_lossy().into_owned()).collect::<Vec<_>>();
	let envs = command
		.get_envs()
		.filter_map(|(key, value)| {
			Some((key.to_string_lossy().into_owned(), value?.to_string_lossy().into_owned()))
		})
		.collect::<HashMap<_, _>>();

	assert_eq!(
		args,
		["-C", "/repo", "fetch", "origin", "refs/heads/main:refs/remotes/origin/main"]
	);
	assert_eq!(envs.get("GH_TOKEN").map(String::as_str), Some("ghp_test_token"));
	assert_eq!(envs.get("GITHUB_TOKEN").map(String::as_str), Some("ghp_test_token"));
	assert_eq!(envs.get("GITHUB_PAT_Y").map(String::as_str), Some("ghp_test_token"));
	assert_eq!(envs.get("GH_PROMPT_DISABLED").map(String::as_str), Some("1"));
	assert_eq!(envs.get("GIT_TERMINAL_PROMPT").map(String::as_str), Some("0"));
	assert_eq!(envs.get("GCM_INTERACTIVE").map(String::as_str), Some("never"));
	assert!(!envs.contains_key("GIT_ASKPASS"));
	assert_eq!(envs.get("GIT_CONFIG_COUNT").map(String::as_str), Some("11"));
	assert_eq!(envs.get("GIT_CONFIG_KEY_0").map(String::as_str), Some("credential.helper"));
	assert_eq!(envs.get("GIT_CONFIG_VALUE_0").map(String::as_str), Some(""));
	assert_eq!(envs.get("GIT_CONFIG_KEY_1").map(String::as_str), Some("credential.helper"));
	assert!(
		envs.get("GIT_CONFIG_VALUE_1")
			.is_some_and(|value| value.contains("github.com") && value.contains("x-access-token"))
	);
	assert_eq!(
		envs.get("GIT_CONFIG_KEY_2").map(String::as_str),
		Some("url.https://github.com/.insteadOf")
	);
	assert_eq!(envs.get("GIT_CONFIG_VALUE_2").map(String::as_str), Some("git@github.com:"));
	assert_eq!(envs.get("GIT_CONFIG_KEY_8").map(String::as_str), Some("commit.gpgsign"));
	assert_eq!(envs.get("GIT_CONFIG_VALUE_8").map(String::as_str), Some("false"));
	assert_eq!(envs.get("GIT_CONFIG_KEY_9").map(String::as_str), Some("tag.gpgsign"));
	assert_eq!(envs.get("GIT_CONFIG_VALUE_9").map(String::as_str), Some("false"));
	assert_eq!(envs.get("GIT_CONFIG_KEY_10").map(String::as_str), Some("user.signingkey"));
	assert_eq!(envs.get("GIT_CONFIG_VALUE_10").map(String::as_str), Some(""));
}
