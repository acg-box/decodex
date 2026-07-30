use std::path::PathBuf;

use clap::{Args, Subcommand};

use crate::{
	DEFAULT_SOCIAL_ATTEMPTS_DIR, DEFAULT_SOCIAL_CANDIDATES_DIR, DEFAULT_SOCIAL_LOCKS_DIR,
	DEFAULT_SOCIAL_OUTCOMES_DIR, DEFAULT_SOCIAL_POSTS_DIR, DEFAULT_SOCIAL_RESERVATIONS_DIR,
	DEFAULT_SOCIAL_STAGING_DIR, DEFAULT_SOCIAL_STRATEGIES_DIR, DEFAULT_XURL_AUTH_CONTRACT_PATH,
	SOCIAL_DAILY_LIMIT, SOCIAL_MONTHLY_BUDGET_MICROUSD, SOCIAL_TIMEZONE, SocialClock,
	SocialGcRequest, SocialObserveXurlRequest, SocialPublishXurlRequest,
	SocialReconcileXurlRequest, SocialRecordManagerRequest, SocialReservePublishRequest,
	SocialSealXurlAuthRequest, SocialTerminalizeSkipRequest, prelude::Result,
};

#[derive(Debug, Args)]
pub(super) struct SocialCommand {
	#[command(subcommand)]
	command: SocialSubcommand,
}
impl SocialCommand {
	pub(super) fn run(&self) -> Result<()> {
		match &self.command {
			SocialSubcommand::CostReport(args) => args.run(),
			SocialSubcommand::Gc(args) => args.run(),
			SocialSubcommand::ObserveXurl(args) => args.run(),
			SocialSubcommand::ProbeXurl(args) => args.run(),
			SocialSubcommand::PublishXurl(args) => args.run(),
			SocialSubcommand::ReconcileXurl(args) => args.run(),
			SocialSubcommand::RecordManager(args) => args.run(),
			SocialSubcommand::ReservePublish(args) => args.run(),
			SocialSubcommand::SealXurlAuth(args) => args.run(),
			SocialSubcommand::TerminalizeSkip(args) => args.run(),
		}
	}
}

#[derive(Debug, Args)]
struct SocialRecordManagerCommand {
	#[arg(long = "staging", value_name = "FILE")]
	staging_path: PathBuf,
	#[arg(long)]
	run_id: String,
}
impl SocialRecordManagerCommand {
	fn run(&self) -> Result<()> {
		require_current_thread_id(&self.run_id)?;
		let report = crate::record_social_manager(&SocialRecordManagerRequest {
			staging_path: self.staging_path.clone(),
			staging_dir: PathBuf::from(DEFAULT_SOCIAL_STAGING_DIR),
			candidates_dir: PathBuf::from(DEFAULT_SOCIAL_CANDIDATES_DIR),
			strategies_dir: PathBuf::from(DEFAULT_SOCIAL_STRATEGIES_DIR),
			reservations_dir: PathBuf::from(DEFAULT_SOCIAL_RESERVATIONS_DIR),
			posts_dir: PathBuf::from(DEFAULT_SOCIAL_POSTS_DIR),
			outcomes_dir: PathBuf::from(DEFAULT_SOCIAL_OUTCOMES_DIR),
			locks_dir: PathBuf::from(DEFAULT_SOCIAL_LOCKS_DIR),
			run_id: self.run_id.clone(),
		})?;
		println!("{}", serde_json::to_string_pretty(&report)?);

		Ok(())
	}
}

#[derive(Debug, Args)]
struct SocialProbeXurlCommand {}
impl SocialProbeXurlCommand {
	fn run(&self) -> Result<()> {
		let clock = SocialClock::current()?;
		let report = crate::probe_social_xurl(&clock.now)?;
		println!("{}", serde_json::to_string_pretty(&report)?);

		Ok(())
	}
}

#[derive(Debug, Args)]
struct SocialReconcileXurlCommand {
	#[arg(
		long = "evidence",
		value_name = "FILE",
		conflicts_with = "attempt_path",
		required_unless_present = "attempt_path"
	)]
	evidence_path: Option<PathBuf>,
	#[arg(
		long = "attempt",
		value_name = "FILE",
		conflicts_with = "evidence_path",
		required_unless_present = "evidence_path"
	)]
	attempt_path: Option<PathBuf>,
	#[arg(long)]
	operation_id: String,
}
impl SocialReconcileXurlCommand {
	fn run(&self) -> Result<()> {
		let clock = SocialClock::current()?;
		require_current_thread_id(&self.operation_id)?;
		let report = crate::reconcile_social_xurl(&SocialReconcileXurlRequest {
			evidence_path: self.evidence_path.clone().unwrap_or_default(),
			attempt_path: self.attempt_path.clone(),
			authorization_contract_path: PathBuf::from(DEFAULT_XURL_AUTH_CONTRACT_PATH),
			reservations_dir: PathBuf::from(DEFAULT_SOCIAL_RESERVATIONS_DIR),
			candidates_dir: PathBuf::from(DEFAULT_SOCIAL_CANDIDATES_DIR),
			posts_dir: PathBuf::from(DEFAULT_SOCIAL_POSTS_DIR),
			outcomes_dir: PathBuf::from(DEFAULT_SOCIAL_OUTCOMES_DIR),
			attempts_dir: PathBuf::from(DEFAULT_SOCIAL_ATTEMPTS_DIR),
			locks_dir: PathBuf::from(DEFAULT_SOCIAL_LOCKS_DIR),
			operation_id: self.operation_id.clone(),
			reconciled_at: clock.now,
		})?;
		println!("{}", serde_json::to_string_pretty(&report)?);

		Ok(())
	}
}

