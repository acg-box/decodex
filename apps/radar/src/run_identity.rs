//! Authoritative identity for one scheduled Codex automation run.

use crate::prelude::{Result, eyre};

const CODEX_THREAD_ID: &str = "CODEX_THREAD_ID";

pub(crate) fn current_run_id() -> Result<String> {
	let run_id = std::env::var(CODEX_THREAD_ID)
		.map_err(|_| eyre::eyre!("CODEX_THREAD_ID must be set to a lowercase UUID"))?;

	validate_run_id(&run_id)?;
	Ok(run_id)
}

pub(crate) fn validate_run_id(run_id: &str) -> Result<()> {
	let lengths = [8, 4, 4, 4, 12];
	let mut segments = run_id.split('-');

	if lengths.into_iter().any(|length| {
		segments.next().is_none_or(|segment| {
			segment.len() != length
				|| !segment.bytes().all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
		})
	}) || segments.next().is_some()
	{
		eyre::bail!("CODEX_THREAD_ID must be a lowercase UUID");
	}

	Ok(())
}
