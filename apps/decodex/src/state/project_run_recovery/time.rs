use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::state::runtime_row_parsers;

pub(in crate::state::project_run_recovery) fn timestamp_text_from_unix(unix_epoch: i64) -> String {
	OffsetDateTime::from_unix_timestamp(unix_epoch)
		.ok()
		.and_then(|timestamp| timestamp.format(&Rfc3339).ok())
		.unwrap_or_else(|| runtime_row_parsers::timestamp_parts().text)
}
