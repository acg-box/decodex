use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

use crate::{
	self as radar, RadarBackfillReleaseRangeRequest, RadarBundleBuildRequest,
	RadarBundleValidateRequest, RadarLedgerArtifactLinkRequest, RadarLedgerBootstrapRequest,
	RadarLedgerIngestExistingRequest, RadarLedgerIngestRequest, RadarLedgerSummaryRequest,
	RadarRefreshQueueRequest, RadarRefreshReleaseDeltaRequest, RadarRenderSignalRequest,
	RadarSocialReservePublishRequest, RadarValidateRequest, prelude::Result,
};

/// Root CLI parser for the Radar auxiliary tool.
#[derive(Debug, Parser)]
#[command(
	about = "Auxiliary Radar automation and artifact tooling.",
	version,
	arg_required_else_help = true,
	rename_all = "kebab",
	subcommand_required = true
)]
pub(crate) struct Cli {
	#[command(subcommand)]
	command: RadarSubcommand,
}
impl Cli {
	pub(crate) fn run(&self) -> Result<()> {
		match &self.command {
			RadarSubcommand::Validate(args) => args.run(),
			RadarSubcommand::RefreshUpstreamQueue(args) => args.run(),
			RadarSubcommand::RefreshReleaseDelta(args) => args.run(),
			RadarSubcommand::Bundle(args) => args.run(),
			RadarSubcommand::Social(args) => args.run(),
			RadarSubcommand::RenderSignal(args) => args.run(),
			RadarSubcommand::BackfillReleaseRange(args) => args.run(),
			RadarSubcommand::Ledger(args) => args.run(),
		}
	}
}

#[derive(Debug, Args)]
struct RadarValidateCommand {
	#[arg(value_name = "PATH")]
	paths: Vec<PathBuf>,
}
impl RadarValidateCommand {
	fn run(&self) -> Result<()> {
		let report = radar::validate(&RadarValidateRequest { paths: self.paths.clone() })?;

		println!("{report:#?}");

		Ok(())
	}
}

#[derive(Debug, Args)]
struct RadarRefreshUpstreamQueueCommand {
	#[arg(long, default_value = "openai/codex")]
	repo: String,
	#[arg(long, default_value_t = 40)]
	search_limit: usize,
	#[arg(
		long,
		value_name = "DIR",
		default_value = ".agent/automations/decodex/cache/site-content/signals"
	)]
	signals_dir: PathBuf,
	#[arg(
		long,
		value_name = "FILE",
		default_value = ".agent/automations/decodex/cache/github/review-queue/openai-codex-latest.json"
	)]
	queue_out: PathBuf,
	#[arg(long)]
	token_env: Option<String>,
	#[arg(long, value_name = "FILE")]
	ledger: Option<PathBuf>,
	#[arg(long)]
	no_ledger: bool,
	#[arg(long)]
	dry_run: bool,
}
impl RadarRefreshUpstreamQueueCommand {
	fn run(&self) -> Result<()> {
		let report = radar::refresh_queue(&RadarRefreshQueueRequest {
			repo: self.repo.clone(),
			search_limit: self.search_limit,
			signals_dir: self.signals_dir.clone(),
			queue_out: self.queue_out.clone(),
			token_env: self.token_env.clone(),
			ledger: self.ledger.clone().unwrap_or_else(radar::default_ledger_path),
			no_ledger: self.no_ledger,
			dry_run: self.dry_run,
		})?;

		println!("{report:#?}");

		Ok(())
	}
}

