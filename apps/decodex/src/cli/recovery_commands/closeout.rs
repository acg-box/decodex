use clap::Args;

#[derive(Debug, Args)]
pub(in crate::cli) struct LegacyCloseoutRecoveryCommand {
	/// Issue identifier for the legacy cleanup-only worktree.
	#[arg(value_name = "ISSUE")]
	pub(in crate::cli) issue: String,
	/// Merged pull request URL that proves the lane's terminal code lineage.
	#[arg(long, value_name = "PR_URL")]
	pub(in crate::cli) pr: String,
	/// Validate without writing a Linear execution audit event.
	#[arg(long)]
	pub(in crate::cli) dry_run: bool,
	/// Required for non-dry-run audited legacy closeout.
	#[arg(long)]
	pub(in crate::cli) manual_authority: bool,
}

#[derive(Debug, Args)]
pub(in crate::cli) struct MergedCloseoutRecoveryCommand {
	/// Issue identifier for the already-merged retained lane.
	#[arg(value_name = "ISSUE")]
	pub(in crate::cli) issue: String,
	/// Merged pull request URL that proves the lane's terminal code lineage.
	#[arg(long, value_name = "PR_URL")]
	pub(in crate::cli) pr: String,
	/// Validate without writing closeout or cleanup ledger events.
	#[arg(long)]
	pub(in crate::cli) dry_run: bool,
	/// Required for non-dry-run merged closeout reconciliation.
	#[arg(long)]
	pub(in crate::cli) manual_authority: bool,
}
