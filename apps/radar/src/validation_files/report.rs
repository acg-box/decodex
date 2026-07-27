use crate::{Map, RadarRefreshQueueReport, RefreshWriteReport, Value};

pub(crate) fn queue_report(
	queue: &Value,
	refresh: RefreshWriteReport,
	ledger_enabled: bool,
) -> RadarRefreshQueueReport {
	let counts = queue.get("counts").and_then(Value::as_object);

	RadarRefreshQueueReport {
		material_changed: refresh.material_changed,
		written: refresh.written,
		refreshed_at: refresh.refreshed_at,
		recent_commits_scanned: count_field(counts, "recent_commits_scanned"),
		published_subjects_seen: count_field(counts, "published_subjects_seen"),
		subjects_queued: count_field(counts, "subjects_queued"),
		ledger_enabled,
	}
}

fn count_field(counts: Option<&Map<String, Value>>, field: &str) -> usize {
	counts
		.and_then(|counts| counts.get(field))
		.and_then(Value::as_u64)
		.and_then(|value| usize::try_from(value).ok())
		.unwrap_or_default()
}
