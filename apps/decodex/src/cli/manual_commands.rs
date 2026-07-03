//! Manual Git lifecycle CLI command definitions.

use clap::Args;

use crate::{
	cli::ProjectConfigArgs,
	manual::{self, ManualCommitRequest, ManualLandRequest},
	prelude::Result,
};

#[derive(Debug, Args)]
pub(super) struct CommitCommand {
	#[command(flatten)]
	pub(super) project_config: ProjectConfigArgs,
	/// Tree-change summary for the new commit message.
	#[arg(value_name = "SUMMARY")]
	pub(super) summary: String,
	/// Primary issue that authorizes the change. Defaults to the current issue worktree name.
	#[arg(long, value_name = "ISSUE", conflicts_with = "manual_authority")]
	pub(super) authority: Option<String>,
	/// Use reserved authority `manual` instead of a Linear issue.
	#[arg(long, conflicts_with = "authority")]
	pub(super) manual_authority: bool,
	/// Additional related issues for the commit message.
	#[arg(long, value_name = "ISSUE")]
	pub(super) related: Vec<String>,
	/// Mark the change as breaking.
	#[arg(long)]
	pub(super) breaking: bool,
}
impl CommitCommand {
	pub(super) fn run(&self) -> Result<()> {
		manual::run_commit(
			self.project_config.as_path(),
			&ManualCommitRequest {
				summary: self.summary.clone(),
				authority: self.authority.clone(),
				manual_authority: self.manual_authority,
				related: self.related.clone(),
				breaking: self.breaking,
			},
		)
	}
}

#[derive(Debug, Args)]
pub(super) struct LandCommand {
	#[command(flatten)]
	pub(super) project_config: ProjectConfigArgs,
	/// Tree-change summary for the landed change record.
	#[arg(value_name = "SUMMARY")]
	pub(super) summary: String,
	/// Primary issue that authorizes the merged change. Defaults to the current issue worktree
	/// name.
	#[arg(long, value_name = "ISSUE", conflicts_with = "manual_authority")]
	pub(super) authority: Option<String>,
	/// Use reserved authority `manual` instead of a Linear issue; requires `--pr`.
	#[arg(long, conflicts_with = "authority", requires = "pr")]
	pub(super) manual_authority: bool,
	/// Pull request URL to land. Required with `--manual-authority`; otherwise defaults to the
	/// current review lifecycle record.
	#[arg(long, value_name = "URL")]
	pub(super) pr: Option<String>,
	/// Additional related issues for the landed change record.
	#[arg(long, value_name = "ISSUE")]
	pub(super) related: Vec<String>,
	/// Mark the landed change record as breaking.
	#[arg(long)]
	pub(super) breaking: bool,
}
impl LandCommand {
	pub(super) fn run(&self) -> Result<()> {
		manual::run_land(
			self.project_config.as_path(),
			&ManualLandRequest {
				summary: self.summary.clone(),
				authority: self.authority.clone(),
				manual_authority: self.manual_authority,
				pr_url: self.pr.clone(),
				related: self.related.clone(),
				breaking: self.breaking,
			},
		)
	}
}
