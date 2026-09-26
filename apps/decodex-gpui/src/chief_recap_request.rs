//! One submission followed by read-only polling; never replay an uncertain command.
use super::*;
type Update = Option<(Option<TaskRecapStatus>, String)>;

pub(super) async fn run(
	profile: ClientProfile,
	owner: EntityId,
	thread: WireText,
	generate: bool,
	mut cancellation: watch::Receiver<bool>,
	updates: watch::Sender<Update>,
) {
	if *cancellation.borrow() || cancellation.has_changed().is_err() {
		return;
	}
	let client = ChiefClient::new(profile);
	let key = IdempotencyKey::new(unique_command()).expect("bounded command identity");
	let mut cancellable = generate;
	let mut request = generate.then(|| WireText::new(key.as_str()).expect("bounded identity"));
	if generate {
		let _ = client
			.execute(
				ChiefActionDto::GenerateRecap { work_id: owner.clone(), thread_id: thread.clone() },
				key,
			)
			.await;
	}
	loop {
		if *cancellation.borrow() || cancellation.has_changed().is_err() {
			if cancellable && let Some(request_id) = request {
				let key = IdempotencyKey::new(unique_command()).expect("bounded command identity");
				let _ = client
					.execute(
						ChiefActionDto::CancelRecap { work_id: owner.clone(), request_id },
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
					_ = tokio::time::sleep(std::time::Duration::from_secs(2)) => {},
				}
				continue;
			},
		};
		request = state.request_id.clone();
		cancellable = matches!(state.phase, Phase::Pending | Phase::Cancelling);
		let active = matches!(state.phase, Phase::Pending | Phase::Cancelling | Phase::Ready);
		let message = match state.phase {
			Phase::Idle => "Generate a short recap of this conversation.",
			Phase::Pending => "Generating recap…",
			Phase::Cancelling => "Cancelling recap…",
			Phase::Ready => "Task recap",
			Phase::Failed => "Could not generate a recap. You can try again.",
			Phase::Cancelled => "This recap was cancelled or is no longer current.",
		};
		let _ = updates.send(Some((Some(state), message.into())));
		if !active {
			return;
		}
		tokio::select! {
			_ = cancellation.changed() => {},
			_ = tokio::time::sleep(std::time::Duration::from_secs(1)) => {},
		}
	}
}
