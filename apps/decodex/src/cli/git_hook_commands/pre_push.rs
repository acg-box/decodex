use std::io::{self, BufRead};

use clap::Args;

use crate::{
	cli::git_hook_commands::{
		commit_msg, git,
		model::{PrePushUpdate, ZERO_OID},
	},
	commit_message,
	prelude::{Result, eyre},
};

#[derive(Debug, Args)]
pub(in crate::cli) struct PrePushHookCommand {
	/// Remote name supplied by Git.
	#[arg(value_name = "REMOTE_NAME")]
	remote_name: String,
	/// Remote URL supplied by Git.
	#[arg(value_name = "REMOTE_URL")]
	remote_url: String,
}
impl PrePushHookCommand {
	pub(super) fn run(&self) -> Result<()> {
		let updates = read_pre_push_updates(io::stdin().lock())?;

		validate_pre_push_updates(&self.remote_name, &self.remote_url, &updates)
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RemoteConfig {
	name: String,
	urls: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RemoteUrlIdentity {
	host: String,
	path: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RemoteUrlMode {
	Fetch,
	Push,
}

pub(in crate::cli::git_hook_commands) fn read_pre_push_updates(
	reader: impl BufRead,
) -> Result<Vec<PrePushUpdate>> {
	let mut updates = Vec::new();

	for line in reader.lines() {
		let line = line?;

		if line.trim().is_empty() {
			continue;
		}

		let fields = line.split_whitespace().collect::<Vec<_>>();

		if fields.len() != 4 {
			eyre::bail!("Invalid pre-push input line `{line}`.");
		}

		updates.push(PrePushUpdate::new(
			fields[0].to_owned(),
			fields[1].to_owned(),
			fields[2].to_owned(),
			fields[3].to_owned(),
		));
	}

	Ok(updates)
}

fn validate_pre_push_updates(
	remote_name: &str,
	remote_url: &str,
	updates: &[PrePushUpdate],
) -> Result<()> {
	for update in updates {
		if is_zero_oid(&update.local_oid) {
			continue;
		}

		for oid in rev_list_args(remote_name, remote_url, update)? {
			let message = commit_message_text(&oid)?;
			let subject = commit_msg::extract_commit_subject(&message).map_err(|error| {
				eyre::eyre!(
					"Commit `{oid}` on `{}` has an invalid Decodex commit message body: {error}",
					update.local_ref
				)
			})?;

			commit_message::validate_commit_message_subject(subject).map_err(|error| {
				eyre::eyre!(
					"Commit `{oid}` on `{}` has an invalid Decodex commit message subject: {error}\nsubject: {subject}",
					update.local_ref
				)
			})?;
		}
	}

	Ok(())
}

fn rev_list_args(
	remote_name: &str,
	remote_url: &str,
	update: &PrePushUpdate,
) -> Result<Vec<String>> {
	let args = rev_list_command_args(remote_name, remote_url, update)?;

	git::run_git_lines(&args).map_err(|error| {
		eyre::eyre!(
			"Failed to list commits for push update `{} -> {}`: {error}",
			update.local_ref,
			update.remote_ref
		)
	})
}

fn rev_list_command_args(
	remote_name: &str,
	remote_url: &str,
	update: &PrePushUpdate,
) -> Result<Vec<String>> {
	Ok(rev_list_command_args_with_remote_exclusion(
		update,
		remote_exclusion_arg(remote_name, remote_url)?,
	))
}

fn rev_list_command_args_with_remote_exclusion(
	update: &PrePushUpdate,
	remote_exclusion: Option<String>,
) -> Vec<String> {
	let mut args = if is_zero_oid(&update.remote_oid) {
		vec![String::from("rev-list"), update.local_oid.clone()]
	} else {
		vec![String::from("rev-list"), format!("{}..{}", update.remote_oid, update.local_oid)]
	};

	if let Some(remotes_arg) = remote_exclusion {
		args.push(String::from("--not"));
		args.push(remotes_arg);
	}

	args
}

fn remote_exclusion_arg(remote_name: &str, remote_url: &str) -> Result<Option<String>> {
	let remotes = configured_remotes()?;

	Ok(remote_exclusion_arg_from_config(remote_name, remote_url, &remotes))
}

fn remote_exclusion_arg_from_config(
	remote_name: &str,
	remote_url: &str,
	remotes: &[RemoteConfig],
) -> Option<String> {
	if remote_name.is_empty() {
		return Some(String::from("--remotes"));
	}
	if remotes.iter().any(|candidate| candidate.name == remote_name) {
		return Some(format!("--remotes={remote_name}"));
	}
	if !remote_url.is_empty()
		&& let Some(remote) = remotes.iter().find(|candidate| {
			candidate.urls.iter().any(|candidate_url| urls_match(candidate_url, remote_url))
		}) {
		return Some(format!("--remotes={}", remote.name));
	}

	None
}

fn configured_remotes() -> Result<Vec<RemoteConfig>> {
	let remote_names = git::run_git_lines(&[String::from("remote")])?;

	remote_names
		.into_iter()
		.map(|name| Ok(RemoteConfig { urls: remote_urls(&name)?, name }))
		.collect()
}

fn remote_urls(remote: &str) -> Result<Vec<String>> {
	let mut urls = Vec::new();

	for mode in [RemoteUrlMode::Fetch, RemoteUrlMode::Push] {
		let mut args = vec![String::from("remote"), String::from("get-url")];

		if mode == RemoteUrlMode::Push {
			args.push(String::from("--push"));
		}

		args.push(String::from("--all"));
		args.push(remote.to_owned());
		urls.extend(git::run_git_lines(&args)?);
	}

	urls.sort();
	urls.dedup();

	Ok(urls)
}

fn urls_match(candidate: &str, remote_url: &str) -> bool {
	candidate == remote_url
		|| match (remote_url_identity(candidate), remote_url_identity(remote_url)) {
			(Some(candidate), Some(remote)) => candidate == remote,
			_ => false,
		}
}

fn remote_url_identity(value: &str) -> Option<RemoteUrlIdentity> {
	let value = value.trim();

	if value.is_empty() || value.starts_with('/') || value.starts_with('.') {
		return None;
	}

	if let Some(scheme_separator) = value.find("://") {
		return url_identity_from_authority_path(&value[scheme_separator + 3..]);
	}

	let user_host_separator = value.find('@')?;
	let path_separator = value[user_host_separator + 1..].find(':')? + user_host_separator + 1;
	let host = &value[user_host_separator + 1..path_separator];
	let path = &value[path_separator + 1..];

	remote_url_identity_parts(host, path)
}

fn url_identity_from_authority_path(value: &str) -> Option<RemoteUrlIdentity> {
	let path_separator = value.find('/')?;
	let authority = &value[..path_separator];
	let path = &value[path_separator + 1..];
	let host = authority.rsplit_once('@').map_or(authority, |(_, host)| host);

	remote_url_identity_parts(host, path)
}

fn remote_url_identity_parts(host: &str, path: &str) -> Option<RemoteUrlIdentity> {
	let host = host.trim();
	let path = path.trim().trim_start_matches('/').trim_end_matches('/');

	if host.is_empty() || path.is_empty() {
		return None;
	}

	Some(RemoteUrlIdentity {
		host: host.to_ascii_lowercase(),
		path: path.strip_suffix(".git").unwrap_or(path).to_owned(),
	})
}

fn commit_message_text(oid: &str) -> Result<String> {
	let lines = git::run_git_lines(&[
		String::from("show"),
		String::from("-s"),
		String::from("--format=%B"),
		oid.to_owned(),
	])?;

	Ok(lines.join("\n"))
}

fn is_zero_oid(value: &str) -> bool {
	value == ZERO_OID
}

#[cfg(test)]
mod tests {
	use crate::cli::git_hook_commands::{
		model::PrePushUpdate,
		pre_push::{self, RemoteConfig},
	};

	fn remote(name: &str, urls: &[&str]) -> RemoteConfig {
		RemoteConfig { name: name.to_owned(), urls: urls.iter().map(ToString::to_string).collect() }
	}

	#[test]
	fn remote_exclusion_uses_configured_remote_name() {
		let exclusion = pre_push::remote_exclusion_arg_from_config(
			"origin",
			"https://github.com/hack-ink/decodex.git",
			&[remote("origin", &["git@github.com-y:hack-ink/decodex.git"])],
		);

		assert_eq!(exclusion.as_deref(), Some("--remotes=origin"));
	}

	#[test]
	fn remote_exclusion_maps_exact_url_remote_name_to_configured_remote() {
		let exclusion = pre_push::remote_exclusion_arg_from_config(
			"https://github.com/helixbox/pubfi-insight.git",
			"https://github.com/helixbox/pubfi-insight.git",
			&[remote(
				"origin",
				&[
					"git@github.com:helixbox/pubfi-insight.git",
					"https://github.com/helixbox/pubfi-insight.git",
				],
			)],
		);

		assert_eq!(exclusion.as_deref(), Some("--remotes=origin"));
	}

	#[test]
	fn remote_exclusion_maps_https_url_push_to_ssh_configured_remote() {
		let exclusion = pre_push::remote_exclusion_arg_from_config(
			"https://github.com/helixbox/pubfi-insight.git",
			"https://github.com/helixbox/pubfi-insight.git",
			&[remote("origin", &["git@github.com:helixbox/pubfi-insight.git"])],
		);

		assert_eq!(exclusion.as_deref(), Some("--remotes=origin"));
	}

	#[test]
	fn remote_exclusion_does_not_exclude_unmatched_url_remotes() {
		let exclusion = pre_push::remote_exclusion_arg_from_config(
			"https://github.com/example/other.git",
			"https://github.com/example/other.git",
			&[
				remote("origin", &["https://github.com/hack-ink/decodex.git"]),
				remote("backup", &["https://github.com/example/backup.git"]),
			],
		);

		assert_eq!(exclusion, None);
	}

	#[test]
	fn remote_url_matching_accepts_common_git_url_forms() {
		assert!(pre_push::urls_match(
			"git@github.com:helixbox/pubfi-insight.git",
			"https://github.com/helixbox/pubfi-insight.git",
		));
		assert!(pre_push::urls_match(
			"ssh://git@github.com/helixbox/pubfi-insight.git",
			"https://github.com/helixbox/pubfi-insight",
		));
		assert!(!pre_push::urls_match(
			"git@github.com:helixbox/pubfi-insight.git",
			"https://github.com/hack-ink/decodex.git",
		));
	}

	#[test]
	fn pre_push_update_excludes_remote_reachable_commits_for_existing_branch() {
		let update = PrePushUpdate::new(
			String::from("refs/heads/topic"),
			String::from("local"),
			String::from("refs/heads/topic"),
			String::from("remote"),
		);
		let args = pre_push::rev_list_command_args_with_remote_exclusion(
			&update,
			Some(String::from("--remotes=origin")),
		);

		assert_eq!(args, ["rev-list", "remote..local", "--not", "--remotes=origin"]);
	}
}
