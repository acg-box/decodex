use clap::Parser;

use crate::cli::{
	Cli, Command,
	account_commands::{AccountCommand, AccountSubcommand, AccountUseCommand},
};

#[test]
fn parses_account_use_with_auth_json_override() {
	let cli = Cli::parse_from([
		"decodex",
		"account",
		"use",
		"copy@example.com",
		"--auth-json",
		"./auth.json",
		"--json",
	]);

	assert!(matches!(
		cli.command,
		Command::Account(AccountCommand {
			command: AccountSubcommand::Use(AccountUseCommand {
				selector,
				auth_json: Some(_),
				json: true,
			})
		}) if selector == "copy@example.com"
	));
}

#[test]
fn account_commands_reject_project_config() {
	let error = Cli::try_parse_from(["decodex", "account", "list", "--config", "./project.toml"])
		.expect_err("global account commands should not accept project config");

	assert!(error.to_string().contains("--config"));
}

#[test]
fn project_config_must_belong_to_project_scoped_command() {
	let error = Cli::try_parse_from(["decodex", "--config", "./project.toml", "status"])
		.expect_err("project config should not be accepted at root");

	assert!(error.to_string().contains("--config"));
}
