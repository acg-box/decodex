use std::path::PathBuf;

/// Request to build a deterministic GitHub change bundle.
#[derive(Debug)]
pub(crate) struct RadarBundleBuildRequest {
	/// GitHub repository in `owner/name` form.
	pub(crate) repo: String,
	/// Pull request number to fetch.
	pub(crate) pr: Option<u64>,
	/// Commit SHA to fetch when PR context is unavailable.
	pub(crate) commit: Option<String>,
	/// Skip commit-to-PR promotion when building from a commit.
	pub(crate) force_commit_only: bool,
	/// Optional environment variable name holding a GitHub token.
	pub(crate) token_env: Option<String>,
	/// Output path for the bundle JSON artifact.
	pub(crate) out: PathBuf,
	/// Additional note strings to store in the bundle.
	pub(crate) notes: Vec<String>,
}

/// Request to validate GitHub change bundle JSON artifacts.
#[derive(Debug)]
pub(crate) struct RadarBundleValidateRequest {
	/// Bundle JSON files or directories to validate.
	pub(crate) paths: Vec<PathBuf>,
}
