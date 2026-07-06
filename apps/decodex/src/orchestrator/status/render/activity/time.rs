pub(crate) fn format_seconds_compact(seconds: i64) -> String {
	if seconds >= 3_600 {
		return format!("{}h{}m", seconds / 3_600, (seconds % 3_600) / 60);
	}
	if seconds >= 60 {
		return format!("{}m{}s", seconds / 60, seconds % 60);
	}

	format!("{seconds}s")
}