#[derive(Debug, Args)]
struct RadarRefreshReleaseDeltaCommand {
	#[arg(long, default_value = "openai/codex")]
	repo: String,
	#[arg(
		long,
		value_name = "DIR",
		default_value = ".agent/automations/decodex/cache/site-content/signals"
	)]
	signals_dir: PathBuf,
	#[arg(
		long,
		value_name = "FILE",
		default_value = ".agent/automations/decodex/cache/site-content/release-deltas/openai-codex-latest.json"
	)]
	out: PathBuf,
	#[arg(long, default_value = "rust-v")]
	tag_prefix: String,
	#[arg(long)]
	token_env: Option<String>,
	#[arg(long, default_value_t = 0)]
	stable_limit: usize,
	#[arg(long, default_value_t = 0)]
	preview_limit: usize,
	#[arg(long, default_value_t = 24)]
	pair_limit: usize,
	#[arg(long, default_value = "rust-v0.116.0")]
	min_stable_tag: String,
	#[arg(long)]
	dry_run: bool,
}
impl RadarRefreshReleaseDeltaCommand {
	fn run(&self) -> Result<()> {
		let report = radar::refresh_release_delta(&RadarRefreshReleaseDeltaRequest {
			repo: self.repo.clone(),
			signals_dir: self.signals_dir.clone(),
			out: self.out.clone(),
			tag_prefix: self.tag_prefix.clone(),
			token_env: self.token_env.clone(),
			stable_limit: self.stable_limit,
			preview_limit: self.preview_limit,
			pair_limit: self.pair_limit,
			min_stable_tag: self.min_stable_tag.clone(),
			dry_run: self.dry_run,
		})?;

		println!("{report:#?}");

		Ok(())
	}
}

#[derive(Debug, Args)]
struct RadarBundleCommand {
	#[command(subcommand)]
	command: RadarBundleSubcommand,
}
impl RadarBundleCommand {
	fn run(&self) -> Result<()> {
		match &self.command {
			RadarBundleSubcommand::Build(args) => args.run(),
			RadarBundleSubcommand::Validate(args) => args.run(),
		}
	}
}

#[derive(Debug, Args)]
struct RadarBundleBuildCommand {
	#[arg(long, default_value = "openai/codex")]
	repo: String,
	#[arg(long)]
	pr: Option<u64>,
	#[arg(long)]
	commit: Option<String>,
	#[arg(long)]
	force_commit_only: bool,
	#[arg(long)]
	token_env: Option<String>,
	#[arg(long, value_name = "FILE")]
	out: PathBuf,
	#[arg(long = "note")]
	notes: Vec<String>,
}
impl RadarBundleBuildCommand {
	fn run(&self) -> Result<()> {
		let out = radar::build_bundle(&RadarBundleBuildRequest {
			repo: self.repo.clone(),
			pr: self.pr,
			commit: self.commit.clone(),
			force_commit_only: self.force_commit_only,
			token_env: self.token_env.clone(),
			out: self.out.clone(),
			notes: self.notes.clone(),
		})?;

		println!("{}", out.display());

		Ok(())
	}
}

#[derive(Debug, Args)]
struct RadarBundleValidateCommand {
	#[arg(value_name = "PATH")]
	paths: Vec<PathBuf>,
}
impl RadarBundleValidateCommand {
	fn run(&self) -> Result<()> {
		let report =
			radar::validate_bundles(&RadarBundleValidateRequest { paths: self.paths.clone() })?;

		println!("{report:#?}");

		Ok(())
	}
}

#[derive(Debug, Args)]
struct RadarSocialCommand {
	#[command(subcommand)]
	command: RadarSocialSubcommand,
}
impl RadarSocialCommand {
	fn run(&self) -> Result<()> {
		match &self.command {
			RadarSocialSubcommand::ReservePublish(args) => args.run(),
		}
	}
}

