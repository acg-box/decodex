use std::path::Path;

use clap::Parser;

use crate::cli::{
	AttemptCommand, Cli, Command, ProbeCommand, ProjectConfigArgs, control_commands::StatusCommand,
};

#[test]
fn parses_hidden_attempt_with_stdin_request() {
	let cli = Cli::parse_from(["decodex", "_attempt", "--config", "./project.toml", "-"]);

	assert!(matches!(
		cli.command,
		Command::Attempt(AttemptCommand {
			project_config: ProjectConfigArgs { config: Some(config) },
			request,
		}) if request == "-" && config == Path::new("./project.toml")
	));
}

#[test]
fn parses_probe_with_custom_transport() {
	let cli = Cli::parse_from(["decodex", "probe", "ws://127.0.0.1:9000"]);

	assert!(matches!(
		cli.command,
		Command::Probe(ProbeCommand { transport, .. }) if transport == "ws://127.0.0.1:9000"
	));
}

#[test]
fn parses_status_with_json_limit_and_project_config() {
	let cli = Cli::parse_from([
		"decodex",
		"status",
		"--config",
		"./project.toml",
		"--json",
		"--limit",
		"5",
	]);

	assert!(matches!(
		cli.command,
		Command::Status(StatusCommand {
			project_config: ProjectConfigArgs { config: Some(config) },
			json: true,
			limit: 5,
			live: false,
		}) if config == Path::new("./project.toml")
	));
}
