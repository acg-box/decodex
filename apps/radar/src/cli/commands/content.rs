use std::path::PathBuf;

use clap::Args;

use crate::{
	DEFAULT_SOURCE_MAX_AGE_HOURS, RadarContentEligibilityRequest, RadarContentPairCommitRequest,
	RadarReviewNextRequest, prelude::Result,
};

#[derive(Debug, Args)]
pub(in crate::cli) struct RadarContentPairCommitCommand {
	#[arg(
		long,
		value_name = "DIRECTORY",
		default_value = crate::paths::DEFAULT_CACHE_ROOT
	)]
	cache_root: PathBuf,
	/// Create-only mode-0600 JSON. Its impact review digest must be 64 zeroes.
	#[arg(long, value_name = "FILE")]
	staging: PathBuf,
	#[arg(long, value_name = "HOURS", default_value_t = DEFAULT_SOURCE_MAX_AGE_HOURS)]
	max_age_hours: u64,
}

#[derive(Debug, Args)]
pub(in crate::cli) struct RadarContentEligibilityCommand {
	#[arg(long, value_name = "FILE", default_value = crate::paths::DEFAULT_QUEUE_OUT)]
	queue: PathBuf,
	#[arg(long, value_name = "FILE")]
	review: PathBuf,
	#[arg(long, value_name = "FILE")]
	impact: PathBuf,
	#[arg(long, value_name = "HOURS", default_value_t = DEFAULT_SOURCE_MAX_AGE_HOURS)]
	max_age_hours: u64,
}

#[derive(Debug, Args)]
pub(in crate::cli) struct RadarReviewNextCommand {
	#[arg(
		long,
		value_name = "DIRECTORY",
		default_value = crate::paths::DEFAULT_CACHE_ROOT
	)]
	cache_root: PathBuf,
	#[arg(long, value_name = "HOURS", default_value_t = DEFAULT_SOURCE_MAX_AGE_HOURS)]
	max_age_hours: u64,
}
impl RadarReviewNextCommand {
	pub(in crate::cli::commands) fn run(&self) -> Result<()> {
		let report = crate::review_next(&RadarReviewNextRequest {
			cache_root: self.cache_root.clone(),
			max_age_hours: self.max_age_hours,
		})?;

		println!("{}", serde_json::to_string_pretty(&report)?);

		Ok(())
	}
}
impl RadarContentPairCommitCommand {
	pub(in crate::cli::commands) fn run(&self) -> Result<()> {
		let report = crate::commit_content_pair(&RadarContentPairCommitRequest {
			cache_root: self.cache_root.clone(),
			staging: self.staging.clone(),
			max_age_hours: self.max_age_hours,
		})?;

		println!("{}", serde_json::to_string_pretty(&report)?);

		Ok(())
	}
}
impl RadarContentEligibilityCommand {
	pub(in crate::cli::commands) fn run(&self) -> Result<()> {
		let report = crate::content_eligibility(&RadarContentEligibilityRequest {
			queue: self.queue.clone(),
			review: self.review.clone(),
			impact: self.impact.clone(),
			max_age_hours: self.max_age_hours,
		})?;

		println!("{}", serde_json::to_string_pretty(&report)?);

		Ok(())
	}
}
