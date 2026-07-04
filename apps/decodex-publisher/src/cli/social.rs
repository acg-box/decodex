use std::path::PathBuf;

use clap::{Args, Subcommand};

use crate::{
	DEFAULT_SOCIAL_POSTS_DIR, DEFAULT_SOCIAL_RESERVATIONS_DIR, SocialReservePublishRequest,
	prelude::Result,
};

#[derive(Debug, Args)]
pub(super) struct SocialCommand {
	#[command(subcommand)]
	command: SocialSubcommand,
}
impl SocialCommand {
	pub(super) fn run(&self) -> Result<()> {
		match &self.command {
			SocialSubcommand::ReservePublish(args) => args.run(),
		}
	}
}

#[derive(Debug, Args)]
struct SocialReservePublishCommand {
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
	#[arg(long, value_name = "DIR", default_value = DEFAULT_SOCIAL_RESERVATIONS_DIR)]
	out_dir: PathBuf,
	#[arg(long, value_name = "DIR", default_value = DEFAULT_SOCIAL_POSTS_DIR)]
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
impl SocialReservePublishCommand {
	fn run(&self) -> Result<()> {
		let report = crate::reserve_social_publish(&SocialReservePublishRequest {
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

#[derive(Debug, Subcommand)]
enum SocialSubcommand {
	/// Atomically reserve one social publish slot before browser compose.
	ReservePublish(Box<SocialReservePublishCommand>),
}
