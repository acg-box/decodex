use clap::Parser;

use crate::cli::{Cli, Command, manual_commands::LandCommand};

#[test]
fn parses_land_with_pr_override() {
	let cli = Cli::parse_from([
		"decodex",
		"land",
		"redesign decodex cli",
		"--pr",
		"https://github.com/hack-ink/decodex/pull/64",
	]);

	assert!(matches!(cli.command, Command::Land(LandCommand { pr: Some(_), .. })));
}
