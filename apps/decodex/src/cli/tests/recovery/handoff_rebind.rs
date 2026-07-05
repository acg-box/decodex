use clap::Parser;

use crate::cli::{
	Cli, Command,
	recovery_commands::{
		RecoverCommand, RecoverSubcommand,
		review_handoff::{
			ReviewHandoffAdoptCommand, ReviewHandoffRebindCommand, ReviewHandoffRecoveryCommand,
			ReviewHandoffRecoverySubcommand,
		},
	},
};

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
