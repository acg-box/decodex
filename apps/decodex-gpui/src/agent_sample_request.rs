//! One explicit generation command followed by read-only polling, without inference replay.
use std::time::Duration;

use tokio::{
	sync::watch::{Receiver, Sender},
	time,
};

use decodex_protocol::{
	AgentActionDto, AgentClient, AgentCommandResponse, AgentSamplePhase, AgentSampleStatus,
	ClientProfile, CommandError, EntityId, IdempotencyKey, WireText,
};

pub(super) type Update = (Option<AgentSampleStatus>, String);

pub(super) enum Operation {
	Read,
	Generate(WireText),
	Cancel(WireText),
}

pub(super) const fn phase_message(phase: AgentSamplePhase) -> &'static str {
	match phase {
		AgentSamplePhase::Idle => "Ask for another perspective.",
		AgentSamplePhase::Pending => "Asking separately… Continues if you close this panel.",
		AgentSamplePhase::Cancelling => "Cancelling the answer…",
		AgentSamplePhase::Cancelled => "The separate answer was cancelled.",
		AgentSamplePhase::Ready => "Separate answer",
		AgentSamplePhase::NoText =>
			"No text returned. The model may have requested a tool; no tools ran.",
		AgentSamplePhase::Failed =>
			"Answer unavailable. Wait for the current turn to finish, then retry.",
		AgentSamplePhase::Unconfirmed =>
			"Answer unconfirmed; may still use quota. Refresh status before asking again.",
		AgentSamplePhase::Obsolete => "Conversation changed. This answer is outdated.",
		AgentSamplePhase::Unsupported => "Separate answers unsupported by this Codex version.",
	}
}

pub(super) async fn run(
	profile: ClientProfile,
	work: EntityId,
	thread: WireText,
	operation: Operation,
	mut closed: Receiver<bool>,
	updates: Sender<Option<Update>>,
) {
	if *closed.borrow() || closed.has_changed().is_err() {
		return;
	}

	let client = AgentClient::new(profile);
	let (expected, action) = match operation {
		Operation::Read => (None, None),
		Operation::Generate(prompt) => {
			let key = IdempotencyKey::new(super::unique_command())
				.expect("bounded sample command identity");

			(
				Some(WireText::new(key.as_str()).expect("bounded request identity")),
				Some((
					AgentActionDto::GenerateSample {
						work_id: work.clone(),
						thread_id: thread.clone(),
						prompt,
					},
					key,
				)),
			)
		},
		Operation::Cancel(request_id) => {
			let key = IdempotencyKey::new(super::unique_command())
				.expect("bounded cancellation command identity");

			(
				Some(request_id.clone()),
				Some((AgentActionDto::CancelSample { work_id: work.clone(), request_id }, key)),
			)
		},
	};

	if let Some((action, key)) = action
		&& let Ok(AgentCommandResponse::Rejected { error }) = client.execute(action, key).await
	{
		let message = match error {
			CommandError::ApplicationUnavailable { message } => message.as_str().to_owned(),
			_ => "Request rejected. Refresh status before retrying.".into(),
		};
		let _ = updates.send(Some((None, message)));

		return;
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
					AgentSamplePhase::Pending
						| AgentSamplePhase::Cancelling
						| AgentSamplePhase::Ready
						| AgentSamplePhase::NoText
				);
				let message = phase_message(state.phase).into();
				let _ = updates.send(Some((Some(state), message)));

				if !active {
					return;
				}
			},
			Ok(_) => {
				let _ = updates.send(Some((
					None,
					"Request unconfirmed. Refresh status; generation will not repeat.".into(),
				)));

				return;
			},
			Err(_) => {
				let _ = updates
					.send(Some((None, "Checking status. The request will not repeat.".into())));
			},
		}

		tokio::select! { _ = closed.changed() => {}, _ = time::sleep(Duration::from_secs(1)) => {} }
	}
}
