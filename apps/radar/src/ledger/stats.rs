use std::collections::BTreeMap;

use rusqlite::Connection;

pub(super) fn summary_counts(
	connection: &Connection,
) -> crate::prelude::Result<BTreeMap<String, i64>> {
	let mut result = BTreeMap::new();

	for (key, table) in [
		("upstream_commits", "upstream_commit"),
		("radar_reviews", "radar_review"),
		("artifact_links", "artifact_link"),
		("source_cache_entries", "source_cache"),
	] {
		let count =
			connection.query_row(&format!("SELECT COUNT(*) AS count FROM {table}"), [], |row| {
				row.get::<_, i64>(0)
			})?;

		result.insert(key.into(), count);
	}

	Ok(result)
}
