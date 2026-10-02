//! Explicit continuation of an exact reviewed provider precaution.
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest as _, Sha256};

use crate::agent::{self, AgentCoordinator, AgentError, ClientError, Value};
use decodex_codex::app_server_client::HistoryGuard;
use decodex_database::{AgentDispatchState, AgentMisalignment};

impl AgentCoordinator {
	pub(super) async fn superseded_misalignment(
		&self,
		work: &str,
		thread: &str,
		turns: &[Value],
	) -> Result<Option<AgentMisalignment>, AgentError> {
		let expected = self.store.agent_misalignment(work.into()).await?;

		Ok(expected.filter(|review| {
			review.thread_id == thread
				&& turns
					.iter()
					.position(|turn| turn["id"].as_str() == Some(&review.turn_id))
					.is_some_and(|index| index + 1 < turns.len())
		}))
	}

	/// The host calls this only for an explicit acknowledgment of the displayed review.
	pub async fn continue_misalignment(
		&mut self,
		id: &str,
		review: AgentMisalignment,
		key: &str,
		expected_review: &str,
	) -> Result<(), AgentError> {
		let (live_error, guard) =
			self.client.live_misalignment_review(&review.thread_id, &review.turn_id).ok_or_else(
				|| AgentError::Rejected("Live provider findings are no longer available.".into()),
			)?;

		if details(&live_error) != review.details_json
			|| review_token(&review, &guard).as_deref() != Some(expected_review)
		{
			return Err(AgentError::Rejected("The live provider findings changed.".into()));
		}

		let work = self.store.get_agent_work_item(id.into()).await?;

		if self.dispatch_paused
			|| work.dispatch_state != AgentDispatchState::Idle
			|| work.codex_thread_id.as_ref() != Some(&review.thread_id)
			|| self.store.agent_misalignment(id.into()).await?.as_ref() != Some(&review)
		{
			return Err(AgentError::Rejected(
				"The displayed precaution is no longer current.".into(),
			));
		}

		let resume = Self::resume_params(&review.thread_id);
		let resumed = self.client.thread_resume(resume).await?;

		if !Self::hydrated_thread_matches(&resumed, &review.thread_id) {
			return Err(AgentError::Rejected("Continuation thread changed.".into()));
		}
		if self.client.thread_latest_turn_id(&review.thread_id).await?.as_deref()
			!= Some(review.turn_id.as_str())
		{
			return Err(AgentError::Rejected("A newer turn superseded these findings.".into()));
		}

		let history = self.client.thread_read_turn(&review.thread_id, &review.turn_id).await?;
		let turn = history
			.pointer("/thread/turns")
			.and_then(Value::as_array)
			.and_then(|turns| {
				turns.iter().find(|turn| turn["id"].as_str() == Some(&review.turn_id))
			})
			.ok_or_else(|| AgentError::Rejected("Exact failed turn unavailable.".into()))?;

		if turn["status"] != "failed"
			|| turn["error"]["codexErrorInfo"] != "misalignmentPolicyViolation"
			|| (!turn["error"]["misalignment"].is_null()
				&& details(&turn["error"]) != review.details_json)
		{
			self.observe_misalignment(&review.thread_id, &review.turn_id, &turn["error"]).await?;

			return Err(AgentError::Rejected(
				"The provider findings changed. Review them again.".into(),
			));
		}

		let value: Value = serde_json::from_str(review.details_json.as_deref().unwrap_or("null"))
			.map_err(|_| AgentError::Rejected("Findings unavailable.".into()))?;
		let text = value
			.pointer("/steer/message")
			.and_then(Value::as_str)
			.ok_or_else(|| AgentError::Rejected("No continuation was supplied.".into()))?;
		let timestamp = SystemTime::now()
			.duration_since(UNIX_EPOCH)
			.map_err(|_| AgentError::Rejected("Clock before epoch.".into()))?
			.as_millis();
		let event = self
			.store
			.begin_agent_misalignment_continuation(id.into(), review.clone(), key.into())
			.await?;
		let result=self.client.request_with_history("turn/start",serde_json::json!({"threadId":review.thread_id,"input":[{"type":"text","text":text,"text_elements":[]}],"responsesapiClientMetadata":{"misalignment_override":serde_json::json!({"timestamp":timestamp}).to_string()}}),guard).await;

		match result {
			Ok(value) => {
				let turn = agent::exact(&value, "/turn/id")?;

				self.store
					.finish_agent_misalignment_continuation(id.into(), event, review, Some(turn))
					.await?;

				Ok(())
			},
			Err(ClientError::Remote(_)) => {
				self.store
					.finish_agent_misalignment_continuation(id.into(), event, review, None)
					.await?;

				Err(AgentError::Rejected(
					"Continuation was rejected. Review the latest conversation status before trying again.".into(),
				))
			},
			Err(ClientError::StaleHistory) => {
				self.store
					.finish_agent_misalignment_continuation(id.into(), event, review, None)
					.await?;

				Err(AgentError::Rejected(
					"The live conversation changed before continuation was sent.".into(),
				))
			},
			Err(error) => Err(error.into()),
		}
	}
}

pub(crate) fn review_token(review: &AgentMisalignment, guard: &HistoryGuard) -> Option<String> {
	let identity = guard.live_review_identity()?;

	Some(
		Sha256::digest(serde_json::json!([review.review_id(), identity]).to_string().as_bytes())
			.iter()
			.map(|byte| format!("{byte:02x}"))
			.collect(),
	)
}

pub(crate) fn details(error: &Value) -> Option<String> {
	error.get("misalignment").filter(|value|value.is_object()).map(|value| {
        let explanation=value["detailedExplanation"].as_str().filter(|text|!text.trim().is_empty() && text.len()<=65_536);
        let steer=value.pointer("/steer/message").and_then(Value::as_str).filter(|text|!text.trim().is_empty() && text.len()<=1_024);

        serde_json::json!({"detailedExplanation":explanation,"steer":steer.map(|message|serde_json::json!({"message":message}))}).to_string()
    })
}
