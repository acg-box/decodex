use clap::Parser;

use crate::cli::{
	Cli, Command,
	recovery_commands::{
		RecoverCommand, RecoverSubcommand,
		ghost_lane::{
			GhostLaneCleanupCommand, GhostLaneRecoveryCommand, GhostLaneRecoverySubcommand,
		},
		stale_active::{
			StaleActiveRecoveryCommand, StaleActiveRecoverySubcommand, StaleActiveReleaseCommand,
		},
	},
};

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
