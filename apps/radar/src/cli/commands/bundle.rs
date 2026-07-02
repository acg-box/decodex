use std::path::PathBuf;

use clap::{Args, Subcommand};

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
struct RadarBundleBuildCommand {
	#[arg(long, default_value = "openai/codex")]
	repo: String,
	#[arg(long)]
	pr: Option<u64>,
	#[arg(long)]
	commit: Option<String>,
	#[arg(long)]
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
		let out = crate::build_bundle(&RadarBundleBuildRequest {
			repo: self.repo.clone(),
			pr: self.pr,
			commit: self.commit.clone(),
			force_commit_only: self.force_commit_only,
			token_env: self.token_env.clone(),
			out: self.out.clone(),
			notes: self.notes.clone(),
		})?;

		println!("{}", out.display());

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
