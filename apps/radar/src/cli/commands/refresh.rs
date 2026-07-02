use std::path::PathBuf;

use clap::Args;

use crate::{
	RadarBackfillReleaseRangeRequest, RadarRefreshQueueRequest, RadarRefreshReleaseDeltaRequest,
	prelude::Result,
};

#[derive(Debug, Args)]
pub(in crate::cli) struct RadarRefreshUpstreamQueueCommand {
	#[arg(long, default_value = "openai/codex")]
	repo: String,
	#[arg(long, default_value_t = 40)]
	search_limit: usize,
	#[arg(
		long,
		value_name = "DIR",
		default_value = crate::paths::DEFAULT_SIGNALS_DIR
	)]
	signals_dir: PathBuf,
	#[arg(
		long,
		value_name = "FILE",
		default_value = crate::paths::DEFAULT_QUEUE_OUT
	)]
	queue_out: PathBuf,
	#[arg(long)]
	token_env: Option<String>,
	#[arg(long, value_name = "FILE")]
	ledger: Option<PathBuf>,
	#[arg(long)]
	no_ledger: bool,
	#[arg(long)]
	dry_run: bool,
}
impl RadarRefreshUpstreamQueueCommand {
	pub(super) fn run(&self) -> Result<()> {
		let report = crate::refresh_queue(&RadarRefreshQueueRequest {
			repo: self.repo.clone(),
			search_limit: self.search_limit,
			signals_dir: self.signals_dir.clone(),
			queue_out: self.queue_out.clone(),
			token_env: self.token_env.clone(),
			ledger: self.ledger.clone().unwrap_or_else(crate::default_ledger_path),
			no_ledger: self.no_ledger,
			dry_run: self.dry_run,
		})?;

		println!("{report:#?}");

		Ok(())
	}
}

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
	pub(super) fn run(&self) -> Result<()> {
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

		println!("{report:#?}");

		Ok(())
	}
}

#[derive(Debug, Args)]
pub(in crate::cli) struct RadarBackfillReleaseRangeCommand {
	#[arg(long, default_value = "openai/codex")]
	repo: String,
	#[arg(
		long,
		value_name = "FILE",
		default_value = crate::paths::DEFAULT_RELEASE_DELTA_OUT
	)]
	release_delta: PathBuf,
	#[arg(long)]
	stable_tag: Option<String>,
	#[arg(long)]
	preview_tag: Option<String>,
	#[arg(
		long,
		value_name = "DIR",
		default_value = crate::paths::DEFAULT_SIGNALS_DIR
	)]
	signals_dir: PathBuf,
	#[arg(
		long,
		value_name = "DIR",
		default_value = crate::paths::DEFAULT_BUNDLES_DIR
	)]
	bundles_dir: PathBuf,
	#[arg(
		long,
		value_name = "DIR",
		default_value = crate::paths::DEFAULT_ANALYSIS_DIR
	)]
	analysis_dir: PathBuf,
	#[arg(long)]
	token_env: Option<String>,
	#[arg(long, default_value = "codex")]
	codex_bin: String,
	#[arg(long)]
	model: Option<String>,
	#[arg(long)]
	max_prs: Option<usize>,
	#[arg(long)]
	dry_run: bool,
	#[arg(long)]
	refresh_release_delta_first: bool,
	#[arg(long)]
	refresh_stable_limit: Option<usize>,
	#[arg(long)]
	refresh_preview_limit: Option<usize>,
	#[arg(long)]
	refresh_pair_limit: Option<usize>,
	#[arg(long, default_value = "python3")]
	python_bin: String,
}
impl RadarBackfillReleaseRangeCommand {
	pub(super) fn run(&self) -> Result<()> {
		let report = crate::backfill_release_range(&RadarBackfillReleaseRangeRequest {
			repo: self.repo.clone(),
			release_delta: self.release_delta.clone(),
			stable_tag: self.stable_tag.clone(),
			preview_tag: self.preview_tag.clone(),
			signals_dir: self.signals_dir.clone(),
			bundles_dir: self.bundles_dir.clone(),
			analysis_dir: self.analysis_dir.clone(),
			token_env: self.token_env.clone(),
			codex_bin: self.codex_bin.clone(),
			model: self.model.clone(),
			max_prs: self.max_prs,
			dry_run: self.dry_run,
			refresh_release_delta_first: self.refresh_release_delta_first,
			refresh_stable_limit: self.refresh_stable_limit,
			refresh_preview_limit: self.refresh_preview_limit,
			refresh_pair_limit: self.refresh_pair_limit,
			python_bin: self.python_bin.clone(),
		})?;

		println!("{report}");

		Ok(())
	}
}
