use std::path::Path;

use clap::Parser;

use crate::cli::{
	Cli, Command, ProjectConfigArgs,
	control_commands::{
		LaneCommand,
		lane::{LaneInspectCommand, LaneInterruptCommand, LaneSteerCommand, LaneSubcommand},
	},
};

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
