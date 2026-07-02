use std::path::PathBuf;

use clap::Args;

use crate::{RadarValidateRequest, prelude::Result};

#[derive(Debug, Args)]
pub(in crate::cli) struct RadarValidateCommand {
	#[arg(value_name = "PATH")]
	paths: Vec<PathBuf>,
}
impl RadarValidateCommand {
	pub(super) fn run(&self) -> Result<()> {
		let report = crate::validate(&RadarValidateRequest { paths: self.paths.clone() })?;

		println!("{report:#?}");

		Ok(())
	}
}