#[derive(Debug, Args)]
struct RadarSocialReservePublishCommand {
	#[arg(long)]
	slug: String,
	#[arg(long)]
	mode: String,
	#[arg(long)]
	idempotency_key: String,
	#[arg(long)]
	reserved_at: String,
	#[arg(long)]
	expires_at: String,
	#[arg(long)]
	day: String,
	#[arg(long, default_value = "Asia/Shanghai")]
	timezone: String,
	#[arg(long = "candidate", value_name = "FILE")]
	candidate_paths: Vec<PathBuf>,
	#[arg(long = "url")]
	urls: Vec<String>,
	#[arg(long = "duplicate-key")]
	duplicate_keys: Vec<String>,
	#[arg(
		long,
		value_name = "DIR",
		default_value = ".agent/automations/decodex/cache/social/x/reservations"
	)]
	out_dir: PathBuf,
	#[arg(
		long,
		value_name = "DIR",
		default_value = ".agent/automations/decodex/cache/social/x/posts"
	)]
	posts_dir: PathBuf,
	#[arg(long)]
	automation_id: Option<String>,
	#[arg(long)]
	run_id: Option<String>,
	#[arg(long)]
	branch: Option<String>,
	#[arg(long, default_value_t = 8)]
	daily_limit: usize,
	#[arg(long)]
	dry_run: bool,
}
impl RadarSocialReservePublishCommand {
	fn run(&self) -> Result<()> {
		let report = radar::reserve_social_publish(&RadarSocialReservePublishRequest {
			slug: self.slug.clone(),
			mode: self.mode.clone(),
			idempotency_key: self.idempotency_key.clone(),
			reserved_at: self.reserved_at.clone(),
			expires_at: self.expires_at.clone(),
			day: self.day.clone(),
			timezone: self.timezone.clone(),
			candidate_paths: self.candidate_paths.clone(),
			urls: self.urls.clone(),
			duplicate_keys: self.duplicate_keys.clone(),
			out_dir: self.out_dir.clone(),
			posts_dir: self.posts_dir.clone(),
			automation_id: self.automation_id.clone(),
			run_id: self.run_id.clone(),
			branch: self.branch.clone(),
			daily_limit: self.daily_limit,
			dry_run: self.dry_run,
		})?;

		println!("{}", serde_json::to_string_pretty(&report)?);

		Ok(())
	}
}

#[derive(Debug, Args)]
struct RadarRenderSignalCommand {
	#[arg(long, value_name = "FILE")]
	bundle: PathBuf,
	#[arg(long, value_name = "FILE")]
	analysis: PathBuf,
	#[arg(long, value_name = "FILE")]
	out: PathBuf,
	#[arg(long)]
	published_at: Option<String>,
}
impl RadarRenderSignalCommand {
	fn run(&self) -> Result<()> {
		let report = radar::render_signal(&RadarRenderSignalRequest {
			bundle: self.bundle.clone(),
			analysis: self.analysis.clone(),
			out: self.out.clone(),
			published_at: self.published_at.clone(),
		})?;

		println!("{report:#?}");

		Ok(())
	}
}

#[derive(Debug, Args)]
struct RadarBackfillReleaseRangeCommand {
	#[arg(long, default_value = "openai/codex")]
	repo: String,
	#[arg(
		long,
		value_name = "FILE",
		default_value = ".agent/automations/decodex/cache/site-content/release-deltas/openai-codex-latest.json"
	)]
	release_delta: PathBuf,
	#[arg(long)]
	stable_tag: Option<String>,
	#[arg(long)]
	preview_tag: Option<String>,
	#[arg(
		long,
		value_name = "DIR",
		default_value = ".agent/automations/decodex/cache/site-content/signals"
	)]
	signals_dir: PathBuf,
	#[arg(
		long,
		value_name = "DIR",
		default_value = ".agent/automations/decodex/cache/github/bundles"
	)]
	bundles_dir: PathBuf,
	#[arg(
		long,
		value_name = "DIR",
		default_value = ".agent/automations/decodex/cache/generated/analysis"
	)]
	analysis_dir: PathBuf,
	#[arg(long)]
	token_env: Option<String>,
	#[arg(long, default_value = "codex")]
	codex_bin: String,
	#[arg(long)]
	model: Option<String>,
	#[arg(long)]
	max_prs: Option<usize>,
	#[arg(long)]
	dry_run: bool,
	#[arg(long)]
	refresh_release_delta_first: bool,
	#[arg(long)]
	refresh_stable_limit: Option<usize>,
	#[arg(long)]
	refresh_preview_limit: Option<usize>,
	#[arg(long)]
	refresh_pair_limit: Option<usize>,
	#[arg(long, default_value = "python3")]
	python_bin: String,
}
impl RadarBackfillReleaseRangeCommand {
	fn run(&self) -> Result<()> {
		let report = radar::backfill_release_range(&RadarBackfillReleaseRangeRequest {
			repo: self.repo.clone(),
			release_delta: self.release_delta.clone(),
			stable_tag: self.stable_tag.clone(),
			preview_tag: self.preview_tag.clone(),
			signals_dir: self.signals_dir.clone(),
			bundles_dir: self.bundles_dir.clone(),
			analysis_dir: self.analysis_dir.clone(),
			token_env: self.token_env.clone(),
			codex_bin: self.codex_bin.clone(),
			model: self.model.clone(),
			max_prs: self.max_prs,
			dry_run: self.dry_run,
			refresh_release_delta_first: self.refresh_release_delta_first,
			refresh_stable_limit: self.refresh_stable_limit,
			refresh_preview_limit: self.refresh_preview_limit,
			refresh_pair_limit: self.refresh_pair_limit,
			python_bin: self.python_bin.clone(),
		})?;

		println!("{report}");

		Ok(())
	}
}

