//! One explicit generation command followed by read-only polling, without inference replay.
use std::time::Duration;

use decodex_protocol::{
	AgentActionDto, AgentClient, AgentCommandResponse, AgentSamplePhase, AgentSampleStatus,
	ClientProfile, CommandError, EntityId, IdempotencyKey, WireText,
};
use tokio::sync::watch;

pub(super) type Update = (Option<AgentSampleStatus>, String);

pub(super) async fn run(
	profile: ClientProfile,
	work: EntityId,
	thread: WireText,
	prompt: Option<WireText>,
	mut closed: watch::Receiver<bool>,
	updates: watch::Sender<Option<Update>>,
) {
	if *closed.borrow() || closed.has_changed().is_err() {
		return;
	}
	let client = AgentClient::new(profile);
	let mut expected = None;
	if let Some(prompt) = prompt {
		let key =
			IdempotencyKey::new(super::unique_command()).expect("bounded sample command identity");
		expected = Some(WireText::new(key.as_str()).expect("bounded request identity"));
		if let Ok(AgentCommandResponse::Rejected { error }) = client
			.execute(
				AgentActionDto::GenerateSample {
					work_id: work.clone(),
					thread_id: thread.clone(),
					prompt,
				},
				key,
			)
			.await
		{
			let message = match error {
				CommandError::ApplicationUnavailable { message } => message.as_str().to_owned(),
				_ => "This sample was not accepted. Refresh its status before trying again.".into(),
			};
			let _ = updates.send(Some((None, message)));
			return;
		}
	}
	loop {
		if *closed.borrow() || closed.has_changed().is_err() {
			return;
		}
		let result = client.sample(work.clone()).await;
		if *closed.borrow() || closed.has_changed().is_err() {
			return;
		}
		match result {
			Ok(state)
				if state.thread_id.as_ref().is_none_or(|id| id == &thread)
					&& expected.as_ref().is_none_or(|id| state.request_id.as_ref() == Some(id)) =>
			{
				let active = matches!(
					state.phase,
					AgentSamplePhase::Pending | AgentSamplePhase::Ready | AgentSamplePhase::NoText
				);
				let message = phase_message(state.phase).into();
				let _ = updates.send(Some((Some(state), message)));
				if !active {
					return;
				}
			},
			Ok(_) => {
				let _ = updates.send(Some((None, "The sample request could not be confirmed. Refresh its status; generation will not be repeated.".into())));
				return;
			},
			Err(_) => {
				let _ = updates.send(Some((
					None,
					"Checking sample status. The model request will not be repeated.".into(),
				)));
			},
		}
		tokio::select! { _ = closed.changed() => {}, _ = tokio::time::sleep(Duration::from_secs(1)) => {} }
	}
}

pub(super) const fn phase_message(phase: AgentSamplePhase) -> &'static str {
	match phase {
		AgentSamplePhase::Idle => "Ask for another perspective on this conversation.",
		AgentSamplePhase::Pending =>
			"Asking separately… Closing this panel leaves the request running.",
		AgentSamplePhase::Ready => "Separate answer",
		AgentSamplePhase::NoText =>
			"The model returned no text, possibly because it attempted a tool call. No tools were executed.",
		AgentSamplePhase::Failed =>
			"Could not obtain an answer. Use a completed, idle conversation and refresh before trying again.",
		AgentSamplePhase::Unconfirmed =>
			"The answer could not be confirmed and may still use quota. Refresh status before choosing a new request.",
		AgentSamplePhase::Obsolete =>
			"The conversation or its connection changed. This answer is no longer current.",
		AgentSamplePhase::Unsupported => "This Codex runtime does not support separate answers.",
	}
}
