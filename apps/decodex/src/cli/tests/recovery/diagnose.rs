use std::path::Path;

use clap::Parser;

use crate::cli::{
	Cli, Command, ProjectConfigArgs,
	control_commands::DiagnoseCommand,
	recovery_commands::{
		RecoverCommand, RecoverSubcommand,
		ghost_lane::{
			GhostLaneDiagnoseCommand, GhostLaneRecoveryCommand, GhostLaneRecoverySubcommand,
		},
		review_handoff::{
			ReviewHandoffDiagnoseCommand, ReviewHandoffRecoveryCommand,
			ReviewHandoffRecoverySubcommand,
		},
		stale_active::{
			StaleActiveDiagnoseCommand, StaleActiveRecoveryCommand, StaleActiveRecoverySubcommand,
		},
	},
};

#[test]
fn parses_diagnose_with_json_limit_and_project_config() {
	let cli = Cli::parse_from([
		"decodex",
		"diagnose",
		"--config",
		"./project.toml",
		"--json",
		"--limit",
		"5",
	]);

	assert!(matches!(
		cli.command,
		Command::Diagnose(DiagnoseCommand {
			project_config: ProjectConfigArgs { config: Some(config) },
			json: true,
			limit: 5,
		}) if config == Path::new("./project.toml")
	));
}

#[test]
fn parses_review_handoff_diagnose_with_issue_and_json() {
	let cli = Cli::parse_from([
		"decodex",
		"recover",
		"--config",
		"./project.toml",
		"review-handoff",
		"diagnose",
		"PUB-718",
		"--json",
	]);

	assert!(matches!(
		cli.command,
		Command::Recover(RecoverCommand {
			project_config: ProjectConfigArgs { config: Some(config) },
			command: RecoverSubcommand::ReviewHandoff(ReviewHandoffRecoveryCommand {
				command: ReviewHandoffRecoverySubcommand::Diagnose(
					ReviewHandoffDiagnoseCommand { issue: Some(_), json: true }
				)
			})
		}) if config == Path::new("./project.toml")
	));
}

#[test]
fn parses_ghost_lane_diagnose_with_issue_and_json() {
	let cli = Cli::parse_from([
		"decodex",
		"recover",
		"--config",
		"./project.toml",
		"ghost-lane",
		"diagnose",
		"PUBFI-012",
		"--json",
	]);

	assert!(matches!(
		cli.command,
		Command::Recover(RecoverCommand {
			project_config: ProjectConfigArgs { config: Some(config) },
			command: RecoverSubcommand::GhostLane(GhostLaneRecoveryCommand {
				command: GhostLaneRecoverySubcommand::Diagnose(
					GhostLaneDiagnoseCommand { issue: Some(_), json: true }
				)
			})
		}) if config == Path::new("./project.toml")
	));
}

#[test]
fn parses_stale_active_diagnose_with_issue_and_json() {
	let cli = Cli::parse_from([
		"decodex",
		"recover",
		"--config",
		"./project.toml",
		"stale-active",
		"diagnose",
		"PUB-1626",
		"--json",
	]);

	assert!(matches!(
		cli.command,
		Command::Recover(RecoverCommand {
			project_config: ProjectConfigArgs { config: Some(config) },
			command: RecoverSubcommand::StaleActive(StaleActiveRecoveryCommand {
				command: StaleActiveRecoverySubcommand::Diagnose(
					StaleActiveDiagnoseCommand { issue: Some(_), json: true }
				)
			})
		}) if config == Path::new("./project.toml")
	));
}
