use std::path::Path;

use clap::Parser;

use crate::cli::{
	Cli, Command,
	docs_okf_commands::{OkfCommand, OkfInitCommand, OkfInitProfileArg, OkfSubcommand},
};

#[test]
fn parses_okf_init_command() {
	let cli = Cli::parse_from(["decodex", "okf", "init", "knowledge", "--profile", "wiki"]);

	assert!(matches!(
		cli.command,
		Command::Okf(OkfCommand {
			command: OkfSubcommand::Init(OkfInitCommand {
				root,
				profile: OkfInitProfileArg::Wiki,
			}),
		}) if root == Path::new("knowledge")
	));
}
