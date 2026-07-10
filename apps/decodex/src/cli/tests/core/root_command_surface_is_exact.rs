use clap::CommandFactory;

use crate::cli::Cli;

#[test]
fn root_command_surface_is_exact() {
	let command = Cli::command();
	let commands = command.get_subcommands().map(|command| command.get_name()).collect::<Vec<_>>();

	assert_eq!(
		commands,
		[
			"app",
			"commit",
			"git-hook",
			"land",
			"run",
			"serve",
			"mcp",
			"project",
			"lane",
			"status",
			"diagnose",
			"evidence",
			"intake",
			"recover",
			"archive-linear",
			"maintenance",
			"account",
			"probe",
			"verify",
			"_attempt",
		]
	);
}
