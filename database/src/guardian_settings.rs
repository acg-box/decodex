//! Global intent and per-source receipts. Uncertain publication is never replayed.
use crate::{DatabaseError, SqliteStore, StoreError, error::sqlite_error};
use decodex_core::guardian::{
	GuardianSettings, GuardianSource, GuardianState, ReviewDetails, ReviewPreference,
};
use rusqlite::{OptionalExtension as _, params};
impl SqliteStore {
	pub async fn guardian_state(&self) -> Result<GuardianState, StoreError> {
		self.run(|c|{
			let tx=c.transaction().map_err(sqlite_error)?;
			let settings=tx.query_row("SELECT revision,reviewer,review_details FROM guardian_settings WHERE singleton=1",[],|r|Ok(GuardianSettings{revision:r.get::<_,i64>(0)? as u64,reviewer:if r.get::<_,String>(1)?=="user"{ReviewPreference::AskMe}else{ReviewPreference::AutomaticReview},review_details:r.get(2)?})).map_err(sqlite_error)?;
			let sources=tx.prepare("SELECT agent_id,thread_id,revision,outcome,effective_reviewer,detail,source_id FROM guardian_sources ORDER BY agent_id LIMIT 100").map_err(sqlite_error)?.query_map([],|r|Ok(GuardianSource{source_id:r.get(6)?,agent_id:r.get(0)?,thread_id:r.get(1)?,revision:r.get::<_,i64>(2)? as u64,outcome:r.get(3)?,effective_reviewer:r.get(4)?,detail:r.get(5)?})).map_err(sqlite_error)?.collect::<Result<Vec<_>,_>>().map_err(sqlite_error)?;
			let mut details=tx.prepare("SELECT reviewer_thread_id,reviewer_turn_id,item_id,review_id,record_json,parent_thread_id FROM guardian_review_details ORDER BY id DESC LIMIT 32").map_err(sqlite_error)?.query_map([],|r|Ok(ReviewDetails{parent_thread_id:r.get(5)?,truncated:false,reviewer_thread_id:r.get(0)?,reviewer_turn_id:r.get(1)?,item_id:r.get(2)?,review_id:r.get(3)?,record:r.get(4)?})).map_err(sqlite_error)?.collect::<Result<Vec<_>,_>>().map_err(sqlite_error)?;
			for record in &mut details { if record.record.len()>256 { let end=record.record.char_indices().map(|(i,_)|i).take_while(|i|*i<=256).last().unwrap_or(0); record.record.truncate(end); record.truncated=true; } }
			tx.commit().map_err(sqlite_error)?;
			Ok(GuardianState{settings,sources,details,observation_limit:"Optional native assistant output and interruption records only. Retain up to 128 records of 64 KiB each; show the newest 32 previews. After a completed review, history recovery checks the newest 32 private sources in the same directory and five turns per source, then verifies the native parent. Exact action-to-private-review association is unavailable. Native resume cannot subscribe to raw details; bounded recovery can leave gaps. These records never complete Work.".into()})
		}).await
	}

	pub async fn set_guardian_settings(
		&self,
		expected_revision: u64,
		reviewer: ReviewPreference,
		review_details: bool,
	) -> Result<(), StoreError> {
		self.run(move|c|{
			let n=c.execute("UPDATE guardian_settings SET revision=revision+1,reviewer=?2,review_details=?3 WHERE singleton=1 AND revision=?1",params![i64::try_from(expected_revision).map_err(|_|StoreError::InvalidInput("invalid settings revision"))?,reviewer.native(),review_details]).map_err(sqlite_error)?;
			if n!=1{return Err(DatabaseError::Conflict.into());}Ok(())
		}).await
	}

