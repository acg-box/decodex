mod artifact_link;
mod bootstrap;
mod ingest;
mod ingest_existing;
mod summary;

use clap::{Args, Subcommand};

use crate::{
	cli::commands::ledger::{
		artifact_link::RadarLedgerArtifactLinkCommand, bootstrap::RadarLedgerBootstrapCommand,
		ingest::RadarLedgerIngestCommand, ingest_existing::RadarLedgerIngestExistingCommand,
		summary::RadarLedgerSummaryCommand,
	},
	prelude::Result,
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