#[derive(Debug, Args)]
struct RadarLedgerCommand {
	#[command(subcommand)]
	command: RadarLedgerSubcommand,
}
impl RadarLedgerCommand {
	fn run(&self) -> Result<()> {
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
		radar::ledger_bootstrap(&RadarLedgerBootstrapRequest {
			db_path: self.db_path.clone().unwrap_or_else(radar::default_ledger_path),
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
		let summary = radar::ledger_ingest(&RadarLedgerIngestRequest {
			db_path: self.db_path.clone().unwrap_or_else(radar::default_ledger_path),
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
		default_value = ".agent/automations/decodex/cache/github/bundles"
	)]
	bundles_dir: PathBuf,
	#[arg(
		long,
		value_name = "DIR",
		default_value = ".agent/automations/decodex/cache/generated/analysis"
	)]
	analysis_dir: PathBuf,
	#[arg(
		long,
		value_name = "DIR",
		default_value = ".agent/automations/decodex/cache/site-content/signals"
	)]
	signals_dir: PathBuf,
}
impl RadarLedgerIngestExistingCommand {
	fn run(&self) -> Result<()> {
		let summary = radar::ledger_ingest_existing(&RadarLedgerIngestExistingRequest {
			db_path: self.db_path.clone().unwrap_or_else(radar::default_ledger_path),
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
		let summary = radar::ledger_artifact_link(&RadarLedgerArtifactLinkRequest {
			db_path: self.db_path.clone().unwrap_or_else(radar::default_ledger_path),
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
		let summary = radar::ledger_summary(&RadarLedgerSummaryRequest {
			db_path: self.db_path.clone().unwrap_or_else(radar::default_ledger_path),
		})?;

		println!("{summary:#?}");

		Ok(())
	}
}

#[derive(Debug, Subcommand)]
enum RadarSubcommand {
	/// Validate Radar JSON artifacts.
	Validate(RadarValidateCommand),
	/// Refresh the upstream Codex review queue artifact.
	RefreshUpstreamQueue(RadarRefreshUpstreamQueueCommand),
	/// Refresh the release-delta artifact.
	RefreshReleaseDelta(RadarRefreshReleaseDeltaCommand),
	/// Build or validate GitHub change bundles.
	Bundle(RadarBundleCommand),
	/// Manage social publication handoff state.
	Social(RadarSocialCommand),
	/// Render a signal entry from a bundle and analysis draft.
	RenderSignal(RadarRenderSignalCommand),
	/// Backfill unpublished signal entries from a release comparison.
	BackfillReleaseRange(RadarBackfillReleaseRangeCommand),
	/// Manage the local Radar ledger.
	Ledger(RadarLedgerCommand),
}

#[derive(Debug, Subcommand)]
enum RadarBundleSubcommand {
	/// Build a deterministic GitHub change bundle.
	Build(RadarBundleBuildCommand),
	/// Validate GitHub change bundle artifacts.
	Validate(RadarBundleValidateCommand),
}

#[derive(Debug, Subcommand)]
enum RadarSocialSubcommand {
	/// Atomically reserve one social publish slot before browser compose.
	ReservePublish(RadarSocialReservePublishCommand),
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
