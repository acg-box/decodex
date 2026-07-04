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
	_remote_url: String,
}
impl PrePushHookCommand {
	pub(super) fn run(&self) -> Result<()> {
		let updates = read_pre_push_updates(io::stdin().lock())?;

		validate_pre_push_updates(&self.remote_name, &updates)
	}
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

fn validate_pre_push_updates(remote_name: &str, updates: &[PrePushUpdate]) -> Result<()> {
	for update in updates {
		if is_zero_oid(&update.local_oid) {
			continue;
		}

		for oid in rev_list_args(remote_name, update)? {
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

fn rev_list_args(remote_name: &str, update: &PrePushUpdate) -> Result<Vec<String>> {
	let args = if is_zero_oid(&update.remote_oid) {
		let remotes_arg = if remote_name.is_empty() {
			String::from("--remotes")
		} else {
			format!("--remotes={remote_name}")
		};

		vec![String::from("rev-list"), update.local_oid.clone(), String::from("--not"), remotes_arg]
	} else {
		vec![String::from("rev-list"), format!("{}..{}", update.remote_oid, update.local_oid)]
	};

	git::run_git_lines(&args).map_err(|error| {
		eyre::eyre!(
			"Failed to list commits for push update `{} -> {}`: {error}",
			update.local_ref,
			update.remote_ref
		)
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
