use std::path::PathBuf;

use clap::Args;

use crate::{RadarLedgerIngestExistingRequest, prelude::Result};

#[derive(Debug, Args)]
pub(super) struct RadarLedgerIngestExistingCommand {
	#[arg(long, value_name = "FILE")]
	db_path: Option<PathBuf>,
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
	#[arg(
		long,
		value_name = "DIR",
		default_value = crate::paths::DEFAULT_SIGNALS_DIR
	)]
	signals_dir: PathBuf,
}
impl RadarLedgerIngestExistingCommand {
	pub(super) fn run(&self) -> Result<()> {
		let summary = crate::ledger_ingest_existing(&RadarLedgerIngestExistingRequest {
			db_path: self.db_path.clone().unwrap_or_else(crate::default_ledger_path),
			bundles_dir: self.bundles_dir.clone(),
			analysis_dir: self.analysis_dir.clone(),
			signals_dir: self.signals_dir.clone(),
		})?;

		println!("{summary:#?}");

		Ok(())
	}
}
