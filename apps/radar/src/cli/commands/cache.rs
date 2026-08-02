use clap::Args;

use crate::{RadarCacheGcRequest, prelude::Result};

#[derive(Debug, Args)]
pub(in crate::cli) struct RadarCacheGcCommand {}
impl RadarCacheGcCommand {
	pub(super) fn run(&self) -> Result<()> {
		let report = crate::cache_gc(&RadarCacheGcRequest::default())?;

		println!("{}", serde_json::to_string_pretty(&report)?);

		Ok(())
	}
}
