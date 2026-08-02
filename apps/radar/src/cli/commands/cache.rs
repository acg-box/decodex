use clap::Args;

use crate::{RadarCacheGcRequest, RadarContentV2ResetRequest, prelude::Result};

#[derive(Debug, Args)]
pub(in crate::cli) struct RadarCacheGcCommand {}
impl RadarCacheGcCommand {
	pub(super) fn run(&self) -> Result<()> {
		let report = crate::cache_gc(&RadarCacheGcRequest::default())?;

		println!("{}", serde_json::to_string_pretty(&report)?);

		Ok(())
	}
}

#[derive(Debug, Args)]
pub(in crate::cli) struct RadarContentV2ResetCommand {}
impl RadarContentV2ResetCommand {
	pub(super) fn run(&self) -> Result<()> {
		let report = crate::reset_content_v2(&content_v2_reset_request()?)?;
		println!("{}", serde_json::to_string_pretty(&report)?);

		Ok(())
	}
}

fn content_v2_reset_request() -> Result<RadarContentV2ResetRequest> {
	content_v2_reset_request_from(&std::env::current_dir()?)
}

fn content_v2_reset_request_from(start: &std::path::Path) -> Result<RadarContentV2ResetRequest> {
	Ok(RadarContentV2ResetRequest {
		cache_root: crate::repo_root_from(start)?.join(crate::DEFAULT_CACHE_ROOT),
	})
}

#[cfg(test)]
mod tests {
	#[test]
	fn content_v2_reset_root_resolves_from_a_repo_subdirectory_and_rejects_outside() {
		let root = crate::repo_root().expect("repository root");
		let request = super::content_v2_reset_request_from(&root.join("apps/radar/src"))
			.expect("subdirectory reset request");
		assert_eq!(request.cache_root, root.join(crate::DEFAULT_CACHE_ROOT));

		let outside = crate::test_support::private_tempdir();
		let error = super::content_v2_reset_request_from(outside.path())
			.expect_err("outside-repository reset request must fail");
		assert!(error.to_string().contains("Unable to find Decodex repository root"));
	}
}
