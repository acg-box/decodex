use crate::{Map, Path, RadarRefreshQueueReport, Value};

pub(crate) fn queue_report(
	queue: &Value,
	changed: bool,
	ledger_enabled: bool,
	root: &Path,
	queue_out: &Path,
) -> RadarRefreshQueueReport {
	let counts = queue.get("counts").and_then(Value::as_object);

	RadarRefreshQueueReport {
		changed,
		recent_commits_scanned: count_field(counts, "recent_commits_scanned"),
		published_subjects_seen: count_field(counts, "published_subjects_seen"),
		subjects_queued: count_field(counts, "subjects_queued"),
		ledger_enabled,
		queue_out: crate::absolute_repo_path(root, queue_out),
	}
}

fn count_field(counts: Option<&Map<String, Value>>, field: &str) -> usize {
	counts
		.and_then(|counts| counts.get(field))
		.and_then(Value::as_u64)
		.and_then(|value| usize::try_from(value).ok())
		.unwrap_or_default()
}
