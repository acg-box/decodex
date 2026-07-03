//! Recovery CLI command definitions.

use std::path::Path;

use clap::{Args, Subcommand};

use crate::{
	cli::ProjectConfigArgs,
	prelude::Result,
	recovery::{
		self, GhostLaneCleanupRequest, GhostLaneDiagnoseRequest, LegacyCloseoutRecoveryRequest,
		MergedCloseoutRecoveryRequest, ReviewHandoffAdoptRequest, ReviewHandoffDiagnoseRequest,
		ReviewHandoffRebindRequest, StaleActiveDiagnoseRequest, StaleActiveReleaseRequest,
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
		}
	}
}

#[derive(Debug, Args)]
pub(super) struct StaleActiveRecoveryCommand {
	#[command(subcommand)]
	pub(super) command: StaleActiveRecoverySubcommand,
}
impl StaleActiveRecoveryCommand {
	fn run(&self, config_path: Option<&Path>) -> Result<()> {
		match &self.command {
			StaleActiveRecoverySubcommand::Diagnose(args) => recovery::run_stale_active_diagnose(
				config_path,
				&StaleActiveDiagnoseRequest { issue: args.issue.clone(), json: args.json },
			),
			StaleActiveRecoverySubcommand::Release(args) => recovery::run_stale_active_release(
				config_path,
				&StaleActiveReleaseRequest { issue: args.issue.clone(), dry_run: args.dry_run },
			),
		}
	}
}

#[derive(Debug, Args)]
pub(super) struct StaleActiveDiagnoseCommand {
	/// Issue identifier or tracker issue id to inspect. Omit to inspect active-label issues.
	#[arg(value_name = "ISSUE")]
	pub(super) issue: Option<String>,
	/// Emit structured JSON instead of human-readable text.
	#[arg(long)]
	pub(super) json: bool,
}

#[derive(Debug, Args)]
pub(super) struct StaleActiveReleaseCommand {
	/// Issue identifier or tracker issue id for the stale active lane.
	#[arg(value_name = "ISSUE")]
	pub(super) issue: String,
	/// Validate only; do not clear labels, terminalize the run, or write private audit evidence.
	#[arg(long)]
	pub(super) dry_run: bool,
}

#[derive(Debug, Args)]
pub(super) struct GhostLaneRecoveryCommand {
	#[command(subcommand)]
	pub(super) command: GhostLaneRecoverySubcommand,
}
impl GhostLaneRecoveryCommand {
	fn run(&self, config_path: Option<&Path>) -> Result<()> {
		match &self.command {
			GhostLaneRecoverySubcommand::Diagnose(args) => recovery::run_ghost_lane_diagnose(
				config_path,
				&GhostLaneDiagnoseRequest { issue: args.issue.clone(), json: args.json },
			),
			GhostLaneRecoverySubcommand::Cleanup(args) => recovery::run_ghost_lane_cleanup(
				config_path,
				&GhostLaneCleanupRequest { issue: args.issue.clone(), dry_run: args.dry_run },
			),
		}
	}
}

#[derive(Debug, Args)]
pub(super) struct GhostLaneDiagnoseCommand {
	/// Issue identifier or local issue id to inspect. Omit to inspect leased lanes.
	#[arg(value_name = "ISSUE")]
	pub(super) issue: Option<String>,
	/// Emit structured JSON instead of human-readable text.
	#[arg(long)]
	pub(super) json: bool,
}

#[derive(Debug, Args)]
pub(super) struct GhostLaneCleanupCommand {
	/// Issue identifier or local issue id for the ghost lane.
	#[arg(value_name = "ISSUE")]
	pub(super) issue: String,
	/// Validate only; do not terminalize the run lease or write private audit evidence.
	#[arg(long)]
	pub(super) dry_run: bool,
}

#[derive(Debug, Args)]
pub(super) struct ReviewHandoffRecoveryCommand {
	#[command(subcommand)]
	pub(super) command: ReviewHandoffRecoverySubcommand,
}
impl ReviewHandoffRecoveryCommand {
	fn run(&self, config_path: Option<&Path>) -> Result<()> {
		match &self.command {
			ReviewHandoffRecoverySubcommand::Diagnose(args) =>
				recovery::run_review_handoff_diagnose(
					config_path,
					&ReviewHandoffDiagnoseRequest { issue: args.issue.clone(), json: args.json },
				),
			ReviewHandoffRecoverySubcommand::Rebind(args) => recovery::run_review_handoff_rebind(
				config_path,
				&ReviewHandoffRebindRequest {
					issue: args.issue.clone(),
					pr_url: args.pr.clone(),
					dry_run: args.dry_run,
				},
			),
			ReviewHandoffRecoverySubcommand::Adopt(args) => recovery::run_review_handoff_adopt(
				config_path,
				&ReviewHandoffAdoptRequest {
					issue: args.issue.clone(),
					pr_url: args.pr.clone(),
					dry_run: args.dry_run,
				},
			),
		}
	}
}

