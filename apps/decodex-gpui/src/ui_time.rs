//! Shared relative ages, elapsed durations and local timestamps for desktop surfaces.

pub(crate) fn duration(ms: u64) -> String {
	if ms >= 86_400_000 {
		format!("{}d {}h", ms / 86_400_000, ms % 86_400_000 / 3_600_000)
	} else if ms >= 3_600_000 {
		format!("{}h {}m", ms / 3_600_000, ms % 3_600_000 / 60_000)
	} else if ms >= 60_000 {
		format!("{}m {}s", ms / 60_000, ms % 60_000 / 1_000)
	} else {
		let seconds = format!("{:.1}", ms as f64 / 1_000.);
		format!("{}s", seconds.trim_end_matches(".0"))
	}
}

pub(crate) fn optional_duration(ms: Option<u64>) -> String {
	ms.map(duration).unwrap_or_else(|| "—".into())
}

pub(crate) fn relative_time(micros: i64, now: i64) -> String {
	if micros <= 0 {
		return "—".into();
	}
	let seconds = now.saturating_sub(micros).max(0) / 1_000_000;
	for (unit, suffix) in [
		(31_536_000, "y"),
		(2_592_000, "m"),
		(604_800, "w"),
		(86_400, "d"),
		(3_600, "h"),
		(60, "min"),
	] {
		if seconds >= unit {
			return format!("{}{suffix}", seconds / unit);
		}
	}
	"now".into()
}

pub(crate) fn now() -> i64 {
	std::time::SystemTime::now()
		.duration_since(std::time::UNIX_EPOCH)
		.map_or(0, |d| d.as_micros().min(i64::MAX as u128) as i64)
}

pub(crate) fn local_parts(micros: i64) -> (String, String) {
	let seconds = (micros / 1_000_000) as libc::time_t;
	let mut local = std::mem::MaybeUninit::<libc::tm>::zeroed();
	// SAFETY: localtime_r writes a tm into valid storage for the supplied timestamp.
	let local = unsafe {
		if libc::localtime_r(&seconds, local.as_mut_ptr()).is_null() {
			return ("—".into(), "—".into());
		}
		local.assume_init()
	};
	let month =
		["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"]
			[local.tm_mon as usize];
	(
		format!("{month} {}, {}", local.tm_mday, local.tm_year + 1900),
		format!("{:02}:{:02}", local.tm_hour, local.tm_min),
	)
}

pub(crate) fn timestamp(micros: i64) -> String {
	if micros <= 0 {
		return "—".into();
	}
	let (day, clock) = local_parts(micros);
	format!("{day} {clock}")
}

#[cfg(test)]
mod tests {
	#[test]
	fn duration_units_stay_compact_and_keep_missing_distinct_from_zero() {
		for (ms, expected) in [
			(0, "0s"),
			(2_000, "2s"),
			(31_300, "31.3s"),
			(86_399_000, "23h 59m"),
			(86_400_000, "1d 0h"),
			(123 * 3_600_000 + 45 * 60_000, "5d 3h"),
			(99 * 86_400_000 + 23 * 3_600_000, "99d 23h"),
		] {
			assert_eq!(super::duration(ms), expected);
			assert_eq!(super::optional_duration(Some(ms)), expected);
		}
		assert_eq!(super::optional_duration(None), "—");
	}

	#[test]
	fn relative_units_keep_minutes_and_months_distinct() {
		let now = 40_000_000_000_000;
		for (seconds, expected) in [
			(0, "now"),
			(60, "1min"),
			(3_600, "1h"),
			(86_400, "1d"),
			(604_800, "1w"),
			(2_592_000, "1m"),
			(31_536_000, "1y"),
		] {
			assert_eq!(super::relative_time(now - seconds * 1_000_000, now), expected);
		}
		assert_eq!(super::relative_time(0, now), "—");
		assert_eq!(super::relative_time(now + 60_000_000, now), "now");
	}
}
