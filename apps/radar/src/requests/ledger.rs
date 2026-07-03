use std::path::PathBuf;

/// Request to initialize the local Radar SQLite ledger.
#[derive(Debug)]
pub(crate) struct RadarLedgerBootstrapRequest {
	/// SQLite ledger path.
	pub(crate) db_path: PathBuf,
}

/// Request to ingest one bundle and optional derived artifacts into the Radar ledger.
#[derive(Debug)]
pub(crate) struct RadarLedgerIngestRequest {
	/// SQLite ledger path.
	pub(crate) db_path: PathBuf,
	/// Path to a `github_change_bundle/v1` JSON artifact.
	pub(crate) bundle_path: PathBuf,
	/// Optional analysis draft artifact path.
	pub(crate) analysis_path: Option<PathBuf>,
	/// Optional rendered `signal_entry/v1` artifact path.
	pub(crate) signal_path: Option<PathBuf>,
}

/// Request to ingest existing checked-in Radar artifacts into the Radar ledger.
#[derive(Debug)]
pub(crate) struct RadarLedgerIngestExistingRequest {
	/// SQLite ledger path.
	pub(crate) db_path: PathBuf,
	/// Directory containing `github_change_bundle/v1` JSON artifacts.
	pub(crate) bundles_dir: PathBuf,
	/// Directory containing analysis draft artifacts.
	pub(crate) analysis_dir: PathBuf,
	/// Directory containing rendered `signal_entry/v1` artifacts.
	pub(crate) signals_dir: PathBuf,
}

/// Request to attach one artifact path to an existing Radar subject.
#[derive(Debug)]
pub(crate) struct RadarLedgerArtifactLinkRequest {
	/// SQLite ledger path.
	pub(crate) db_path: PathBuf,
	/// GitHub repository in `owner/name` form.
	pub(crate) repo: String,
	/// Subject kind, either `commit` or `pr`.
	pub(crate) subject_kind: String,
	/// Subject id, either a commit SHA or pull request number.
	pub(crate) subject_id: String,
	/// Artifact kind stored in the ledger.
	pub(crate) artifact_kind: String,
	/// Artifact path to digest and link.
	pub(crate) path: PathBuf,
}

/// Request to summarize the local Radar SQLite ledger.
#[derive(Debug)]
pub(crate) struct RadarLedgerSummaryRequest {
	/// SQLite ledger path.
	pub(crate) db_path: PathBuf,
}
