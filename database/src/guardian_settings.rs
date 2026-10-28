//! Optional review records, independent of conversation permission settings.
use rusqlite::OptionalExtension as _;

use crate::{DatabaseError, SqliteStore, StoreError, error};
use decodex_core::guardian::{GuardianSettings, GuardianState, ReviewDetails};
impl SqliteStore {
	pub async fn guardian_state(&self) -> Result<GuardianState, StoreError> {
		self.run(|c|{
			let tx=c.transaction().map_err(crate::error::sqlite_error)?;
			let settings=tx.query_row("SELECT revision,review_details FROM guardian_settings WHERE singleton=1",[],|r|Ok(GuardianSettings{revision:r.get::<_,i64>(0)? as u64,review_details:r.get(1)?})).map_err(crate::error::sqlite_error)?;
			let mut details=tx.prepare("SELECT reviewer_thread_id,reviewer_turn_id,item_id,review_id,record_json,parent_thread_id FROM guardian_review_details ORDER BY id DESC LIMIT 32").map_err(crate::error::sqlite_error)?.query_map([],|r|Ok(ReviewDetails{parent_thread_id:r.get(5)?,truncated:false,reviewer_thread_id:r.get(0)?,reviewer_turn_id:r.get(1)?,item_id:r.get(2)?,review_id:r.get(3)?,record:r.get(4)?})).map_err(crate::error::sqlite_error)?.collect::<Result<Vec<_>,_>>().map_err(crate::error::sqlite_error)?;

			for record in &mut details { if record.record.len()>256 { let end=record.record.char_indices().map(|(i,_)|i).take_while(|i|*i<=256).last().unwrap_or(0); record.record.truncate(end); record.truncated=true; } }

			tx.commit().map_err(crate::error::sqlite_error)?;

			Ok(GuardianState{settings,details,observation_limit:"Optional native assistant output and interruption records only. Retain up to 128 records of 64 KiB each; show the newest 32 previews. After a completed review, history recovery checks the newest 32 private sources in the same directory and five turns per source, then verifies the native parent. Exact action-to-private-review association is unavailable. Native resume cannot subscribe to raw details; bounded recovery can leave gaps. These records never complete Work.".into()})
		}).await
	}

	pub async fn set_review_details(
		&self,
		expected_revision: u64,
		review_details: bool,
	) -> Result<(), StoreError> {
		self.run(move|c|{
			let n=c.execute("UPDATE guardian_settings SET revision=revision+1,review_details=?2 WHERE singleton=1 AND revision=?1",rusqlite::params![i64::try_from(expected_revision).map_err(|_|StoreError::InvalidInput("invalid settings revision"))?,review_details]).map_err(crate::error::sqlite_error)?;

			if n!=1{return Err(DatabaseError::Conflict.into());}Ok(())
		}).await
	}

	pub async fn record_review_details(
		&self,
		parent: Option<String>,
		details: ReviewDetails,
	) -> Result<(), StoreError> {
		if details.record.len() > 65_536
			|| decodex_core::contains_credential_material(&details.record)
		{
			return Err(StoreError::InvalidInput("private record cannot be retained"));
		}

		self.run(move|c|{let tx=c.transaction().map_err(crate::error::sqlite_error)?;
			tx.execute("INSERT INTO guardian_review_details(parent_thread_id,reviewer_thread_id,reviewer_turn_id,item_id,review_id,record_json) SELECT ?1,?2,?3,?4,?5,?6 WHERE (SELECT review_details FROM guardian_settings WHERE singleton=1)=1 ON CONFLICT(reviewer_thread_id,reviewer_turn_id,item_id) DO NOTHING",rusqlite::params![parent.unwrap_or_default(),details.reviewer_thread_id,details.reviewer_turn_id,details.item_id,details.review_id,details.record]).map_err(crate::error::sqlite_error)?;
			tx.execute("DELETE FROM guardian_review_details WHERE id NOT IN(SELECT id FROM guardian_review_details ORDER BY id DESC LIMIT 128)",[]).map_err(crate::error::sqlite_error)?;
			tx.commit().map_err(crate::error::sqlite_error)?;Ok(())}).await
	}
}

impl SqliteStore {
	pub async fn private_review_record(
		&self,
		thread: String,
		turn: String,
		item: String,
	) -> Result<Option<String>, StoreError> {
		self.run(move|c|c.query_row("SELECT record_json FROM guardian_review_details WHERE reviewer_thread_id=?1 AND reviewer_turn_id=?2 AND item_id=?3",rusqlite::params![thread,turn,item],|r|r.get(0)).optional().map_err(|e|error::sqlite_error(e).into())).await
	}
}

impl SqliteStore {
	pub async fn guardian_settings(&self) -> Result<GuardianSettings, StoreError> {
		self.run(|c| {
			c.query_row(
				"SELECT revision,review_details FROM guardian_settings WHERE singleton=1",
				[],
				|r| {
					Ok(GuardianSettings {
						revision: r.get::<_, i64>(0)? as u64,
						review_details: r.get(1)?,
					})
				},
			)
			.map_err(|e| error::sqlite_error(e).into())
		})
		.await
	}
}
