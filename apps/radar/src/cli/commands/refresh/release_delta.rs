use std::path::PathBuf;

use clap::Args;

use crate::{RadarRefreshReleaseDeltaRequest, prelude::Result};

#[derive(Debug, Args)]
pub(in crate::cli) struct RadarRefreshReleaseDeltaCommand {
	#[arg(long, default_value = "openai/codex")]
	repo: String,
	#[arg(
		long,
		value_name = "DIR",
		default_value = crate::paths::DEFAULT_SIGNALS_DIR
	)]
	signals_dir: PathBuf,
	#[arg(
		long,
		value_name = "FILE",
		default_value = crate::paths::DEFAULT_RELEASE_DELTA_OUT
	)]
	out: PathBuf,
	#[arg(long, default_value = "rust-v")]
	tag_prefix: String,
	#[arg(long)]
	token_env: Option<String>,
	#[arg(long, default_value_t = 0)]
	stable_limit: usize,
	#[arg(long, default_value_t = 0)]
	preview_limit: usize,
	#[arg(long, default_value_t = 24)]
	pair_limit: usize,
	#[arg(long, default_value = "rust-v0.116.0")]
	min_stable_tag: String,
	#[arg(long)]
	dry_run: bool,
}
impl RadarRefreshReleaseDeltaCommand {
	pub(in crate::cli::commands) fn run(&self) -> Result<()> {
		let report = crate::refresh_release_delta(&RadarRefreshReleaseDeltaRequest {
			repo: self.repo.clone(),
			signals_dir: self.signals_dir.clone(),
			out: self.out.clone(),
			tag_prefix: self.tag_prefix.clone(),
			token_env: self.token_env.clone(),
			stable_limit: self.stable_limit,
			preview_limit: self.preview_limit,
			pair_limit: self.pair_limit,
			min_stable_tag: self.min_stable_tag.clone(),
			dry_run: self.dry_run,
		})?;

		println!("{}", serde_json::to_string_pretty(&report)?);

		Ok(())
	}
}
