use std::path::PathBuf;

use clap::Args;

use crate::{SocialValidationReport, prelude::Result};

#[derive(Debug, Args)]
pub(super) struct ValidateSocialCommand {
	#[arg(value_name = "PATH")]
	paths: Vec<PathBuf>,
}
impl ValidateSocialCommand {
	pub(super) fn run(&self) -> Result<()> {
		let SocialValidationReport { checked_files, errors } = crate::validate_social(&self.paths)?;

		println!("validated {checked_files} social artifact file(s)");
		debug_assert!(errors.is_empty());

		Ok(())
	}
}
