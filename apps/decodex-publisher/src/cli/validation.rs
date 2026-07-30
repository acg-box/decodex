use std::path::PathBuf;

use clap::Args;

use crate::{SocialValidationReport, prelude::Result};

#[derive(Debug, Args)]
pub(super) struct ValidateSocialCommand {
	#[arg(value_name = "PATH")]
	paths: Vec<PathBuf>,

	/// Validate one artifact without applying cross-file constraints.
	#[arg(long, value_name = "PATH", conflicts_with = "paths")]
	artifact_only: Option<PathBuf>,
}
impl ValidateSocialCommand {
	pub(super) fn run(&self) -> Result<()> {
		if let Some(path) = &self.artifact_only {
			let root = crate::repo_root()?;
			let path = crate::resolve_against(&root, path);
			let payload = crate::load_json(&path)?;

			crate::validate_generated_social_artifact(&payload)?;
			println!("validated 1 social artifact file(s)");

			return Ok(());
		}

		let SocialValidationReport { checked_files, errors } = crate::validate_social(&self.paths)?;

		println!("validated {checked_files} social state file(s)");
		debug_assert!(errors.is_empty());

		Ok(())
	}
}
