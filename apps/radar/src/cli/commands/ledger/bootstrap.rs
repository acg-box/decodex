use std::path::PathBuf;

use clap::Args;

use crate::{RadarLedgerBootstrapRequest, prelude::Result};

#[derive(Debug, Args)]
pub(super) struct RadarLedgerBootstrapCommand {
	#[arg(long, value_name = "FILE")]
	db_path: Option<PathBuf>,
}
impl RadarLedgerBootstrapCommand {
	pub(super) fn run(&self) -> Result<()> {
		crate::ledger_bootstrap(&RadarLedgerBootstrapRequest {
			db_path: self.db_path.clone().unwrap_or_else(crate::default_ledger_path),
		})?;

		Ok(())
	}
}
