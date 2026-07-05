use clap::Parser;

use crate::cli::{
	Cli, Command,
	control_commands::{ProjectCommand, project::ProjectSubcommand},
};

#[test]
fn parses_project_subcommands() {
	enum ExpectedProjectSubcommand {
		Add,
		Enable,
		Remove,
	}

	for (case_name, args, expected) in [
		(
			"add",
			&["decodex", "project", "add", "./project.toml"][..],
			ExpectedProjectSubcommand::Add,
		),
		(
			"enable",
			&["decodex", "project", "enable", "pubfi"][..],
			ExpectedProjectSubcommand::Enable,
		),
		(
			"remove",
			&["decodex", "project", "remove", "vibe-mono"][..],
			ExpectedProjectSubcommand::Remove,
		),
	] {
		let cli = Cli::parse_from(args.iter().copied());

		match expected {
			ExpectedProjectSubcommand::Add => assert!(
				matches!(
					cli.command,
					Command::Project(ProjectCommand { command: ProjectSubcommand::Add(_) })
				),
				"unexpected parsed project subcommand for `{case_name}`"
			),
			ExpectedProjectSubcommand::Enable => assert!(
				matches!(
					cli.command,
					Command::Project(ProjectCommand { command: ProjectSubcommand::Enable(_) })
				),
				"unexpected parsed project subcommand for `{case_name}`"
			),
			ExpectedProjectSubcommand::Remove => assert!(
				matches!(
					cli.command,
					Command::Project(ProjectCommand { command: ProjectSubcommand::Remove(_) })
				),
				"unexpected parsed project subcommand for `{case_name}`"
			),
		}
	}
}
