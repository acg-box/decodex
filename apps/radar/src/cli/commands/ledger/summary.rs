use std::path::PathBuf;

use clap::Args;

use crate::{RadarLedgerSummaryRequest, prelude::Result};

#[derive(Debug, Args)]
pub(super) struct RadarLedgerSummaryCommand {
	#[arg(long, value_name = "FILE")]
	db_path: Option<PathBuf>,
}
impl RadarLedgerSummaryCommand {
	pub(super) fn run(&self) -> Result<()> {
		let summary = crate::ledger_summary(&RadarLedgerSummaryRequest {
			db_path: self.db_path.clone().unwrap_or_else(crate::default_ledger_path),
		})?;

		println!("{summary:#?}");

		Ok(())
	}
}