	/// Reserve before the native write. Existing unknown attempts stay fenced across restart.
	pub async fn reserve_guardian_source(
		&self,
		source: GuardianSource,
	) -> Result<bool, StoreError> {
		self.run(move|c|{
			let previous=c.query_row("SELECT thread_id,revision,outcome,source_id FROM guardian_sources WHERE agent_id=?1",[&source.agent_id],|r|Ok((r.get::<_,String>(0)?,r.get::<_,i64>(1)? as u64,r.get::<_,String>(2)?,r.get::<_,String>(3)?))).optional().map_err(sqlite_error)?;
			if previous.as_ref().is_some_and(|(thread,rev,_state,identity)|identity==&source.source_id && thread==&source.thread_id && *rev==source.revision){return Ok(false);}
			c.execute("INSERT INTO guardian_sources(source_id,agent_id,thread_id,revision,outcome,effective_reviewer,detail) VALUES(?4,?1,?2,?3,'pending',NULL,'Reserved before native publication') ON CONFLICT(agent_id) DO UPDATE SET source_id=excluded.source_id,thread_id=excluded.thread_id,revision=excluded.revision,outcome=excluded.outcome,effective_reviewer=NULL,detail=excluded.detail",params![source.agent_id,source.thread_id,i64::try_from(source.revision).map_err(|_|StoreError::InvalidInput("invalid settings revision"))?,source.source_id]).map_err(sqlite_error)?;Ok(true)
		}).await
	}

	pub async fn observe_guardian_source(&self, source: GuardianSource) -> Result<(), StoreError> {
		self.run(move|c|{c.execute("UPDATE guardian_sources SET outcome=?4,effective_reviewer=?5,detail=?6 WHERE agent_id=?1 AND thread_id=?2 AND revision=?3 AND source_id=?7",params![source.agent_id,source.thread_id,i64::try_from(source.revision).map_err(|_|StoreError::InvalidInput("invalid settings revision"))?,source.outcome,source.effective_reviewer,source.detail,source.source_id]).map_err(sqlite_error)?;Ok(())}).await
	}

	pub async fn record_review_details(
		&self,
		parent: Option<String>,
		details: ReviewDetails,
	) -> Result<(), StoreError> {
		if details.record.len() > 65536
			|| decodex_core::contains_credential_material(&details.record)
		{
			return Err(StoreError::InvalidInput("private record cannot be retained"));
		}
		self.run(move|c|{let tx=c.transaction().map_err(sqlite_error)?;
			tx.execute("INSERT INTO guardian_review_details(parent_thread_id,reviewer_thread_id,reviewer_turn_id,item_id,review_id,record_json) SELECT ?1,?2,?3,?4,?5,?6 WHERE (SELECT review_details FROM guardian_settings WHERE singleton=1)=1 ON CONFLICT(reviewer_thread_id,reviewer_turn_id,item_id) DO NOTHING",params![parent.unwrap_or_default(),details.reviewer_thread_id,details.reviewer_turn_id,details.item_id,details.review_id,details.record]).map_err(sqlite_error)?;
			tx.execute("DELETE FROM guardian_review_details WHERE id NOT IN(SELECT id FROM guardian_review_details ORDER BY id DESC LIMIT 128)",[]).map_err(sqlite_error)?;
			tx.commit().map_err(sqlite_error)?;Ok(())}).await
	}
}

impl SqliteStore {
	pub async fn private_review_record(
		&self,
		thread: String,
		turn: String,
		item: String,
	) -> Result<Option<String>, StoreError> {
		self.run(move|c|c.query_row("SELECT record_json FROM guardian_review_details WHERE reviewer_thread_id=?1 AND reviewer_turn_id=?2 AND item_id=?3",params![thread,turn,item],|r|r.get(0)).optional().map_err(|e|sqlite_error(e).into())).await
	}
}

impl SqliteStore {
	pub async fn guardian_settings(&self) -> Result<GuardianSettings, StoreError> {
		self.run(|c| {
			c.query_row(
				"SELECT revision,reviewer,review_details FROM guardian_settings WHERE singleton=1",
				[],
				|r| {
					Ok(GuardianSettings {
						revision: r.get::<_, i64>(0)? as u64,
						reviewer: if r.get::<_, String>(1)? == "user" {
							ReviewPreference::AskMe
						} else {
							ReviewPreference::AutomaticReview
						},
						review_details: r.get(2)?,
					})
				},
			)
			.map_err(|e| sqlite_error(e).into())
		})
		.await
	}
}
