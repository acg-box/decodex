use std::{path::Path, process::Command};

use crate::prelude::{Result, eyre};

#[derive(Clone, Default, Eq, PartialEq)]
pub(crate) enum GitSigningConfig {
	#[default]
	Preserve,
	DisableInherited,
	SigningKey(String),
}
impl GitSigningConfig {
	pub(crate) fn from_local_git_config(repo_root: &Path) -> Result<Self> {
		let output = Command::new("git")
			.arg("-C")
			.arg(repo_root)
			.args(["config", "--local", "--includes", "--get", "user.signingkey"])
			.output()?;

		if output.status.success() {
			let signing_key = String::from_utf8_lossy(&output.stdout).trim().to_owned();

			return if signing_key.is_empty() {
				Ok(Self::DisableInherited)
			} else {
				Ok(Self::SigningKey(signing_key))
			};
		}
		if output.status.code() == Some(1) {
			return Ok(Self::Preserve);
		}

		let stderr = String::from_utf8_lossy(&output.stderr);

		eyre::bail!(
			"Failed to inspect local Git signing key in `{}`: {}",
			repo_root.display(),
			stderr.trim()
		);
	}
}
