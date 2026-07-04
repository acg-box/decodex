use std::path::PathBuf;

use clap::Args;

use crate::{RadarRefreshQueueRequest, prelude::Result};

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
	pub(in crate::cli::commands) fn run(&self) -> Result<()> {
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
