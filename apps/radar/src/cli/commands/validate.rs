use std::path::PathBuf;

use clap::Args;

use crate::{RadarValidateRequest, prelude::Result};

#[derive(Debug, Args)]
pub(in crate::cli) struct RadarValidateCommand {
	#[arg(value_name = "PATH")]
	paths: Vec<PathBuf>,
	#[arg(long, value_name = "HOURS")]
	max_age_hours: Option<u64>,
	/// Permit an empty default cache during an explicit first-run bootstrap.
	#[arg(long, conflicts_with = "paths")]
	bootstrap: bool,
}
impl RadarValidateCommand {
	pub(super) fn run(&self) -> Result<()> {
		let report = crate::validate(&RadarValidateRequest {
			paths: self.paths.clone(),
			max_age_hours: self.max_age_hours,
			bootstrap: self.bootstrap,
		})?;

		println!("{}", serde_json::to_string_pretty(&report)?);

		Ok(())
	}
}
