use serde::Serialize;
use serde_json::Error;

use crate::tracker::records::{LinearExecutionEventIdentity, LinearExecutionEventRecord};

pub(crate) fn render_linear_execution_event_comment_body(
	record: &LinearExecutionEventRecord,
	retry_budget_attempt_count: Option<i64>,
) -> String {
	let mut body = format!(
		"Decodex execution event: {}\n\n- run_sequence_attempt: `{}` (not retry-budget count)",
		record.event_type, record.attempt_number
	);

	if let Some(retry_budget_attempt_count) = retry_budget_attempt_count {
		body.push_str(&format!(
			"\n- retry_budget_attempts_consumed: `{retry_budget_attempt_count}`"
		));
	}

	body
}

/// Render the low-frequency public Linear projection for a private progress checkpoint.
pub(crate) fn render_progress_checkpoint_public_projection(
	identity: LinearExecutionEventIdentity<'_>,
	event_timestamp: String,
	phase: &str,
	branch: Option<&str>,
	worktree_path: Option<&str>,
	pr_url: Option<&str>,
) -> LinearExecutionEventRecord {
	let anchor = stable_event_anchor(&[
		phase,
		branch.unwrap_or_default(),
		worktree_path.unwrap_or_default(),
		pr_url.unwrap_or_default(),
	]);
	let mut record =
		LinearExecutionEventRecord::new(identity, "progress_checkpoint", event_timestamp, &anchor);

	record.phase = Some(phase.to_owned());
	record.branch = branch.map(ToOwned::to_owned);
	record.worktree_path = worktree_path.map(ToOwned::to_owned);
	record.pr_url = pr_url.map(ToOwned::to_owned);
	record.summary = Some(format!("Execution phase: {phase}."));

	record
}

pub(crate) fn append_structured_comment_record<T>(
	body: &str,
	record: &T,
) -> std::result::Result<String, Error>
where
	T: Serialize,
{
	let payload = format_structured_comment(record)?;

	if body.trim().is_empty() {
		return Ok(payload);
	}

	Ok(format!("{body}\n\n{payload}"))
}

pub(crate) fn stable_event_anchor(parts: &[&str]) -> String {
	let mut hash = 0xcbf29ce484222325_u64;

	for part in parts {
		for byte in part.as_bytes() {
			hash ^= u64::from(*byte);
			hash = hash.wrapping_mul(0x100000001b3);
		}

		hash ^= 0xff;
		hash = hash.wrapping_mul(0x100000001b3);
	}

	format!("{hash:016x}")
}

fn format_structured_comment<T>(record: &T) -> std::result::Result<String, Error>
where
	T: Serialize,
{
	let payload = serde_json::to_string_pretty(record)?;

	Ok(format!("```json\n{payload}\n```"))
}
