//! Recovery CLI command definitions.

pub(in crate::cli) mod closeout;
pub(in crate::cli) mod ghost_lane;
pub(in crate::cli) mod review_handoff;
pub(in crate::cli) mod stale_active;

use clap::{Args, Subcommand};

use self::{
	closeout::{
		LegacyCloseoutRecoveryCommand, MergedCloseoutRecoveryCommand,
		SupersededCloseoutRecoveryCommand,
	},
	ghost_lane::GhostLaneRecoveryCommand,
	review_handoff::ReviewHandoffRecoveryCommand,
	stale_active::StaleActiveRecoveryCommand,
};
use crate::{
	cli::ProjectConfigArgs,
	prelude::Result,
	recovery::{
		self, LegacyCloseoutRecoveryRequest, MergedCloseoutRecoveryRequest,
		SupersededCloseoutRecoveryRequest,
	},
};

#[derive(Debug, Args)]
pub(super) struct RecoverCommand {
	#[command(flatten)]
	pub(super) project_config: ProjectConfigArgs,
	#[command(subcommand)]
	pub(super) command: RecoverSubcommand,
}
impl RecoverCommand {
	pub(super) fn run(&self) -> Result<()> {
		match &self.command {
			RecoverSubcommand::ReviewHandoff(args) => args.run(self.project_config.as_path()),
			RecoverSubcommand::GhostLane(args) => args.run(self.project_config.as_path()),
			RecoverSubcommand::StaleActive(args) => args.run(self.project_config.as_path()),
			RecoverSubcommand::LegacyCloseout(args) => recovery::run_legacy_closeout(
				self.project_config.as_path(),
				&LegacyCloseoutRecoveryRequest {
					issue: args.issue.clone(),
					pr_url: args.pr.clone(),
					dry_run: args.dry_run,
					manual_authority: args.manual_authority,
				},
			),
			RecoverSubcommand::MergedCloseout(args) => recovery::run_merged_closeout(
				self.project_config.as_path(),
				&MergedCloseoutRecoveryRequest {
					issue: args.issue.clone(),
					pr_url: args.pr.clone(),
					dry_run: args.dry_run,
					manual_authority: args.manual_authority,
				},
			),
			RecoverSubcommand::SupersededCloseout(args) => recovery::run_superseded_closeout(
				self.project_config.as_path(),
				&SupersededCloseoutRecoveryRequest {
					issue: args.issue.clone(),
					pr_url: args.pr.clone(),
					successor_issue: args.successor_issue.clone(),
					successor_pr_url: args.successor_pr.clone(),
					dry_run: args.dry_run,
					manual_authority: args.manual_authority,
				},
			),
		}
	}
}

#[derive(Debug, Subcommand)]
pub(super) enum RecoverSubcommand {
	/// Recover retained review lanes whose lifecycle record is missing.
	ReviewHandoff(ReviewHandoffRecoveryCommand),
	/// Diagnose or clear missing-issue ghost lanes after fail-closed safety checks.
	GhostLane(GhostLaneRecoveryCommand),
	/// Diagnose or release tracker-present stale active ownership after fail-closed checks.
	StaleActive(StaleActiveRecoveryCommand),
	/// Record an audited fallback closeout for a legacy cleanup-only worktree.
	LegacyCloseout(LegacyCloseoutRecoveryCommand),
	/// Reconcile stale retained attention after a PR is already merged and cleaned up.
	MergedCloseout(MergedCloseoutRecoveryCommand),
	/// Close an obsolete retained PR after a successor PR landed the repair lineage.
	SupersededCloseout(SupersededCloseoutRecoveryCommand),
}
