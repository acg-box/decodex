use time::{Duration, OffsetDateTime, format_description::well_known::Rfc3339};

pub(in crate::recovery) fn current_timestamp() -> String {
	OffsetDateTime::now_utc().format(&Rfc3339).expect("timestamp formatting should succeed")
}

pub(in crate::recovery) fn timestamp_after_seconds(seconds: i64) -> String {
	(OffsetDateTime::now_utc() + Duration::seconds(seconds))
		.format(&Rfc3339)
		.expect("timestamp formatting should succeed")
}
