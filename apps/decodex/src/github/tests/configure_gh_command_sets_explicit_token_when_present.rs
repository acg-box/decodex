use std::{collections::HashMap, ffi::OsStr, process::Command};

use crate::github;

#[test]
fn configure_gh_command_sets_explicit_token_when_present() {
	let mut command = Command::new("gh");

	github::configure_gh_command(&mut command, "ghp_example");

	let envs = command
		.get_envs()
		.filter_map(|(key, value)| Some((key.to_owned(), value?.to_owned())))
		.collect::<HashMap<_, _>>();

	assert_eq!(envs.get(OsStr::new("GH_TOKEN")), Some(&OsStr::new("ghp_example").to_owned()));
	assert_eq!(envs.get(OsStr::new("GITHUB_TOKEN")), Some(&OsStr::new("ghp_example").to_owned()));
}
