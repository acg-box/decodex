use std::path::PathBuf;

use clap::Args;

use crate::{RadarLedgerIngestRequest, prelude::Result};

#[derive(Debug, Args)]
pub(super) struct RadarLedgerIngestCommand {
	#[arg(long, value_name = "FILE")]
	db_path: Option<PathBuf>,
	#[arg(long, value_name = "FILE")]
	bundle_path: PathBuf,
	#[arg(long, value_name = "FILE")]
	analysis_path: Option<PathBuf>,
	#[arg(long, value_name = "FILE")]
	signal_path: Option<PathBuf>,
}
impl RadarLedgerIngestCommand {
	pub(super) fn run(&self) -> Result<()> {
		let summary = crate::ledger_ingest(&RadarLedgerIngestRequest {
			db_path: self.db_path.clone().unwrap_or_else(crate::default_ledger_path),
			bundle_path: self.bundle_path.clone(),
			analysis_path: self.analysis_path.clone(),
			signal_path: self.signal_path.clone(),
		})?;

		println!("{summary:#?}");

		Ok(())
	}
}
