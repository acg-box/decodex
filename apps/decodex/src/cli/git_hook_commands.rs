use std::{
	fs,
	io::{self, BufRead},
	path::PathBuf,
	process::Command,
};

use clap::{Args, Subcommand};

use crate::{
	commit_message,
	prelude::{Result, eyre},
};

const ZERO_OID: &str = "0000000000000000000000000000000000000000";

#[derive(Debug, Args)]
pub(super) struct GitHookCommand {
	#[command(subcommand)]
	pub(super) command: GitHookSubcommand,
}
impl GitHookCommand {
	pub(super) fn run(&self) -> Result<()> {
		match &self.command {
			GitHookSubcommand::CommitMsg(args) => args.run(),
			GitHookSubcommand::PrePush(args) => args.run(),
		}
	}
}

#[derive(Debug, Subcommand)]
pub(super) enum GitHookSubcommand {
	/// Validate the commit message file passed by Git's commit-msg hook.
	CommitMsg(CommitMsgHookCommand),
	/// Validate commits passed by Git's pre-push hook on stdin.
	PrePush(PrePushHookCommand),
}

#[derive(Debug, Args)]
pub(super) struct CommitMsgHookCommand {
	/// Commit message file path supplied by Git.
	#[arg(value_name = "MESSAGE_FILE")]
	message_file: PathBuf,
}
impl CommitMsgHookCommand {
	fn run(&self) -> Result<()> {
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

#[derive(Debug, Args)]
pub(super) struct PrePushHookCommand {
	/// Remote name supplied by Git.
	#[arg(value_name = "REMOTE_NAME")]
	remote_name: String,
	/// Remote URL supplied by Git.
	#[arg(value_name = "REMOTE_URL")]
	#[allow(dead_code)]
	remote_url: String,
}
impl PrePushHookCommand {
	fn run(&self) -> Result<()> {
		let updates = read_pre_push_updates(io::stdin().lock())?;

		validate_pre_push_updates(&self.remote_name, &updates)
	}
}

fn validate_subject(subject: &str) -> Result<()> {
	commit_message::validate_commit_message_subject(subject).map_err(|error| {
		eyre::eyre!(
			"Invalid Decodex commit message subject: {error}\nexpected a single-line JSON subject such as `{}`.",
			commit_message::build_commit_message("describe the change", "manual", &[], false)
				.expect("static example should build")
		)
	})
}

fn extract_commit_subject(raw: &str) -> Result<&str> {
	let mut content_lines =
		raw.lines().filter(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#'));
	let subject =
		content_lines.next().ok_or_else(|| eyre::eyre!("Commit message must not be empty."))?;

	if content_lines.next().is_some() {
		eyre::bail!("Decodex commit messages must be a single non-comment line.");
	}

	Ok(subject)
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PrePushUpdate {
	local_ref: String,
	local_oid: String,
	remote_ref: String,
	remote_oid: String,
}

fn read_pre_push_updates(reader: impl BufRead) -> Result<Vec<PrePushUpdate>> {
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

		updates.push(PrePushUpdate {
			local_ref: fields[0].to_owned(),
			local_oid: fields[1].to_owned(),
			remote_ref: fields[2].to_owned(),
			remote_oid: fields[3].to_owned(),
		});
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
			let subject = extract_commit_subject(&message).map_err(|error| {
				eyre::eyre!(
					"Commit `{oid}` on `{}` has an invalid Decodex commit message body: {error}",
					update.local_ref
				)
			})?;

			commit_message::validate_commit_message_subject(&subject).map_err(|error| {
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

	run_git_lines(&args).map_err(|error| {
		eyre::eyre!(
			"Failed to list commits for push update `{} -> {}`: {error}",
			update.local_ref,
			update.remote_ref
		)
	})
}

fn commit_message_text(oid: &str) -> Result<String> {
	let lines = run_git_lines(&[
		String::from("show"),
		String::from("-s"),
		String::from("--format=%B"),
		oid.to_owned(),
	])?;

	Ok(lines.join("\n"))
}

fn run_git_lines(args: &[String]) -> Result<Vec<String>> {
	let output = Command::new("git").args(args).output()?;

	if !output.status.success() {
		eyre::bail!(
			"`git {}` failed: {}",
			args.join(" "),
			String::from_utf8_lossy(&output.stderr).trim()
		);
	}

	let stdout = String::from_utf8(output.stdout)?;

	Ok(stdout.lines().map(ToOwned::to_owned).collect())
}

fn is_zero_oid(value: &str) -> bool {
	value == ZERO_OID
}

#[cfg(test)]
mod tests {
	use std::io::Cursor;

	use super::{ZERO_OID, extract_commit_subject, read_pre_push_updates, validate_subject};

	#[test]
	fn extract_commit_subject_ignores_blank_and_comment_lines() {
		let subject = extract_commit_subject(
			"\n# comment\n{\"schema\":\"decodex/commit/1\",\"summary\":\"ship fix\",\"authority\":\"manual\"}\n# trailing\n",
		)
		.expect("subject should be extracted");

		assert_eq!(
			subject,
			r#"{"schema":"decodex/commit/1","summary":"ship fix","authority":"manual"}"#
		);
	}

	#[test]
	fn extract_commit_subject_rejects_bodies() {
		let error = extract_commit_subject(
			"{\"schema\":\"decodex/commit/1\",\"summary\":\"ship fix\",\"authority\":\"manual\"}\nbody\n",
		)
		.expect_err("message body should be rejected");

		assert!(error.to_string().contains("single non-comment line"));
	}

	#[test]
	fn validate_subject_rejects_plain_text() {
		let error = validate_subject("Improve account removal interaction")
			.expect_err("plain text commit subject should fail");

		assert!(error.to_string().contains("Invalid Decodex commit message subject"));
	}

	#[test]
	fn read_pre_push_updates_parses_git_input_and_deletions() {
		let updates = read_pre_push_updates(Cursor::new(format!(
			"refs/heads/main abc refs/heads/main def\nrefs/heads/topic {ZERO_OID} refs/heads/topic def\n"
		)))
		.expect("pre-push input should parse");

		assert_eq!(updates.len(), 2);
		assert_eq!(updates[0].local_ref, "refs/heads/main");
		assert_eq!(updates[1].local_oid, ZERO_OID);
	}
}
