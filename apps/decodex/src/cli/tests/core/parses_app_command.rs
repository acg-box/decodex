use clap::Parser;

use crate::cli::{AppCommand, Cli, Command};

#[test]
fn parses_app_command() {
	let cli = Cli::parse_from(["decodex", "app"]);

	assert!(matches!(cli.command, Command::App(AppCommand { bundle: None, new: false })));
}