#[derive(Debug, Args)]
struct SocialSealXurlAuthCommand {}
impl SocialSealXurlAuthCommand {
	fn run(&self) -> Result<()> {
		let clock = SocialClock::current()?;
		let report = crate::seal_social_xurl_auth(&SocialSealXurlAuthRequest {
			receipt_path: PathBuf::from(DEFAULT_XURL_AUTH_CONTRACT_PATH),
			sealed_at: clock.now,
		})?;
		println!("{}", serde_json::to_string_pretty(&report)?);

		Ok(())
	}
}

#[derive(Debug, Args)]
struct SocialCostReportCommand {
	#[arg(long, value_name = "YYYY-MM")]
	month: Option<String>,
}
impl SocialCostReportCommand {
	fn run(&self) -> Result<()> {
		let clock = SocialClock::current()?;
		let month = self.month.as_deref().unwrap_or(&clock.day[..7]);
		let report = crate::report_social_xurl_cost(month)?;
		println!("{}", serde_json::to_string_pretty(&report)?);

		Ok(())
	}
}

#[derive(Debug, Args)]
struct SocialGcCommand {}
impl SocialGcCommand {
	fn run(&self) -> Result<()> {
		let clock = SocialClock::current()?;
		let report = crate::gc_social(&SocialGcRequest {
			candidates_dir: PathBuf::from(DEFAULT_SOCIAL_CANDIDATES_DIR),
			reservations_dir: PathBuf::from(DEFAULT_SOCIAL_RESERVATIONS_DIR),
			posts_dir: PathBuf::from(DEFAULT_SOCIAL_POSTS_DIR),
			outcomes_dir: PathBuf::from(DEFAULT_SOCIAL_OUTCOMES_DIR),
			attempts_dir: PathBuf::from(DEFAULT_SOCIAL_ATTEMPTS_DIR),
			strategies_dir: PathBuf::from(DEFAULT_SOCIAL_STRATEGIES_DIR),
			locks_dir: PathBuf::from(DEFAULT_SOCIAL_LOCKS_DIR),
			now: clock.now,
		})?;
		println!("{}", serde_json::to_string_pretty(&report)?);

		Ok(())
	}
}

#[derive(Debug, Args)]
struct SocialObserveXurlCommand {
	#[arg(long = "post", value_name = "FILE")]
	post_path: PathBuf,
	#[arg(long)]
	run_id: String,
	#[arg(long, value_parser = ["24h", "7d"])]
	window: String,
}
impl SocialObserveXurlCommand {
	fn run(&self) -> Result<()> {
		let clock = SocialClock::current()?;
		require_current_thread_id(&self.run_id)?;
		let report = crate::observe_social_xurl(&SocialObserveXurlRequest {
			run_id: self.run_id.clone(),
			post_path: self.post_path.clone(),
			authorization_contract_path: PathBuf::from(DEFAULT_XURL_AUTH_CONTRACT_PATH),
			posts_dir: PathBuf::from(DEFAULT_SOCIAL_POSTS_DIR),
			outcomes_dir: PathBuf::from(DEFAULT_SOCIAL_OUTCOMES_DIR),
			attempts_dir: PathBuf::from(DEFAULT_SOCIAL_ATTEMPTS_DIR),
			locks_dir: PathBuf::from(DEFAULT_SOCIAL_LOCKS_DIR),
			observed_at: clock.now,
			window: self.window.clone(),
			monthly_budget_microusd: SOCIAL_MONTHLY_BUDGET_MICROUSD,
		})?;
		println!("{}", serde_json::to_string_pretty(&report)?);

		Ok(())
	}
}

#[derive(Debug, Args)]
struct SocialTerminalizeSkipCommand {
	#[arg(long = "candidate", value_name = "FILE")]
	candidate_path: PathBuf,
	#[arg(long)]
	run_id: String,
	#[arg(long)]
	dry_run: bool,
}
impl SocialTerminalizeSkipCommand {
	fn run(&self) -> Result<()> {
		let clock = SocialClock::current()?;
		require_current_thread_id(&self.run_id)?;
		let report = crate::terminalize_social_skip(&SocialTerminalizeSkipRequest {
			candidate_path: self.candidate_path.clone(),
			candidates_dir: PathBuf::from(DEFAULT_SOCIAL_CANDIDATES_DIR),
			reservations_dir: PathBuf::from(DEFAULT_SOCIAL_RESERVATIONS_DIR),
			posts_dir: PathBuf::from(DEFAULT_SOCIAL_POSTS_DIR),
			locks_dir: PathBuf::from(DEFAULT_SOCIAL_LOCKS_DIR),
			run_id: self.run_id.clone(),
			day: clock.day,
			timezone: SOCIAL_TIMEZONE.into(),
			daily_limit: SOCIAL_DAILY_LIMIT,
			dry_run: self.dry_run,
		})?;

		println!("{}", serde_json::to_string_pretty(&report)?);

		Ok(())
	}
}

