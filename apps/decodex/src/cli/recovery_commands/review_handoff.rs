use std::path::Path;

use clap::{Args, Subcommand};

use crate::{
	prelude::Result,
	recovery::{
		self, ReviewHandoffAdoptRequest, ReviewHandoffDiagnoseRequest, ReviewHandoffRebindRequest,
	},
};

#[derive(Debug, Args)]
pub(in crate::cli) struct ReviewHandoffRecoveryCommand {
	#[command(subcommand)]
	pub(in crate::cli) command: ReviewHandoffRecoverySubcommand,
}
impl ReviewHandoffRecoveryCommand {
	pub(in crate::cli) fn run(&self, config_path: Option<&Path>) -> Result<()> {
		match &self.command {
			ReviewHandoffRecoverySubcommand::Diagnose(args) => {
				recovery::run_review_handoff_diagnose(
					config_path,
					&ReviewHandoffDiagnoseRequest { issue: args.issue.clone(), json: args.json },
				)
			},
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
pub(in crate::cli) struct ReviewHandoffDiagnoseCommand {
	/// Issue identifier to inspect. Omit to inspect all retained review worktrees.
	#[arg(value_name = "ISSUE")]
	pub(in crate::cli) issue: Option<String>,
	/// Emit structured JSON instead of human-readable text.
	#[arg(long)]
	pub(in crate::cli) json: bool,
}

#[derive(Debug, Args)]
pub(in crate::cli) struct ReviewHandoffRebindCommand {
	/// Issue identifier for the retained review lane.
	#[arg(value_name = "ISSUE")]
	pub(in crate::cli) issue: String,
	/// Pull request URL to bind after validation.
	#[arg(long, value_name = "URL")]
	pub(in crate::cli) pr: String,
	/// Validate only; do not write runtime lifecycle state or tracker audit comments.
	#[arg(long)]
	pub(in crate::cli) dry_run: bool,
}

#[derive(Debug, Args)]
pub(in crate::cli) struct ReviewHandoffAdoptCommand {
	/// Issue identifier for the human-owned review lane.
	#[arg(value_name = "ISSUE")]
	pub(in crate::cli) issue: String,
	/// Pull request URL to adopt after validation.
	#[arg(long, value_name = "URL")]
	pub(in crate::cli) pr: String,
	/// Validate only; do not write runtime lifecycle state or tracker audit comments.
	#[arg(long)]
	pub(in crate::cli) dry_run: bool,
}

#[derive(Debug, Subcommand)]
pub(in crate::cli) enum ReviewHandoffRecoverySubcommand {
	/// Read-only diagnosis for orphaned retained review lanes.
	Diagnose(ReviewHandoffDiagnoseCommand),
	/// Explicitly bind a validated PR URL to one retained review lane.
	Rebind(ReviewHandoffRebindCommand),
	/// Adopt a verified human-owned PR into the retained review lifecycle.
	Adopt(ReviewHandoffAdoptCommand),
}
