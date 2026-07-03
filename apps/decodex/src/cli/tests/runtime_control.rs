use std::path::Path;

use clap::Parser;

use crate::{
	cli::{
		AttemptCommand, Cli, Command, ProbeCommand, ProjectConfigArgs,
		account_commands::{AccountCommand, AccountSubcommand, AccountUseCommand},
		control_commands::{
			LaneCommand, ProjectCommand, RunCommand, ServeCommand, StatusCommand,
			lane::{LaneInspectCommand, LaneInterruptCommand, LaneSteerCommand, LaneSubcommand},
			mcp::McpSubcommand,
			project::ProjectSubcommand,
		},
	},
	mcp::{McpCapabilityProfile, McpTransport},
};

#[test]
fn parses_run_modes() {
	for (case_name, args, expected_issue, expected_dry_run, expected_explain) in [
		(
			"positional issue dry run",
			&["decodex", "run", "issue-1", "--dry-run"][..],
			Some("issue-1"),
			true,
			false,
		),
		("default run", &["decodex", "run"][..], None, false, false),
		("explain dry run", &["decodex", "run", "--dry-run", "--explain"][..], None, true, true),
	] {
		let cli = Cli::parse_from(args.iter().copied());

		assert!(
			matches!(
				cli.command,
				Command::Run(RunCommand { issue, dry_run, explain, .. })
					if issue.as_deref() == expected_issue
						&& dry_run == expected_dry_run
						&& explain == expected_explain
			),
			"unexpected parsed run command for `{case_name}`"
		);
	}

	let error = Cli::try_parse_from(["decodex", "run", "--explain"])
		.expect_err("explain should require dry-run");

	assert!(error.to_string().contains("--dry-run"));

	let error = Cli::try_parse_from(["decodex", "run", "issue-1", "--dry-run", "--explain"])
		.expect_err("explain should reject positional issue");

	assert!(error.to_string().contains("--explain"));
	assert!(error.to_string().contains("[ISSUE]"));
}

#[test]
fn parses_serve_modes() {
	for (case_name, args, expected_listen_address, expected_config, expected_dev) in [
		("default listen address", &["decodex", "serve"][..], "127.0.0.1:8192", None, false),
		(
			"custom listen address and project config",
			&[
				"decodex",
				"serve",
				"--config",
				"./project.toml",
				"--listen-address",
				"127.0.0.1:9000",
			][..],
			"127.0.0.1:9000",
			Some("./project.toml"),
			false,
		),
		("dev mode", &["decodex", "serve", "--dev"][..], "127.0.0.1:8192", None, true),
	] {
		let cli = Cli::parse_from(args.iter().copied());

		assert!(
			matches!(
				cli.command,
				Command::Serve(ServeCommand {
					project_config: ProjectConfigArgs { config },
					listen_address,
					dev,
				}) if listen_address == expected_listen_address
					&& config.as_deref() == expected_config.map(Path::new)
					&& dev == expected_dev
			),
			"unexpected parsed serve command for `{case_name}`"
		);
	}
}

#[test]
fn parses_mcp_stdio_serve() {
	let cli = Cli::parse_from([
		"decodex",
		"mcp",
		"serve",
		"--config",
		"./project.toml",
		"--transport",
		"stdio",
	]);
	let Command::Mcp(command) = cli.command else {
		panic!("expected mcp command");
	};
	let McpSubcommand::Serve(serve) = command.command;

	assert_eq!(serve.project_config.config.as_deref(), Some(Path::new("./project.toml")));
	assert_eq!(serve.transport, McpTransport::Stdio);
	assert_eq!(serve.effective_capability_profile(), McpCapabilityProfile::Admin);
	assert_eq!(serve.listen_address, crate::mcp::DEFAULT_MCP_HTTP_LISTEN_ADDRESS);
	assert!(serve.allowed_origins.is_empty());
	assert_eq!(serve.bearer_token_env, None);
}

#[test]
fn parses_mcp_streamable_http_serve_with_safe_profile_default() {
	let cli = Cli::parse_from([
		"decodex",
		"mcp",
		"serve",
		"--transport",
		"streamable-http",
		"--listen-address",
		"127.0.0.1:8194",
		"--allow-origin",
		"http://127.0.0.1:8194",
		"--bearer-token-env",
		"DECODEX_MCP_TOKEN",
	]);
	let Command::Mcp(command) = cli.command else {
		panic!("expected mcp command");
	};
	let McpSubcommand::Serve(serve) = command.command;

	assert_eq!(serve.transport, McpTransport::StreamableHttp);
	assert_eq!(serve.effective_capability_profile(), McpCapabilityProfile::Observe);
	assert_eq!(serve.listen_address, "127.0.0.1:8194");
	assert_eq!(serve.allowed_origins, vec!["http://127.0.0.1:8194"]);
	assert_eq!(serve.bearer_token_env.as_deref(), Some("DECODEX_MCP_TOKEN"));
}

#[test]
fn rejects_serve_interval_argument() {
	let error = Cli::try_parse_from(["decodex", "serve", "--interval", "30s"])
		.expect_err("serve interval override should be removed");
	let message = error.to_string();

	assert!(message.contains("--interval"));
}

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

#[test]
fn parses_lane_inspect_with_run_id_and_project_config() {
	let cli = Cli::parse_from([
		"decodex",
		"lane",
		"--config",
		"./project.toml",
		"inspect",
		"XY-703",
		"--run-id",
		"xy-703-attempt-1",
		"--json",
	]);

	assert!(matches!(
		cli.command,
		Command::Lane(LaneCommand {
			project_config: ProjectConfigArgs { config: Some(config) },
			command: LaneSubcommand::Inspect(LaneInspectCommand {
				issue,
				run_id: Some(run_id),
				json: true,
			})
		}) if config == Path::new("./project.toml")
			&& issue == "XY-703"
			&& run_id == "xy-703-attempt-1"
	));
}

#[test]
fn parses_lane_interrupt_with_force_reason_and_project_config() {
	let cli = Cli::parse_from([
		"decodex",
		"lane",
		"--config",
		"./project.toml",
		"interrupt",
		"XY-703",
		"--run-id",
		"xy-703-attempt-1",
		"--force",
		"--reason",
		"operator requested",
		"--json",
	]);

	assert!(matches!(
		cli.command,
		Command::Lane(LaneCommand {
			project_config: ProjectConfigArgs { config: Some(config) },
			command: LaneSubcommand::Interrupt(LaneInterruptCommand {
				issue,
				run_id,
				force: true,
				reason: Some(reason),
				json: true,
			})
		}) if config == Path::new("./project.toml")
			&& issue == "XY-703"
			&& run_id == "xy-703-attempt-1"
			&& reason == "operator requested"
	));
}

#[test]
fn parses_lane_steer_with_expected_turn_precondition() {
	let cli = Cli::parse_from([
		"decodex",
		"lane",
		"--config",
		"./project.toml",
		"steer",
		"XY-704",
		"--run-id",
		"run-1",
		"--expected-turn-id",
		"turn-1",
		"--message",
		"adjust the current implementation",
		"--json",
	]);

	assert!(matches!(
		cli.command,
		Command::Lane(LaneCommand {
			project_config: ProjectConfigArgs { config: Some(config) },
			command: LaneSubcommand::Steer(LaneSteerCommand {
				issue,
				run_id,
				expected_turn_id,
				message,
				json: true,
				..
			})
		}) if config == Path::new("./project.toml")
			&& issue == "XY-704"
			&& run_id == "run-1"
			&& expected_turn_id == "turn-1"
			&& message == "adjust the current implementation"
	));
}