#[derive(Debug, Args)]
struct SocialReservePublishCommand {
	#[arg(long = "candidate", value_name = "FILE")]
	candidate_path: PathBuf,
	#[arg(long)]
	run_id: String,
	#[arg(long)]
	dry_run: bool,
}
impl SocialReservePublishCommand {
	fn run(&self) -> Result<()> {
		let clock = SocialClock::current()?;
		require_current_thread_id(&self.run_id)?;
		let report = crate::reserve_social_publish(&SocialReservePublishRequest {
			candidate_path: self.candidate_path.clone(),
			candidates_dir: PathBuf::from(DEFAULT_SOCIAL_CANDIDATES_DIR),
			reserved_at: clock.now,
			expires_at: clock.expires_at,
			day: clock.day,
			timezone: SOCIAL_TIMEZONE.into(),
			out_dir: PathBuf::from(DEFAULT_SOCIAL_RESERVATIONS_DIR),
			posts_dir: PathBuf::from(DEFAULT_SOCIAL_POSTS_DIR),
			attempts_dir: PathBuf::from(DEFAULT_SOCIAL_ATTEMPTS_DIR),
			locks_dir: PathBuf::from(DEFAULT_SOCIAL_LOCKS_DIR),
			run_id: self.run_id.clone(),
			daily_limit: SOCIAL_DAILY_LIMIT,
			dry_run: self.dry_run,
		})?;

		println!("{}", serde_json::to_string_pretty(&report)?);

		Ok(())
	}
}

#[derive(Debug, Args)]
struct SocialPublishXurlCommand {
	#[arg(long = "reservation", value_name = "FILE")]
	reservation_path: PathBuf,
	#[arg(long)]
	run_id: String,
}
impl SocialPublishXurlCommand {
	fn run(&self) -> Result<()> {
		let clock = SocialClock::current()?;
		require_current_thread_id(&self.run_id)?;
		let report = crate::publish_social_xurl(&SocialPublishXurlRequest {
			reservation_path: self.reservation_path.clone(),
			authorization_contract_path: PathBuf::from(DEFAULT_XURL_AUTH_CONTRACT_PATH),
			reservations_dir: PathBuf::from(DEFAULT_SOCIAL_RESERVATIONS_DIR),
			candidates_dir: PathBuf::from(DEFAULT_SOCIAL_CANDIDATES_DIR),
			posts_dir: PathBuf::from(DEFAULT_SOCIAL_POSTS_DIR),
			attempts_dir: PathBuf::from(DEFAULT_SOCIAL_ATTEMPTS_DIR),
			locks_dir: PathBuf::from(DEFAULT_SOCIAL_LOCKS_DIR),
			run_id: self.run_id.clone(),
			posted_at: clock.now,
			monthly_budget_microusd: SOCIAL_MONTHLY_BUDGET_MICROUSD,
		})?;

		println!("{}", serde_json::to_string_pretty(&report)?);

		Ok(())
	}
}

#[derive(Debug, Subcommand)]
enum SocialSubcommand {
	/// Report bounded X cost ceilings and call counts for one billing month.
	CostReport(SocialCostReportCommand),
	/// Prune expired, complete social lineages and strategies under the state lock.
	Gc(SocialGcCommand),
	/// Read one due 24-hour or 7-day outcome through xurl.
	ObserveXurl(Box<SocialObserveXurlCommand>),
	/// Verify xurl runtime, OAuth label, and pricing policy without a paid endpoint.
	ProbeXurl(SocialProbeXurlCommand),
	/// Publish one reserved candidate through the official xurl CLI and verify it.
	PublishXurl(Box<SocialPublishXurlCommand>),
	/// Finalize durable evidence or make one bounded safe X recovery read.
	ReconcileXurl(Box<SocialReconcileXurlCommand>),
	/// Validate and atomically record one Content Manager candidate or strategy.
	RecordManager(SocialRecordManagerCommand),
	/// Atomically reserve one social publish slot before the X API write.
	ReservePublish(Box<SocialReservePublishCommand>),
	/// Seal the fixed nonsecret xurl authorization contract.
	SealXurlAuth(SocialSealXurlAuthCommand),
	/// Atomically terminalize one quality-skip candidate without calling X.
	TerminalizeSkip(SocialTerminalizeSkipCommand),
}

fn require_current_thread_id(run_id: &str) -> Result<()> {
	let current = std::env::var("CODEX_THREAD_ID")
		.map_err(|_| crate::prelude::eyre::eyre!("CODEX_THREAD_ID is required"))?;
	if run_id != current {
		return Err(crate::prelude::eyre::eyre!("run_id must exactly match CODEX_THREAD_ID"));
	}

	Ok(())
}
