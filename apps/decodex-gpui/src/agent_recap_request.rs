//! One submission followed by read-only polling; never replay an uncertain command.
use tokio::{
	sync::watch::{Receiver, Sender},
	time,
};

use crate::shell::agent_surface::recap::{
	self, AgentActionDto, AgentClient, AgentCommandResponse, ClientProfile, EntityId,
	IdempotencyKey, TaskRecapPhase, TaskRecapStatus, WireText,
};
use decodex_protocol::CommandError;

type Update = Option<(Option<TaskRecapStatus>, String)>;

pub(super) async fn run(
	profile: ClientProfile,
	owner: EntityId,
	thread: WireText,
	generate: bool,
	mut cancellation: Receiver<bool>,
	updates: Sender<Update>,
) {
	if *cancellation.borrow() || cancellation.has_changed().is_err() {
		return;
	}

	let client = AgentClient::new(profile);
	let key = IdempotencyKey::new(recap::unique_command()).expect("bounded command identity");
	let mut cancellable = generate;
	let mut request = generate.then(|| WireText::new(key.as_str()).expect("bounded identity"));

	if generate {
		let outcome = client
			.execute(
				AgentActionDto::GenerateRecap { work_id: owner.clone(), thread_id: thread.clone() },
				key,
			)
			.await;

		if let Ok(AgentCommandResponse::Rejected { error }) = outcome {
			let feedback = match error {
				CommandError::ApplicationUnavailable { message } => message.as_str().to_owned(),
				_ => "The recap request was not accepted. Refresh to review its current state."
					.into(),
			};
			let _ = updates.send(Some((None, feedback)));

			return;
		}
	}

	loop {
		if *cancellation.borrow() || cancellation.has_changed().is_err() {
			if cancellable && let Some(request_id) = request {
				let key =
					IdempotencyKey::new(recap::unique_command()).expect("bounded command identity");
				let _ = client
					.execute(
						AgentActionDto::CancelRecap { work_id: owner.clone(), request_id },
						key,
					)
					.await;
			}

			let _ = updates.send(Some((
				None,
				"Recap cancelled locally. Refresh to check service status.".into(),
			)));

			return;
		}

		let response = client.recap(owner.clone()).await;

		if *cancellation.borrow() || cancellation.has_changed().is_err() {
			continue;
		}

		let state = match response {
			Ok(state)
				if state.thread_id.as_ref().is_none_or(|id| id == &thread)
					&& (!generate || state.request_id == request) =>
				state,
			Ok(_) => {
				let _ = updates.send(Some((
					None,
					"Recap could not be confirmed. Refresh before trying again.".into(),
				)));

				return;
			},
			Err(_) => {
				let _ = updates.send(Some((
					None,
					"Recap status is unavailable. Checking again; generation will not be repeated."
						.into(),
				)));

				tokio::select! {
					_ = cancellation.changed() => {},
					_ = time::sleep(std::time::Duration::from_secs(2)) => {},
				}

				continue;
			},
		};

		request = state.request_id.clone();
		cancellable = matches!(state.phase, TaskRecapPhase::Pending | TaskRecapPhase::Cancelling);

		let active = matches!(
			state.phase,
			TaskRecapPhase::Pending | TaskRecapPhase::Cancelling | TaskRecapPhase::Ready
		);
		let message = match state.phase {
			TaskRecapPhase::Idle => "Generate a short recap of this conversation.",
			TaskRecapPhase::Pending => "Generating recap…",
			TaskRecapPhase::Cancelling => "Cancelling recap…",
			TaskRecapPhase::Ready => "Task recap",
			TaskRecapPhase::Failed => "Could not generate a recap. You can try again.",
			TaskRecapPhase::Cancelled => "This recap was cancelled or is no longer current.",
		};
		let _ = updates.send(Some((Some(state), message.into())));

		if !active {
			return;
		}

		tokio::select! {
			_ = cancellation.changed() => {},
			_ = time::sleep(std::time::Duration::from_secs(1)) => {},
		}
	}
}
