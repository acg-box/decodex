//! Read call timing without deriving content from the transcript.
use crate::{SqliteStore, StoreError, error::sqlite_error};

/// Presentation facts for one authorized call.
#[derive(Clone, Debug)]
pub struct AgentVoicePresentation {
	/// Exact local and native realtime session identity.
	pub session: String,
	/// Elapsed authorization time for a closed call.
	pub duration_ms: Option<u64>,
}

impl SqliteStore {
	/// Read recorded call timing without loading speech or generated metadata.
	pub async fn agent_voice_presentations(
		&self,
		work: String,
	) -> Result<Vec<AgentVoicePresentation>, StoreError> {
		self.run(move |connection| {
			let mut query = connection
				.prepare("SELECT session_id,created_at_micros,closed_at_micros FROM agent_voice_calls WHERE work_id=?1 ORDER BY rowid")
				.map_err(sqlite_error)?;
			let rows = query.query_map([work], |row| {
				let created: i64 = row.get(1)?;
				let closed: Option<i64> = row.get(2)?;
				Ok(AgentVoicePresentation {
					session: row.get(0)?,
					duration_ms: closed.map(|end| end.saturating_sub(created) as u64 / 1000),
				})
			}).map_err(sqlite_error)?;
			rows.collect::<Result<Vec<_>, _>>().map_err(|error| sqlite_error(error).into())
		})
		.await
	}
}
