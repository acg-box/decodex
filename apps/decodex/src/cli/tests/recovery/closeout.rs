use clap::Parser;

use crate::cli::{
	Cli, Command,
	recovery_commands::{
		RecoverCommand, RecoverSubcommand,
		closeout::{
			LegacyCloseoutRecoveryCommand, MergedCloseoutRecoveryCommand,
			SupersededCloseoutRecoveryCommand,
		},
	},
};

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

#[test]
fn parses_superseded_closeout_manual_authority() {
	let cli = Cli::parse_from([
		"decodex",
		"recover",
		"superseded-closeout",
		"PUB-1704",
		"--pr",
		"https://github.com/helixbox/pubfi-mono/pull/826",
		"--successor-issue",
		"PUB-1705",
		"--successor-pr",
		"https://github.com/helixbox/pubfi-mono/pull/827",
		"--manual-authority",
	]);

	assert!(matches!(
		cli.command,
		Command::Recover(RecoverCommand {
			command: RecoverSubcommand::SupersededCloseout(SupersededCloseoutRecoveryCommand {
				issue,
				pr,
				successor_issue,
				successor_pr,
				dry_run: false,
				manual_authority: true,
			}),
			..
		}) if issue == "PUB-1704"
			&& pr == "https://github.com/helixbox/pubfi-mono/pull/826"
			&& successor_issue == "PUB-1705"
			&& successor_pr == "https://github.com/helixbox/pubfi-mono/pull/827"
	));
}
