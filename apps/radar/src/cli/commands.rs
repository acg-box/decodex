mod bundle;
mod cache;
mod ledger;
mod refresh;
mod render;
mod validate;

use clap::Subcommand;

use crate::{
	cli::commands::{
		bundle::RadarBundleCommand,
		cache::RadarCacheGcCommand,
		ledger::RadarLedgerCommand,
		refresh::{
			RadarBackfillReleaseRangeCommand, RadarRefreshReleaseDeltaCommand,
			RadarRefreshUpstreamQueueCommand,
		},
		render::RadarRenderSignalCommand,
		validate::RadarValidateCommand,
	},
	prelude::Result,
};

#[derive(Debug, Subcommand)]
pub(super) enum RadarSubcommand {
	/// Validate Radar JSON artifacts.
	Validate(RadarValidateCommand),
	/// Apply deterministic retention to the owner-only local Radar cache.
	CacheGc(RadarCacheGcCommand),
	/// Refresh the upstream Codex review queue artifact.
	RefreshUpstreamQueue(RadarRefreshUpstreamQueueCommand),
	/// Refresh the release-delta artifact.
	RefreshReleaseDelta(RadarRefreshReleaseDeltaCommand),
	/// Build or validate GitHub change bundles.
	Bundle(RadarBundleCommand),
	/// Render a signal entry from a bundle and analysis draft.
	RenderSignal(RadarRenderSignalCommand),
	/// Backfill unpublished signal entries from a release comparison.
	BackfillReleaseRange(RadarBackfillReleaseRangeCommand),
	/// Manage the local Radar ledger.
	Ledger(RadarLedgerCommand),
}
impl RadarSubcommand {
	pub(super) fn run(&self) -> Result<()> {
		match self {
			Self::Validate(args) => args.run(),
			Self::CacheGc(args) => args.run(),
			Self::RefreshUpstreamQueue(args) => args.run(),
			Self::RefreshReleaseDelta(args) => args.run(),
			Self::Bundle(args) => args.run(),
			Self::RenderSignal(args) => args.run(),
			Self::BackfillReleaseRange(args) => args.run(),
			Self::Ledger(args) => args.run(),
		}
	}
}
