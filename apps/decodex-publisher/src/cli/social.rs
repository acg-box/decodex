use std::path::PathBuf;

use clap::{Args, Subcommand, ValueEnum};

use crate::{
	DEFAULT_SOCIAL_ATTEMPTS_DIR, DEFAULT_SOCIAL_CANDIDATES_DIR, DEFAULT_SOCIAL_LOCKS_DIR,
	DEFAULT_SOCIAL_POSTS_DIR, DEFAULT_SOCIAL_STAGING_DIR, DEFAULT_XURL_AUTH_CONTRACT_PATH,
	SocialClock, SocialObserveDueRequest, SocialPublishNextRequest, SocialRecordCandidateRequest,
	SocialSealXurlAuthRequest, prelude::Result,
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
			SocialSubcommand::ObserveDue(args) => args.run(),
			SocialSubcommand::ProbeXurl(args) => args.run(),
			SocialSubcommand::PublishNext(args) => args.run(),
			SocialSubcommand::RecordCandidate(args) => args.run(),
			SocialSubcommand::RefreshPricing(args) => args.run(),
			SocialSubcommand::SealXurlAuth(args) => args.run(),
		}
	}
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum PublishDecision {
	Publish,
	Skip,
}

#[derive(Debug, Args)]
struct SocialRecordCandidateCommand {
	#[arg(long = "staging", value_name = "FILE")]
	staging_path: PathBuf,
	#[arg(long)]
	run_id: String,
}

impl SocialRecordCandidateCommand {
	fn run(&self) -> Result<()> {
		require_current_thread_id(&self.run_id)?;
		let report = crate::record_social_candidate(&SocialRecordCandidateRequest {
			staging_path: self.staging_path.clone(),
			staging_dir: PathBuf::from(DEFAULT_SOCIAL_STAGING_DIR),
			candidates_dir: PathBuf::from(DEFAULT_SOCIAL_CANDIDATES_DIR),
			posts_dir: PathBuf::from(DEFAULT_SOCIAL_POSTS_DIR),
			attempts_dir: PathBuf::from(DEFAULT_SOCIAL_ATTEMPTS_DIR),
			locks_dir: PathBuf::from(DEFAULT_SOCIAL_LOCKS_DIR),
			run_id: self.run_id.clone(),
		})?;
		println!("{}", serde_json::to_string_pretty(&report)?);
		Ok(())
	}
}

#[derive(Debug, Args)]
struct SocialPublishNextCommand {
	#[arg(long)]
	run_id: String,
	#[arg(long, value_enum)]
	decision: PublishDecision,
	#[arg(long)]
	reason: Option<String>,
}

impl SocialPublishNextCommand {
	fn run(&self) -> Result<()> {
		require_current_thread_id(&self.run_id)?;
		let decision = match self.decision {
			PublishDecision::Publish => "publish",
			PublishDecision::Skip => "skip",
		};
		let report = crate::publish_next(&SocialPublishNextRequest {
			run_id: self.run_id.clone(),
			decision: decision.into(),
			reason: self.reason.clone(),
			clock: SocialClock::current()?,
		})?;
		println!("{}", serde_json::to_string_pretty(&report)?);
		Ok(())
	}
}

#[derive(Debug, Args)]
struct SocialObserveDueCommand {
	#[arg(long)]
	run_id: String,
}

impl SocialObserveDueCommand {
	fn run(&self) -> Result<()> {
		require_current_thread_id(&self.run_id)?;
		let report = crate::observe_due(&SocialObserveDueRequest {
			run_id: self.run_id.clone(),
			observed_at: SocialClock::current()?.now,
		})?;
		println!("{}", serde_json::to_string_pretty(&report)?);
		Ok(())
	}
}

#[derive(Debug, Args)]
struct SocialProbeXurlCommand {}

impl SocialProbeXurlCommand {
	fn run(&self) -> Result<()> {
		let report = crate::probe_social_xurl(&SocialClock::current()?.now)?;
		println!("{}", serde_json::to_string_pretty(&report)?);
		Ok(())
	}
}

#[derive(Debug, Args)]
struct SocialRefreshPricingCommand {}

impl SocialRefreshPricingCommand {
	fn run(&self) -> Result<()> {
		let report = crate::refresh_social_x_pricing(&SocialClock::current()?.now)?;
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
struct SocialSealXurlAuthCommand {}

impl SocialSealXurlAuthCommand {
	fn run(&self) -> Result<()> {
		let report = crate::seal_social_xurl_auth(&SocialSealXurlAuthRequest {
			receipt_path: PathBuf::from(DEFAULT_XURL_AUTH_CONTRACT_PATH),
			sealed_at: SocialClock::current()?.now,
		})?;
		println!("{}", serde_json::to_string_pretty(&report)?);
		Ok(())
	}
}

#[derive(Debug, Subcommand)]
enum SocialSubcommand {
	/// Report bounded X cost ceilings and call counts for one billing month.
	CostReport(SocialCostReportCommand),
	/// Observe the next due 24-hour or 7-day X outcome.
	ObserveDue(SocialObserveDueCommand),
	/// Verify xurl, the fixed account, authorization, and current pricing without a paid call.
	ProbeXurl(SocialProbeXurlCommand),
	/// Publish or skip the one pending candidate through the complete safe workflow.
	PublishNext(SocialPublishNextCommand),
	/// Validate and atomically record one source-backed candidate or no-op.
	RecordCandidate(SocialRecordCandidateCommand),
	/// Refresh official X pricing with one free, bounded documentation request.
	RefreshPricing(SocialRefreshPricingCommand),
	/// Seal the fixed nonsecret xurl authorization contract after interactive login.
	SealXurlAuth(SocialSealXurlAuthCommand),
}

fn require_current_thread_id(run_id: &str) -> Result<()> {
	let current = std::env::var("CODEX_THREAD_ID")
		.map_err(|_| crate::prelude::eyre::eyre!("CODEX_THREAD_ID is required"))?;
	if run_id != current {
		return Err(crate::prelude::eyre::eyre!("run_id must exactly match CODEX_THREAD_ID"));
	}
	Ok(())
}
