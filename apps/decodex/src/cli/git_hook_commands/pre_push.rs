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
struct RemoteRefAdvertisement {
	oid: String,
	refname: String,
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
	let remote_exclusions = live_remote_commit_exclusion_oids(remote_name, remote_url)?;

	for update in updates {
		if is_zero_oid(&update.local_oid) {
			continue;
		}

		for oid in rev_list_args(update, &remote_exclusions)? {
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

fn rev_list_args(update: &PrePushUpdate, remote_exclusions: &[String]) -> Result<Vec<String>> {
	let args = rev_list_command_args_with_remote_exclusion(update, remote_exclusions);

	git::run_git_lines(&args).map_err(|error| {
		eyre::eyre!(
			"Failed to list commits for push update `{} -> {}`: {error}",
			update.local_ref,
			update.remote_ref
		)
	})
}

fn rev_list_command_args_with_remote_exclusion(
	update: &PrePushUpdate,
	remote_exclusions: &[String],
) -> Vec<String> {
	let mut args = if is_zero_oid(&update.remote_oid) {
		vec![String::from("rev-list"), update.local_oid.clone()]
	} else {
		vec![String::from("rev-list"), format!("{}..{}", update.remote_oid, update.local_oid)]
	};

	if !remote_exclusions.is_empty() {
		args.push(String::from("--not"));
		args.extend(remote_exclusions.iter().cloned());
	}

	args
}

fn live_remote_commit_exclusion_oids(remote_name: &str, remote_url: &str) -> Result<Vec<String>> {
	let remote = if remote_url.is_empty() { remote_name } else { remote_url };

	if remote.is_empty() {
		return Ok(Vec::new());
	}

	let advertisements = live_remote_ref_advertisements(remote)
		.unwrap_or_default()
		.into_iter()
		.filter(|advertisement| {
			!advertisement.refname.ends_with("^{}")
				&& local_commit_object_exists(&advertisement.oid).unwrap_or(false)
		});
	let mut oids = advertisements.map(|advertisement| advertisement.oid).collect::<Vec<_>>();

	oids.sort();
	oids.dedup();

	Ok(oids)
}

fn live_remote_ref_advertisements(remote: &str) -> Result<Vec<RemoteRefAdvertisement>> {
	let lines = git::run_git_lines(&[String::from("ls-remote"), remote.to_owned()])?;

	Ok(lines.into_iter().filter_map(|line| parse_ls_remote_line(&line)).collect())
}

fn parse_ls_remote_line(line: &str) -> Option<RemoteRefAdvertisement> {
	let (oid, refname) = line.split_once('\t')?;

	Some(RemoteRefAdvertisement { oid: oid.to_owned(), refname: refname.to_owned() })
}

fn local_commit_object_exists(oid: &str) -> Result<bool> {
	git::git_command_success(&[
		String::from("cat-file"),
		String::from("-e"),
		format!("{oid}^{{commit}}"),
	])
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
		pre_push::{self, RemoteRefAdvertisement},
	};

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
			&[String::from("remote-main"), String::from("remote-topic")],
		);

		assert_eq!(args, ["rev-list", "remote..local", "--not", "remote-main", "remote-topic"]);
	}

	#[test]
	fn live_remote_ref_advertisements_parse_ls_remote_lines() {
		let lines = [
			String::from("1111111111111111111111111111111111111111\tHEAD"),
			String::from("2222222222222222222222222222222222222222\trefs/heads/main"),
		];
		let advertisements = lines
			.into_iter()
			.filter_map(|line| pre_push::parse_ls_remote_line(&line))
			.collect::<Vec<_>>();

		assert_eq!(
			advertisements,
			[
				RemoteRefAdvertisement {
					oid: String::from("1111111111111111111111111111111111111111"),
					refname: String::from("HEAD"),
				},
				RemoteRefAdvertisement {
					oid: String::from("2222222222222222222222222222222222222222"),
					refname: String::from("refs/heads/main"),
				},
			]
		);
	}
}
