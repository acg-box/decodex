use std::path::Path;

use clap::Parser;

use crate::cli::{Cli, Command, ProjectConfigArgs, control_commands::EvidenceCommand};

#[test]
fn parses_evidence_with_issue_run_attempt_json_payload_and_project_config() {
	let cli = Cli::parse_from([
		"decodex",
		"evidence",
		"--config",
		"./project.toml",
		"PUB-101",
		"--run-id",
		"run-1",
		"--attempt",
		"2",
		"--json",
		"--include-payload",
	]);

	assert!(matches!(
		cli.command,
		Command::Evidence(EvidenceCommand {
			project_config: ProjectConfigArgs { config: Some(config) },
			project: None,
			issue,
			run_id: Some(_),
			attempt: Some(2),
			json: true,
			include_payload: true,
		}) if config == Path::new("./project.toml") && issue == "PUB-101"
	));
}

#[test]
fn parses_evidence_with_registered_project_id() {
	let cli =
		Cli::parse_from(["decodex", "evidence", "--project", "pubfi-mono", "PUB-101", "--json"]);

	assert!(matches!(
		cli.command,
		Command::Evidence(EvidenceCommand {
			project_config: ProjectConfigArgs { config: None },
			project: Some(project),
			issue,
			run_id: None,
			attempt: None,
			json: true,
			include_payload: false,
		}) if project == "pubfi-mono" && issue == "PUB-101"
	));
}