#[derive(Debug, Args)]
pub(super) struct ReviewHandoffDiagnoseCommand {
	/// Issue identifier to inspect. Omit to inspect all retained review worktrees.
	#[arg(value_name = "ISSUE")]
	pub(super) issue: Option<String>,
	/// Emit structured JSON instead of human-readable text.
	#[arg(long)]
	pub(super) json: bool,
}

#[derive(Debug, Args)]
pub(super) struct ReviewHandoffRebindCommand {
	/// Issue identifier for the retained review lane.
	#[arg(value_name = "ISSUE")]
	pub(super) issue: String,
	/// Pull request URL to bind after validation.
	#[arg(long, value_name = "URL")]
	pub(super) pr: String,
	/// Validate only; do not write runtime lifecycle state or tracker audit comments.
	#[arg(long)]
	pub(super) dry_run: bool,
}

#[derive(Debug, Args)]
pub(super) struct ReviewHandoffAdoptCommand {
	/// Issue identifier for the human-owned review lane.
	#[arg(value_name = "ISSUE")]
	pub(super) issue: String,
	/// Pull request URL to adopt after validation.
	#[arg(long, value_name = "URL")]
	pub(super) pr: String,
	/// Validate only; do not write runtime lifecycle state or tracker audit comments.
	#[arg(long)]
	pub(super) dry_run: bool,
}

#[derive(Debug, Args)]
pub(super) struct LegacyCloseoutRecoveryCommand {
	/// Issue identifier for the legacy cleanup-only worktree.
	#[arg(value_name = "ISSUE")]
	pub(super) issue: String,
	/// Merged pull request URL that proves the lane's terminal code lineage.
	#[arg(long, value_name = "PR_URL")]
	pub(super) pr: String,
	/// Validate without writing a Linear execution audit event.
	#[arg(long)]
	pub(super) dry_run: bool,
	/// Required for non-dry-run audited legacy closeout.
	#[arg(long)]
	pub(super) manual_authority: bool,
}

#[derive(Debug, Args)]
pub(super) struct MergedCloseoutRecoveryCommand {
	/// Issue identifier for the already-merged retained lane.
	#[arg(value_name = "ISSUE")]
	pub(super) issue: String,
	/// Merged pull request URL that proves the lane's terminal code lineage.
	#[arg(long, value_name = "PR_URL")]
	pub(super) pr: String,
	/// Validate without writing closeout or cleanup ledger events.
	#[arg(long)]
	pub(super) dry_run: bool,
	/// Required for non-dry-run merged closeout reconciliation.
	#[arg(long)]
	pub(super) manual_authority: bool,
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
}

#[derive(Debug, Subcommand)]
pub(super) enum GhostLaneRecoverySubcommand {
	/// Read-only diagnosis for local leases whose tracker issue is missing.
	Diagnose(GhostLaneDiagnoseCommand),
	/// Terminalize a proven ghost lane and clear its local run lease.
	Cleanup(GhostLaneCleanupCommand),
}

#[derive(Debug, Subcommand)]
pub(super) enum StaleActiveRecoverySubcommand {
	/// Read-only diagnosis for tracker-present stale active ownership.
	Diagnose(StaleActiveDiagnoseCommand),
	/// Clear a proven stale active label and terminalize stale local ownership.
	Release(StaleActiveReleaseCommand),
}

#[derive(Debug, Subcommand)]
pub(super) enum ReviewHandoffRecoverySubcommand {
	/// Read-only diagnosis for orphaned retained review lanes.
	Diagnose(ReviewHandoffDiagnoseCommand),
	/// Explicitly bind a validated PR URL to one retained review lane.
	Rebind(ReviewHandoffRebindCommand),
	/// Adopt a verified human-owned PR into the retained review lifecycle.
	Adopt(ReviewHandoffAdoptCommand),
}
