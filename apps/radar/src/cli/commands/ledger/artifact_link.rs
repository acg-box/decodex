use std::path::PathBuf;

use clap::Args;

use crate::{RadarLedgerArtifactLinkRequest, prelude::Result};

#[derive(Debug, Args)]
pub(super) struct RadarLedgerArtifactLinkCommand {
	#[arg(long, value_name = "FILE")]
	db_path: Option<PathBuf>,
	#[arg(long, default_value = "openai/codex")]
	repo: String,
	#[arg(long)]
	subject_kind: String,
	#[arg(long)]
	subject_id: String,
	#[arg(long)]
	artifact_kind: String,
	#[arg(long, value_name = "FILE")]
	path: PathBuf,
}
impl RadarLedgerArtifactLinkCommand {
	pub(super) fn run(&self) -> Result<()> {
		let summary = crate::ledger_artifact_link(&RadarLedgerArtifactLinkRequest {
			db_path: self.db_path.clone().unwrap_or_else(crate::default_ledger_path),
			repo: self.repo.clone(),
			subject_kind: self.subject_kind.clone(),
			subject_id: self.subject_id.clone(),
			artifact_kind: self.artifact_kind.clone(),
			path: self.path.clone(),
		})?;

		println!("{summary:#?}");

		Ok(())
	}
}
