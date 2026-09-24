//! Explicit approval of one exact observed Guardian denial.
use super::{ChiefCoordinator, ChiefError, ClientError, Value, json};

impl ChiefCoordinator {
	/// Submit user approval context. This never starts a turn or retries the action.
	pub async fn approve_guardian_denial(
		&mut self,
		id: &str,
		review_row: i64,
		digest: &str,
		key: &str,
	) -> Result<(), ChiefError> {
		let stale = || {
			ChiefError::Rejected("The review changed or its submission is already recorded.".into())
		};
		if self.dispatch_paused {
			return Err(ChiefError::Rejected("Chief is reconnecting.".into()));
		}
		let review = self
			.store
			.chief_guardian_review(id.into(), review_row)
			.await
			.map_err(|_| stale())?
			.ok_or_else(stale)?;
		if review.digest() != digest
			|| review.conflicted
			|| matches!(review.approval_state.as_deref(), Some("pending" | "submitted"))
		{
			return Err(stale());
		}
		let value: Value = serde_json::from_str(&review.event_json).map_err(|_| stale())?;
		let observed =
			decodex_codex::guardian::decode_review("item/autoApprovalReview/completed", &value)
				.ok_or_else(stale)?;
		let event = decodex_codex::guardian::core_denial_event(&observed).ok_or_else(|| {
			ChiefError::Rejected(
				"This review cannot be converted without losing action details.".into(),
			)
		})?;
		let work = self.store.get_chief_work_item(id.into()).await.map_err(|_| stale())?;
		if work.codex_thread_id.as_ref() != Some(&review.thread_id) {
			return Err(stale());
		}
		if !self.loaded_threads.contains(&review.thread_id) {
			let params = Self::resume_params(&review.thread_id);
			let response = tokio::time::timeout(
				std::time::Duration::from_secs(20),
				self.client.thread_resume(params),
			)
			.await
			.map_err(|_| stale())?
			.map_err(|_| stale())?;
			if !Self::hydrated_thread_matches(&response, &review.thread_id) {
				return Err(stale());
			}
			self.loaded_threads.insert(review.thread_id.clone());
		}
		let latest = tokio::time::timeout(
			std::time::Duration::from_secs(20),
			self.client.thread_latest_turn_id(&review.thread_id),
		)
		.await
		.map_err(|_| stale())?
		.map_err(|_| stale())?;
		if latest.as_deref() != Some(&review.turn_id) {
			return Err(ChiefError::Rejected("A newer turn superseded this review.".into()));
		}
		let claim = self
			.store
			.begin_chief_guardian_approval(
				id.into(),
				review_row,
				digest.into(),
				self.connection_id.clone(),
				self.native_generation.as_ref().map(|id| id.as_str().into()),
				key.into(),
			)
			.await
			.map_err(|_| stale())?;
		let result = tokio::time::timeout(
			std::time::Duration::from_secs(20),
			self.client.request(
				"thread/approveGuardianDeniedAction",
				json!({"threadId":review.thread_id,"event":event}),
			),
		)
		.await;
		match result {
			Ok(Ok(value)) if value.is_object() => {
				self.store.finish_chief_guardian_approval(claim, true).await?;
				Ok(())
			},
			Ok(Err(ClientError::Remote(_))) => {
				self.store.finish_chief_guardian_approval(claim, false).await?;
				Err(ChiefError::Rejected("The provider rejected the approval submission.".into()))
			},
			_ => Err(ChiefError::UnknownDispatch),
		}
	}
}
