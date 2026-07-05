use clap::Parser;

use crate::cli::{Cli, Command, manual_commands::CommitCommand};

#[test]
fn parses_commit_with_authority_related_and_breaking() {
	let cli = Cli::parse_from([
		"decodex",
		"commit",
		"redesign decodex cli",
		"--authority",
		"XY-225",
		"--related",
		"XY-201",
		"--related",
		"XY-202",
		"--breaking",
	]);

	assert!(matches!(
		cli.command,
		Command::Commit(CommitCommand {
			authority: Some(_),
			manual_authority: false,
			breaking: true,
			..
		})
	));
}
