use std::path::PathBuf;

use serde::{Deserialize, Serialize};

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

/// Exact-byte evidence for one installed deterministic bundle.
#[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RadarBundleBuildReceipt {
	/// Versioned receipt contract.
	pub(crate) schema: String,
	/// Successful terminal state for the installed and read-back bundle.
	pub(crate) status: String,
	/// SHA-256 of the exact bytes read back from the output path.
	pub(crate) bundle_sha256: String,
	/// Exact size of the installed bundle bytes.
	pub(crate) bundle_bytes: u64,
	/// Bundle analysis mode copied from the validated installed bytes.
	pub(crate) analysis_mode: String,
	/// Number of commit records in the installed bundle.
	pub(crate) commit_count: u32,
	/// Number of file records in the installed bundle.
	pub(crate) file_count: u32,
	/// Number of file records with a non-empty patch excerpt.
	pub(crate) patch_excerpt_count: u32,
	/// Number of documentation references in the installed bundle.
	pub(crate) docs_ref_count: u32,
	/// Number of example references in the installed bundle.
	pub(crate) examples_ref_count: u32,
}

/// Request to validate GitHub change bundle JSON artifacts.
#[derive(Debug)]
pub(crate) struct RadarBundleValidateRequest {
	/// Bundle JSON files or directories to validate.
	pub(crate) paths: Vec<PathBuf>,
}
