use std::path::PathBuf;

use clap::{Args, Subcommand};

use crate::{
	RadarLedgerArtifactLinkRequest, RadarLedgerBootstrapRequest, RadarLedgerIngestExistingRequest,
	RadarLedgerIngestRequest, RadarLedgerSummaryRequest, prelude::Result,
};

#[derive(Debug, Args)]
pub(in crate::cli) struct RadarLedgerCommand {
	#[command(subcommand)]
	command: RadarLedgerSubcommand,
}
impl RadarLedgerCommand {
	pub(super) fn run(&self) -> Result<()> {
		match &self.command {
			RadarLedgerSubcommand::Bootstrap(args) => args.run(),
			RadarLedgerSubcommand::Ingest(args) => args.run(),
			RadarLedgerSubcommand::IngestExisting(args) => args.run(),
			RadarLedgerSubcommand::ArtifactLink(args) => args.run(),
			RadarLedgerSubcommand::Summary(args) => args.run(),
		}
	}
}

#[derive(Debug, Args)]
struct RadarLedgerBootstrapCommand {
	#[arg(long, value_name = "FILE")]
	db_path: Option<PathBuf>,
}
impl RadarLedgerBootstrapCommand {
	fn run(&self) -> Result<()> {
		crate::ledger_bootstrap(&RadarLedgerBootstrapRequest {
			db_path: self.db_path.clone().unwrap_or_else(crate::default_ledger_path),
		})?;

		Ok(())
	}
}

#[derive(Debug, Args)]
struct RadarLedgerIngestCommand {
	#[arg(long, value_name = "FILE")]
	db_path: Option<PathBuf>,
	#[arg(long, value_name = "FILE")]
	bundle_path: PathBuf,
	#[arg(long, value_name = "FILE")]
	analysis_path: Option<PathBuf>,
	#[arg(long, value_name = "FILE")]
	signal_path: Option<PathBuf>,
}
impl RadarLedgerIngestCommand {
	fn run(&self) -> Result<()> {
		let summary = crate::ledger_ingest(&RadarLedgerIngestRequest {
			db_path: self.db_path.clone().unwrap_or_else(crate::default_ledger_path),
			bundle_path: self.bundle_path.clone(),
			analysis_path: self.analysis_path.clone(),
			signal_path: self.signal_path.clone(),
		})?;

		println!("{summary:#?}");

		Ok(())
	}
}

#[derive(Debug, Args)]
struct RadarLedgerIngestExistingCommand {
	#[arg(long, value_name = "FILE")]
	db_path: Option<PathBuf>,
	#[arg(
		long,
		value_name = "DIR",
		default_value = crate::paths::DEFAULT_BUNDLES_DIR
	)]
	bundles_dir: PathBuf,
	#[arg(
		long,
		value_name = "DIR",
		default_value = crate::paths::DEFAULT_ANALYSIS_DIR
	)]
	analysis_dir: PathBuf,
	#[arg(
		long,
		value_name = "DIR",
		default_value = crate::paths::DEFAULT_SIGNALS_DIR
	)]
	signals_dir: PathBuf,
}
impl RadarLedgerIngestExistingCommand {
	fn run(&self) -> Result<()> {
		let summary = crate::ledger_ingest_existing(&RadarLedgerIngestExistingRequest {
			db_path: self.db_path.clone().unwrap_or_else(crate::default_ledger_path),
			bundles_dir: self.bundles_dir.clone(),
			analysis_dir: self.analysis_dir.clone(),
			signals_dir: self.signals_dir.clone(),
		})?;

		println!("{summary:#?}");

		Ok(())
	}
}

#[derive(Debug, Args)]
struct RadarLedgerArtifactLinkCommand {
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
	fn run(&self) -> Result<()> {
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

#[derive(Debug, Args)]
struct RadarLedgerSummaryCommand {
	#[arg(long, value_name = "FILE")]
	db_path: Option<PathBuf>,
}
impl RadarLedgerSummaryCommand {
	fn run(&self) -> Result<()> {
		let summary = crate::ledger_summary(&RadarLedgerSummaryRequest {
			db_path: self.db_path.clone().unwrap_or_else(crate::default_ledger_path),
		})?;

		println!("{summary:#?}");

		Ok(())
	}
}

#[derive(Debug, Subcommand)]
enum RadarLedgerSubcommand {
	/// Initialize the local Radar ledger schema.
	Bootstrap(RadarLedgerBootstrapCommand),
	/// Ingest one bundle and optional derived artifacts.
	Ingest(RadarLedgerIngestCommand),
	/// Ingest existing hot Radar artifact directories.
	IngestExisting(RadarLedgerIngestExistingCommand),
	/// Link an artifact path to an existing Radar subject.
	ArtifactLink(RadarLedgerArtifactLinkCommand),
	/// Summarize the local Radar ledger.
	Summary(RadarLedgerSummaryCommand),
}
