use clap::Args;

use crate::{
	archive_hygiene::{self, ArchiveHygieneRequest},
	cli::ProjectConfigArgs,
	prelude::Result,
};

#[derive(Debug, Args)]
pub(in crate::cli) struct ArchiveLinearCommand {
	#[command(flatten)]
	pub(in crate::cli) project_config: ProjectConfigArgs,
	/// Repo label scope to inspect, for example `repo:decodex`.
	#[arg(long = "repo-label", value_name = "LABEL", required = true)]
	pub(in crate::cli) repo_labels: Vec<String>,
	/// Archive only issues last updated more than this many days ago.
	#[arg(long, value_name = "DAYS", default_value_t = 30)]
	pub(in crate::cli) older_than_days: u32,
	/// Perform the archive mutation. Omit this flag for the dry-run candidate report.
	#[arg(long)]
	pub(in crate::cli) execute: bool,
}
impl ArchiveLinearCommand {
	pub(in crate::cli) fn run(&self) -> Result<()> {
		archive_hygiene::run(
			self.project_config.as_path(),
			&ArchiveHygieneRequest {
				repo_labels: self.repo_labels.clone(),
				older_than_days: self.older_than_days,
				execute: self.execute,
			},
		)
	}
}
