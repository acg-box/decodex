use std::path::PathBuf;

use clap::{ArgGroup, Args, Subcommand};

use crate::{RadarBundleBuildRequest, RadarBundleValidateRequest, prelude::Result};

#[derive(Debug, Args)]
pub(in crate::cli) struct RadarBundleCommand {
	#[command(subcommand)]
	command: RadarBundleSubcommand,
}
impl RadarBundleCommand {
	pub(super) fn run(&self) -> Result<()> {
		match &self.command {
			RadarBundleSubcommand::Build(args) => args.run(),
			RadarBundleSubcommand::Validate(args) => args.run(),
		}
	}
}

#[derive(Debug, Args)]
#[command(group(ArgGroup::new("subject").required(true).args(["pr", "commit"])))]
struct RadarBundleBuildCommand {
	#[arg(long, default_value = "openai/codex")]
	repo: String,
	#[arg(long)]
	pr: Option<u64>,
	#[arg(long)]
	commit: Option<String>,
	#[arg(long, requires = "commit", conflicts_with = "pr")]
	force_commit_only: bool,
	#[arg(long)]
	token_env: Option<String>,
	#[arg(long, value_name = "FILE")]
	out: PathBuf,
	#[arg(long = "note")]
	notes: Vec<String>,
}
impl RadarBundleBuildCommand {
	fn run(&self) -> Result<()> {
		let receipt = crate::build_bundle(&RadarBundleBuildRequest {
			repo: self.repo.clone(),
			pr: self.pr,
			commit: self.commit.clone(),
			force_commit_only: self.force_commit_only,
			token_env: self.token_env.clone(),
			out: self.out.clone(),
			notes: self.notes.clone(),
		})?;

		println!("{}", serde_json::to_string_pretty(&receipt)?);

		Ok(())
	}
}

#[derive(Debug, Args)]
struct RadarBundleValidateCommand {
	#[arg(value_name = "PATH")]
	paths: Vec<PathBuf>,
}
impl RadarBundleValidateCommand {
	fn run(&self) -> Result<()> {
		let report =
			crate::validate_bundles(&RadarBundleValidateRequest { paths: self.paths.clone() })?;

		println!("{report:#?}");

		Ok(())
	}
}

#[derive(Debug, Subcommand)]
enum RadarBundleSubcommand {
	/// Build a deterministic GitHub change bundle.
	Build(RadarBundleBuildCommand),
	/// Validate GitHub change bundle artifacts.
	Validate(RadarBundleValidateCommand),
}

#[cfg(test)]
mod tests {
	use clap::Parser as _;

	use crate::cli::Cli;

	#[test]
	fn build_requires_one_subject_and_commit_only_requires_a_commit() {
		let base = ["radar", "bundle", "build", "--out", "bundle.json"];

		for subject in [
			vec!["--pr", "22414"],
			vec!["--commit", "abc123"],
			vec!["--commit", "abc123", "--force-commit-only"],
		] {
			assert!(Cli::try_parse_from(base.into_iter().chain(subject)).is_ok());
		}
		for subject in [
			vec![],
			vec!["--pr", "22414", "--commit", "abc123"],
			vec!["--pr", "22414", "--force-commit-only"],
			vec!["--force-commit-only"],
		] {
			assert!(
				Cli::try_parse_from(base.into_iter().chain(subject.clone())).is_err(),
				"{subject:?}"
			);
		}
	}
}
