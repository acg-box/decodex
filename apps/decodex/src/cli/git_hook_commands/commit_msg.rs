use std::{fs, path::PathBuf};

use clap::Args;

use crate::{
	commit_message,
	prelude::{Result, eyre},
};

#[derive(Debug, Args)]
pub(in crate::cli) struct CommitMsgHookCommand {
	/// Commit message file path supplied by Git.
	#[arg(value_name = "MESSAGE_FILE")]
	message_file: PathBuf,
}
impl CommitMsgHookCommand {
	pub(super) fn run(&self) -> Result<()> {
		let raw = fs::read_to_string(&self.message_file).map_err(|error| {
			eyre::eyre!(
				"Failed to read commit message file `{}`: {error}",
				self.message_file.display()
			)
		})?;
		let subject = extract_commit_subject(&raw)?;

		validate_subject(subject)
	}
}

pub(in crate::cli::git_hook_commands) fn validate_subject(subject: &str) -> Result<()> {
	commit_message::validate_commit_message_subject(subject).map_err(|error| {
		eyre::eyre!(
			"Invalid Decodex commit message subject: {error}\nexpected a single-line JSON subject such as `{}`.",
			commit_message::build_commit_message("describe the change", "manual", &[], false)
				.expect("static example should build")
		)
	})
}

pub(in crate::cli::git_hook_commands) fn extract_commit_subject(raw: &str) -> Result<&str> {
	let mut content_lines =
		raw.lines().filter(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#'));
	let subject =
		content_lines.next().ok_or_else(|| eyre::eyre!("Commit message must not be empty."))?;

	if content_lines.next().is_some() {
		eyre::bail!("Decodex commit messages must be a single non-comment line.");
	}

	Ok(subject)
}
