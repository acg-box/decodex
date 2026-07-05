use clap::Parser;

use crate::cli::{
	Cli, Command,
	manual_commands::{CommitCommand, LandCommand},
};

#[test]
fn parses_manual_authority_commands() {
	enum ExpectedCommand {
		Commit,
		Land,
	}

	for (case_name, args, expected) in [
		(
			"commit manual authority",
			&["decodex", "commit", "ship hotfix", "--manual-authority"][..],
			ExpectedCommand::Commit,
		),
		(
			"land manual authority",
			&[
				"decodex",
				"land",
				"ship hotfix",
				"--manual-authority",
				"--pr",
				"https://github.com/hack-ink/decodex/pull/64",
			][..],
			ExpectedCommand::Land,
		),
	] {
		let cli = Cli::parse_from(args.iter().copied());

		match expected {
			ExpectedCommand::Commit => assert!(
				matches!(
					cli.command,
					Command::Commit(CommitCommand { authority: None, manual_authority: true, .. })
				),
				"unexpected parsed command for `{case_name}`"
			),
			ExpectedCommand::Land => assert!(
				matches!(
					cli.command,
					Command::Land(LandCommand {
						authority: None,
						manual_authority: true,
						pr: Some(_),
						..
					})
				),
				"unexpected parsed command for `{case_name}`"
			),
		}
	}
}
