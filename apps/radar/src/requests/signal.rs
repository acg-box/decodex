use std::path::PathBuf;

/// Request to render one `signal_entry/v1` artifact from a bundle and analysis draft.
#[derive(Debug)]
pub(crate) struct RadarRenderSignalRequest {
	/// Path to a `github_change_bundle/v1` JSON artifact.
	pub(crate) bundle: PathBuf,
	/// Path to a Codex-owned `analysis_draft` JSON artifact.
	pub(crate) analysis: PathBuf,
	/// Path to write the rendered `signal_entry/v1` artifact.
	pub(crate) out: PathBuf,
	/// Optional publication timestamp override.
	pub(crate) published_at: Option<String>,
}

/// Summary of a rendered signal artifact.
#[derive(Debug, Eq, PartialEq)]
pub(crate) struct RadarRenderSignalReport {
	/// Path that received the rendered signal artifact.
	pub(crate) out: PathBuf,
}
