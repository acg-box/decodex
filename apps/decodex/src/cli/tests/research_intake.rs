use clap::Parser;

use crate::cli::{
	Cli, Command,
	research_intake_commands::{
		IntakeCommand, IntakeGoalCommand, IntakeIssuesCommand, IntakeSubcommand,
	},
};

#[test]
fn parses_intake_issues_dry_run_with_project() {
	let cli = Cli::parse_from([
		"decodex",
		"intake",
		"issues",
		"--project",
		"decodex",
		"XY-1",
		"XY-2",
		"--dry-run",
		"--json",
	]);

	assert!(matches!(
		cli.command,
		Command::Intake(IntakeCommand {
			command: IntakeSubcommand::Issues(IntakeIssuesCommand {
				project: Some(_),
				dry_run: true,
				apply: false,
				json: true,
				issues,
				..
			})
		}) if issues == vec![String::from("XY-1"), String::from("XY-2")]
	));
}

#[test]
fn parses_intake_issues_apply_with_project() {
	let cli = Cli::parse_from([
		"decodex",
		"intake",
		"issues",
		"--project",
		"decodex",
		"XY-1",
		"XY-2",
		"--apply",
		"--json",
	]);

	assert!(matches!(
		cli.command,
		Command::Intake(IntakeCommand {
			command: IntakeSubcommand::Issues(IntakeIssuesCommand {
				project: Some(_),
				dry_run: false,
				apply: true,
				json: true,
				issues,
				..
			})
		}) if issues == vec![String::from("XY-1"), String::from("XY-2")]
	));
}

#[test]
fn parses_intake_goal_apply_with_project_and_team_anchor() {
	let cli = Cli::parse_from([
		"decodex",
		"intake",
		"goal",
		"--project",
		"decodex",
		"goal-intake-contract",
		"--apply",
		"--team-issue",
		"XY-852",
		"--json",
	]);

	assert!(matches!(
		cli.command,
		Command::Intake(IntakeCommand {
			command: IntakeSubcommand::Goal(IntakeGoalCommand {
				project: Some(_),
				contract_id,
				dry_run: false,
				apply: true,
				team_issue: Some(team_issue),
				json: true,
				..
			})
		}) if contract_id == "goal-intake-contract" && team_issue == "XY-852"
	));
}

#[test]
fn rejects_intake_issues_without_explicit_mode() {
	let error = Cli::try_parse_from(["decodex", "intake", "issues", "XY-1"])
		.expect_err("intake issues requires dry-run or apply");

	assert!(error.to_string().contains("--dry-run") || error.to_string().contains("--apply"));
}
