//! Source-bound configured model reads on the existing ordinary process.
use super::{ConversationRuntime, LocalSession, LocalTaskState, WorkerCommand, same_local_process};
use decodex_codex::app_server_client::NativeThreadModelSettings;
use decodex_protocol::{
	ConversationModel, ConversationModelSettingsResult as ResultDto, ConversationReasoningEffort,
};
use std::time::Duration;
#[path = "cold_model_settings.rs"] mod cold;

impl ConversationRuntime {
	pub(crate) async fn model_settings(&self, key: &str, conversation: &str) -> ResultDto {
		if self.is_shutting_down() {
			return ResultDto::Unavailable;
		}
		if !self.local().contains_key(conversation) {
			return self.cold_model_settings(key, conversation).await;
		}
		let (source, commands) = {
			let mut local = self.local();
			let Some(task) = local.get_mut(conversation) else { return ResultDto::Unavailable };
			match &task.state {
				LocalTaskState::Ready(session) => {
					let source = session.clone();
					task.state = LocalTaskState::CatalogReading(source.clone());
					(source, None)
				},
				LocalTaskState::Active { session, commands, .. } =>
					(session.clone(), Some(commands.clone())),
				_ => return ResultDto::Unavailable,
			}
		};
		let runtime = self.clone();
		let conversation = conversation.to_owned();
		// Own idle-state restoration even when the querying client disconnects.
		let query = tokio::spawn(async move {
			let before = runtime
				.inner
				.accounts
				.inspect(&source.account_id)
				.await
				.ok()
				.map(|v| v.account.revision);
			let idle = commands.is_none();
			let result = if before.is_none() {
				None
			} else if let Some(commands) = commands {
				let (reply, result) = tokio::sync::oneshot::channel();
				if commands.try_send(WorkerCommand::ModelSettings(reply)).is_ok() {
					tokio::time::timeout(Duration::from_secs(10), result)
						.await
						.ok()
						.and_then(Result::ok)
						.flatten()
				} else {
					None
				}
			} else {
				runtime.idle_model_settings(&source).await
			};
			let after = runtime
				.inner
				.accounts
				.inspect(&source.account_id)
				.await
				.ok()
				.map(|v| v.account.revision);
			let mut local = runtime.local();
			let Some(task) = local.get_mut(&conversation) else { return ResultDto::Unavailable };
			let same = match &task.state {
				LocalTaskState::CatalogReading(current) if idle =>
					same_local_process(current, &source),
				LocalTaskState::Ready(current)
				| LocalTaskState::Active { session: current, .. }
					if !idle =>
					same_local_process(current, &source),
				_ => false,
			};
			if !same {
				return ResultDto::Unavailable;
			}
			let requested_tier = source
				.execution_overrides
				.is_none_or(|intent| intent.service_tier)
				.then(|| source.service_tier.clone());
			if idle {
				task.state = LocalTaskState::Ready(source);
			}
			if before.is_none() || before != after {
				return ResultDto::Unavailable;
			}
			result
				.and_then(|settings| project(settings, requested_tier))
				.unwrap_or(ResultDto::Unavailable)
		});
		tokio::time::timeout(Duration::from_secs(12), query)
			.await
			.ok()
			.and_then(Result::ok)
			.unwrap_or(ResultDto::Unavailable)
	}

	async fn idle_model_settings(
		&self,
		source: &LocalSession,
	) -> Option<NativeThreadModelSettings> {
		let control = self.inner.process_generations.clone();
		let process = source.process.clone();
		let thread = source.codex_thread_id.clone();
		tokio::task::spawn_blocking(move || {
			control.with_fenced_child(&process, |child| {
				let (result, events) = child.read_ordinary_model_settings(&thread);
				child.retain_ordinary_events(events)?;
				result
			})
		})
		.await
		.ok()
		.and_then(Result::ok)
		.and_then(Result::ok)
		.flatten()
	}
}

fn project(
	settings: NativeThreadModelSettings,
	requested_service_tier: Option<decodex_core::ServiceTier>,
) -> Option<ResultDto> {
	Some(ResultDto::Available {
		requested_service_tier,
		model_provider: settings.model_provider,
		model: settings.model.map(ConversationModel::new).transpose().ok()?,
		reasoning_effort: settings
			.reasoning_effort
			.map(ConversationReasoningEffort::new)
			.transpose()
			.ok()?,
	})
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn ordinary_settings_read_preserves_terminal_events_and_native_identity() {
		for mode in [
			"exact-settings-valid",
			"exact-settings-foreign",
			"exact-settings-missing",
			"exact-settings-rejected",
		] {
			let (_temp, mut child) =
				crate::account_launch::process::tests::ordinary_catalog_child(mode);
			let (result, events) = child.read_ordinary_model_settings("settings-thread");
			if mode == "exact-settings-valid" {
				let settings = result.unwrap().unwrap();
				assert_eq!(settings.model.as_deref(), Some("configured-model"));
				assert_eq!(settings.reasoning_effort, None);
			} else if mode == "exact-settings-missing" {
				assert!(result.unwrap().is_none());
			} else {
				assert!(result.is_err());
			}
			child.retain_ordinary_events(events).unwrap();
			assert!(
				matches!(child.next_ordinary_turn_event(Duration::ZERO).unwrap(), Some(super::super::ConversationProcessEvent::TurnCompleted { turn_id, .. }) if turn_id == "settings-turn")
			);
			assert!(child.next_ordinary_turn_event(Duration::ZERO).unwrap().is_none());
			child.shutdown().unwrap();
		}
	}
	#[test]
	fn active_settings_read_delivers_completion_once_on_success_and_rejection() {
		for mode in ["exact-settings-valid", "exact-settings-rejected"] {
			let (_temp, mut child) =
				crate::account_launch::process::tests::ordinary_catalog_child(mode);
			let (commands, receiver) = std::sync::mpsc::channel();
			let (reply, mut result) = tokio::sync::oneshot::channel();
			commands.send(WorkerCommand::ModelSettings(reply)).unwrap();
			let (output, mut events) = tokio::sync::mpsc::channel(8);
			let shutdown = std::sync::atomic::AtomicBool::new(false);
			super::super::run_event_loop(
				&mut child,
				"settings-thread".into(),
				"settings-turn".into(),
				receiver,
				&shutdown,
				&output,
			)
			.unwrap();
			assert_eq!(result.try_recv().unwrap().is_some(), mode == "exact-settings-valid");
			assert!(
				matches!(events.try_recv().unwrap(), super::super::WorkerOutput::Event(super::super::ConversationProcessEvent::TurnCompleted { turn_id, .. }) if turn_id == "settings-turn")
			);
			assert!(events.try_recv().is_err());
			assert!(child.next_ordinary_turn_event(Duration::ZERO).unwrap().is_none());
			child.shutdown().unwrap();
		}
	}
}
