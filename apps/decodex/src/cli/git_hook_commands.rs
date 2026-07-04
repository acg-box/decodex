mod commit_msg;
mod git;
mod model;
mod pre_push;

use clap::{Args, Subcommand};

use crate::{
	cli::git_hook_commands::{commit_msg::CommitMsgHookCommand, pre_push::PrePushHookCommand},
	prelude::Result,
};

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

#[cfg(test)]
mod tests {
	use std::io::Cursor;

	use crate::cli::git_hook_commands::{
		commit_msg::{self},
		model::ZERO_OID,
		pre_push,
	};

	#[test]
	fn extract_commit_subject_ignores_blank_and_comment_lines() {
		let subject = commit_msg::extract_commit_subject(
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
		let error = commit_msg::extract_commit_subject(
			"{\"schema\":\"decodex/commit/1\",\"summary\":\"ship fix\",\"authority\":\"manual\"}\nbody\n",
		)
		.expect_err("message body should be rejected");

		assert!(error.to_string().contains("single non-comment line"));
	}

	#[test]
	fn validate_subject_rejects_plain_text() {
		let error = commit_msg::validate_subject("Improve account removal interaction")
			.expect_err("plain text commit subject should fail");

		assert!(error.to_string().contains("Invalid Decodex commit message subject"));
	}

	#[test]
	fn read_pre_push_updates_parses_git_input_and_deletions() {
		let updates = pre_push::read_pre_push_updates(Cursor::new(format!(
			"refs/heads/main abc refs/heads/main def\nrefs/heads/topic {ZERO_OID} refs/heads/topic def\n"
		)))
		.expect("pre-push input should parse");

		assert_eq!(updates.len(), 2);
		assert_eq!(updates[0].local_ref, "refs/heads/main");
		assert_eq!(updates[1].local_oid, ZERO_OID);
	}
}
