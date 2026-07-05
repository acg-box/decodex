use std::path::Path;

use clap::Parser;

use crate::cli::{
	Cli, Command, ProjectConfigArgs,
	control_commands::{RunCommand, ServeCommand},
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
fn rejects_serve_interval_argument() {
	let error = Cli::try_parse_from(["decodex", "serve", "--interval", "30s"])
		.expect_err("serve interval override should be removed");
	let message = error.to_string();

	assert!(message.contains("--interval"));
}
