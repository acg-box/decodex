use std::path::Path;

use clap::Parser;

use crate::cli::{
	Cli, Command, ProjectConfigArgs,
	control_commands::{DiagnoseCommand, EvidenceCommand},
	recovery_commands::{
		RecoverCommand, RecoverSubcommand,
		closeout::{LegacyCloseoutRecoveryCommand, MergedCloseoutRecoveryCommand},
		ghost_lane::{
			GhostLaneCleanupCommand, GhostLaneDiagnoseCommand, GhostLaneRecoveryCommand,
			GhostLaneRecoverySubcommand,
		},
		review_handoff::{
			ReviewHandoffAdoptCommand, ReviewHandoffDiagnoseCommand, ReviewHandoffRebindCommand,
			ReviewHandoffRecoveryCommand, ReviewHandoffRecoverySubcommand,
		},
		stale_active::{
			StaleActiveDiagnoseCommand, StaleActiveRecoveryCommand, StaleActiveRecoverySubcommand,
			StaleActiveReleaseCommand,
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
fn parses_ghost_lane_cleanup_dry_run() {
	let cli =
		Cli::parse_from(["decodex", "recover", "ghost-lane", "cleanup", "PUBFI-012", "--dry-run"]);

	assert!(matches!(
		cli.command,
		Command::Recover(RecoverCommand {
			command: RecoverSubcommand::GhostLane(GhostLaneRecoveryCommand {
				command: GhostLaneRecoverySubcommand::Cleanup(
					GhostLaneCleanupCommand { issue, dry_run: true }
				)
			}),
			..
		}) if issue == "PUBFI-012"
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

#[test]
fn parses_stale_active_release_dry_run() {
	let cli =
		Cli::parse_from(["decodex", "recover", "stale-active", "release", "PUB-1626", "--dry-run"]);

	assert!(matches!(
		cli.command,
		Command::Recover(RecoverCommand {
			command: RecoverSubcommand::StaleActive(StaleActiveRecoveryCommand {
				command: StaleActiveRecoverySubcommand::Release(
					StaleActiveReleaseCommand { issue, dry_run: true }
				)
			}),
			..
		}) if issue == "PUB-1626"
	));
}

#[test]
fn parses_review_handoff_rebind_dry_run() {
	let cli = Cli::parse_from([
		"decodex",
		"recover",
		"review-handoff",
		"rebind",
		"PUB-718",
		"--pr",
		"https://github.com/hack-ink/pubfi-mono-v2/pull/14",
		"--dry-run",
	]);

	assert!(matches!(
		cli.command,
		Command::Recover(RecoverCommand {
			command: RecoverSubcommand::ReviewHandoff(ReviewHandoffRecoveryCommand {
				command: ReviewHandoffRecoverySubcommand::Rebind(
					ReviewHandoffRebindCommand { issue, pr, dry_run: true }
				)
			}),
			..
		}) if issue == "PUB-718"
			&& pr == "https://github.com/hack-ink/pubfi-mono-v2/pull/14"
	));
}

#[test]
fn parses_review_handoff_adopt_dry_run() {
	let cli = Cli::parse_from([
		"decodex",
		"recover",
		"review-handoff",
		"adopt",
		"XY-944",
		"--pr",
		"https://github.com/hack-ink/decodex/pull/344",
		"--dry-run",
	]);

	assert!(matches!(
		cli.command,
		Command::Recover(RecoverCommand {
			command: RecoverSubcommand::ReviewHandoff(ReviewHandoffRecoveryCommand {
				command: ReviewHandoffRecoverySubcommand::Adopt(
					ReviewHandoffAdoptCommand { issue, pr, dry_run: true }
				)
			}),
			..
		}) if issue == "XY-944"
			&& pr == "https://github.com/hack-ink/decodex/pull/344"
	));
}

#[test]
fn parses_legacy_closeout_manual_authority() {
	let cli = Cli::parse_from([
		"decodex",
		"recover",
		"legacy-closeout",
		"PUB-718",
		"--pr",
		"https://github.com/hack-ink/pubfi-mono-v2/pull/14",
		"--manual-authority",
	]);

	assert!(matches!(
		cli.command,
		Command::Recover(RecoverCommand {
			command: RecoverSubcommand::LegacyCloseout(LegacyCloseoutRecoveryCommand {
				issue,
				pr,
				dry_run: false,
				manual_authority: true,
			}),
			..
		}) if issue == "PUB-718"
			&& pr == "https://github.com/hack-ink/pubfi-mono-v2/pull/14"
	));
}

#[test]
fn parses_merged_closeout_manual_authority() {
	let cli = Cli::parse_from([
		"decodex",
		"recover",
		"merged-closeout",
		"PUB-1549",
		"--pr",
		"https://github.com/helixbox/pubfi-mono/pull/309",
		"--manual-authority",
	]);

	assert!(matches!(
		cli.command,
		Command::Recover(RecoverCommand {
			command: RecoverSubcommand::MergedCloseout(MergedCloseoutRecoveryCommand {
				issue,
				pr,
				dry_run: false,
				manual_authority: true,
			}),
			..
		}) if issue == "PUB-1549"
			&& pr == "https://github.com/helixbox/pubfi-mono/pull/309"
	));
}
