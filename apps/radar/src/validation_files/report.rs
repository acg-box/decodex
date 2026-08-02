use sha2::{Digest as _, Sha256};

use crate::{Map, RadarRefreshQueueReport, RefreshWriteReport, Value, prelude::Result};

pub(crate) fn queue_report(
	queue: &Value,
	refresh: RefreshWriteReport,
	ledger_enabled: bool,
) -> Result<RadarRefreshQueueReport> {
	let counts = queue.get("counts").and_then(Value::as_object);
	let mut queue_bytes = serde_json::to_vec_pretty(queue)?;

	queue_bytes.push(b'\n');

	Ok(RadarRefreshQueueReport {
		material_changed: refresh.material_changed,
		written: refresh.written,
		refreshed_at: refresh.refreshed_at,
		queue_sha256: Sha256::digest(&queue_bytes)
			.iter()
			.map(|byte| format!("{byte:02x}"))
			.collect(),
		recent_commits_scanned: count_field(counts, "recent_commits_scanned"),
		published_subjects_seen: count_field(counts, "published_subjects_seen"),
		subjects_queued: count_field(counts, "subjects_queued"),
		ledger_enabled,
	})
}

fn count_field(counts: Option<&Map<String, Value>>, field: &str) -> usize {
	counts
		.and_then(|counts| counts.get(field))
		.and_then(Value::as_u64)
		.and_then(|value| usize::try_from(value).ok())
		.unwrap_or_default()
}
