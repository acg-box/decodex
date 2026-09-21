//! Explicit continuation of an exact reviewed provider precaution.
use super::{ChiefCoordinator, ChiefError, ClientError, Value, exact, json};

pub(crate) fn review_token(
	review: &decodex_database::ChiefMisalignment,
	guard: &decodex_codex::app_server_client::HistoryGuard,
) -> Option<String> {
	use sha2::{Digest as _, Sha256};
	let identity = guard.live_review_identity()?;
	Some(
		Sha256::digest(json!([review.review_id(), identity]).to_string().as_bytes())
			.iter()
			.map(|byte| format!("{byte:02x}"))
			.collect(),
	)
}

pub(crate) fn details(error: &Value) -> Option<String> {
	error.get("misalignment").filter(|value|value.is_object()).map(|value| {
        let explanation=value["detailedExplanation"].as_str().filter(|text|!text.trim().is_empty() && text.len()<=65536);
        let steer=value.pointer("/steer/message").and_then(Value::as_str).filter(|text|!text.trim().is_empty() && text.len()<=1024);
        json!({"detailedExplanation":explanation,"steer":steer.map(|message|json!({"message":message}))}).to_string()
    })
}

impl ChiefCoordinator {
	/// The host calls this only for an explicit acknowledgment of the displayed review.
	pub async fn continue_misalignment(
		&mut self,
		id: &str,
		review: decodex_database::ChiefMisalignment,
		key: &str,
		expected_review: &str,
	) -> Result<(), ChiefError> {
		let (live_error, guard) =
			self.client.live_misalignment_review(&review.thread_id, &review.turn_id).ok_or_else(
				|| ChiefError::Rejected("Live provider findings are no longer available.".into()),
			)?;
		if details(&live_error) != review.details_json
			|| review_token(&review, &guard).as_deref() != Some(expected_review)
		{
			return Err(ChiefError::Rejected("The live provider findings changed.".into()));
		}
		let work = self.store.get_chief_work_item(id.into()).await?;
		if self.dispatch_paused
			|| work.dispatch_state != decodex_database::ChiefDispatchState::Idle
			|| work.codex_thread_id.as_ref() != Some(&review.thread_id)
			|| self.store.chief_misalignment(id.into()).await?.as_ref() != Some(&review)
		{
			return Err(ChiefError::Rejected(
				"The displayed precaution is no longer current.".into(),
			));
		}
		let effort = if self.is_manager(id).await? {
			self.config.chief_effort.clone()
		} else {
			self.config.worker_effort.clone()
		};
		let mut resume = self.work_thread_resume_params(&work).await?;
		resume["threadId"] = json!(review.thread_id);
		resume["excludeTurns"] = json!(true);
		let resumed = self.client.thread_resume(resume).await?;
		if exact(&resumed, "/thread/id")? != review.thread_id
			|| resumed["model"].as_str() != Some(self.config.model.as_str())
			|| resumed["reasoningEffort"].as_str() != Some(effort.as_str())
		{
			return Err(ChiefError::Rejected("Continuation thread changed.".into()));
		}
		if self.client.thread_latest_turn_id(&review.thread_id).await?.as_deref()
			!= Some(review.turn_id.as_str())
		{
			return Err(ChiefError::Rejected("A newer turn superseded these findings.".into()));
		}
		let history = self.client.thread_read_turn(&review.thread_id, &review.turn_id).await?;
		let turn = history
			.pointer("/thread/turns")
			.and_then(Value::as_array)
			.and_then(|turns| {
				turns.iter().find(|turn| turn["id"].as_str() == Some(&review.turn_id))
			})
			.ok_or_else(|| ChiefError::Rejected("Exact failed turn unavailable.".into()))?;
		if turn["status"] != "failed"
			|| turn["error"]["codexErrorInfo"] != "misalignmentPolicyViolation"
			|| (!turn["error"]["misalignment"].is_null()
				&& details(&turn["error"]) != review.details_json)
		{
			self.observe_misalignment(&review.thread_id, &review.turn_id, &turn["error"]).await?;
			return Err(ChiefError::Rejected(
				"The provider findings changed. Review them again.".into(),
			));
		}
		let value: Value = serde_json::from_str(review.details_json.as_deref().unwrap_or("null"))
			.map_err(|_| ChiefError::Rejected("Findings unavailable.".into()))?;
		let text = value
			.pointer("/steer/message")
			.and_then(Value::as_str)
			.ok_or_else(|| ChiefError::Rejected("No continuation was supplied.".into()))?;
		let timestamp = std::time::SystemTime::now()
			.duration_since(std::time::UNIX_EPOCH)
			.map_err(|_| ChiefError::Rejected("Clock before epoch.".into()))?
			.as_millis();
		let event = self
			.store
			.begin_chief_misalignment_continuation(id.into(), review.clone(), key.into())
			.await?;
		let result=self.client.request_with_history("turn/start",json!({"threadId":review.thread_id,"model":self.config.model,"effort":effort,"input":[{"type":"text","text":text,"text_elements":[]}],"responsesapiClientMetadata":{"misalignment_override":json!({"timestamp":timestamp}).to_string()}}),guard).await;
		match result {
			Ok(value) => {
				let turn = exact(&value, "/turn/id")?;
				self.store
					.finish_chief_misalignment_continuation(id.into(), event, review, Some(turn))
					.await?;
				Ok(())
			},
			Err(ClientError::Remote(error)) => {
				self.store
					.finish_chief_misalignment_continuation(id.into(), event, review, None)
					.await?;
				Err(ChiefError::Rejected(format!("Continuation rejected: {}", error.message)))
			},
			Err(ClientError::StaleHistory) => {
				self.store
					.finish_chief_misalignment_continuation(id.into(), event, review, None)
					.await?;
				Err(ChiefError::Rejected(
					"The live conversation changed before continuation was sent.".into(),
				))
			},
			Err(error) => Err(error.into()),
		}
	}
}
