//! Acknowledge only a saved draft and freshly applied history presentation.
use super::*;

impl ChiefSurface {
	pub(super) fn handback_prompt_editor(
		&mut self,
		expected: DesktopPromptEditDraft,
		cx: &mut Context<Self>,
	) {
		if self.prompt_edit.task.is_some()
			|| !self.prompt_editor_source_current()
			|| self.prompt_edit.draft.as_ref() != Some(&expected)
			|| expected.receipt_id.is_none()
			|| !expected.handback_pending
			|| !self.command_connection_ready()
		{
			return;
		}
		let Some(profile) = self.profile.clone() else {
			return;
		};
		let original = expected.clone();
		let panel_key = self.prompt_edit.key.clone();
		let (loaded, load) = tokio::sync::oneshot::channel();
		let (permit, permitted) = tokio::sync::oneshot::channel();
		let (completed, completion) = tokio::sync::oneshot::channel();
		let started =
			std::thread::Builder::new().name("prompt-handback-io".into()).spawn(move || {
				let Ok(runtime) =
					tokio::runtime::Builder::new_current_thread().enable_all().build()
				else {
					let _ = loaded.send(Err("Could not start draft restoration"));
					return;
				};
				runtime.block_on(async move {
					let client = ChiefClient::new(profile);
					let result = load_restored_history(&client, &original).await;
					let ready = result.is_ok();
					if loaded.send(result).is_err() || !ready || permitted.await.is_err() {
						return;
					}
					let result = async {
						let _ = client
							.execute(
								ChiefActionDto::AcknowledgePromptEditDraft {
									work_id: original.work_id.clone(),
									thread_id: original.thread_id.clone(),
									receipt_id: original
										.receipt_id
										.ok_or("Missing edit receipt")?,
									review_token: original.review_token.clone(),
								},
								IdempotencyKey::new(unique_command())
									.map_err(|_| "Invalid handback identity")?,
							)
							.await;
						let (status, content) = client
							.prompt_edit(original.work_id.clone(), original.thread_id.clone())
							.await
							.map_err(
								|_| "Draft handback could not be confirmed. Read its receipt again.",
							)?;
						if status.phase != PromptEditPhase::Restored {
							return Err(
								"Draft handback is not confirmed. Keep the draft and read its receipt again.",
							);
						}
						original.recover_receipt(
							&status,
							&PromptDraft::new(content.ok_or("Original input is unavailable")?)?,
						)
					}
					.await;
					let _ = completed.send(result);
				});
			});
		if started.is_err() {
			self.prompt_edit.feedback = "Could not start draft restoration".into();
			cx.notify();
			return;
		}
		self.prompt_edit.feedback = "Refreshing history before restoring the draft…".into();
		self.prompt_edit.task = Some(cx.spawn(async move |surface, cx| {
			let result = load.await.unwrap_or(Err("Draft restoration stopped"));
			let pending = surface.update(cx, |s, cx| {
				if s.prompt_edit.key != panel_key || !s.prompt_editor_source_current() { return None; }
				let result = result.and_then(|(draft, history, timeline)| {
					if s.prompt_edit.draft.as_ref() != Some(&expected) { return Err("Draft changed during restoration. Try again with the current edit."); }
					s.stage_prompt_editor(draft.clone(), cx)?;
					s.prompt_edit.draft = Some(draft.clone());
					s.restore_prompt_presentation(draft.work_id.as_str(), draft.thread_id.as_str(), history, timeline, cx)?;
					Ok(draft)
				});
				match result {
					Ok(draft) => { s.prompt_edit.feedback = "History refreshed. Waiting for the edited draft to be saved…".into(); cx.notify(); Some(draft) },
					Err(message) => { s.prompt_edit.task = None; s.prompt_edit.feedback = message.into(); cx.notify(); None },
				}
			}).ok().flatten();
			let Some(pending) = pending else { return; };
			let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
			loop {
				let ready = surface.update(cx, |s, cx| {
					if s.prompt_edit.key != panel_key || !s.prompt_editor_source_current() { return Some(false); }
					if s.prompt_edit.draft.as_ref() != Some(&pending) || !s.command_connection_ready() || std::time::Instant::now() >= deadline {
						s.prompt_edit.task = None; s.prompt_edit.feedback = "Draft handback was not sent. Keep this draft and restore it again.".into(); cx.notify(); return Some(false);
					}
					if s.prompt_editor_saved(&pending) { return Some(true); }
					None
				}).unwrap_or(Some(false));
				if let Some(ready) = ready { if !ready { return; } break; }
				cx.background_executor().timer(std::time::Duration::from_millis(50)).await;
			}
			if permit.send(()).is_err() { return; }
			let result = completion.await.unwrap_or(Err("Draft handback reply was lost. Read its receipt again."));
			let _ = surface.update(cx, |s, cx| {
				if s.prompt_edit.key != panel_key || !s.prompt_editor_source_current() { return; }
				s.prompt_edit.task = None;
				s.prompt_edit.feedback = if s.prompt_edit.draft.as_ref() != Some(&pending) {
					"Draft changed. Read the handback receipt again before sending.".into()
				} else {
					match result.and_then(|draft| { s.stage_prompt_editor(draft.clone(), cx)?; s.prompt_edit.draft = Some(draft); Ok(()) }) {
						Ok(()) => "Edited draft restored. Nothing was sent.".into(), Err(message) => message.into(),
					}
				};
				cx.notify();
			});
		}));
		cx.notify();
	}
}

async fn load_restored_history(
	client: &ChiefClient,
	original: &DesktopPromptEditDraft,
) -> Result<
	(DesktopPromptEditDraft, ChiefHistoryResult, decodex_protocol::ChiefTimelineResult),
	&'static str,
> {
	let _ = client
		.execute(
			ChiefActionDto::RecoverPromptEdit {
				work_id: original.work_id.clone(),
				thread_id: original.thread_id.clone(),
			},
			IdempotencyKey::new(unique_command()).map_err(|_| "Invalid recovery identity")?,
		)
		.await;
	let (status, content) = client
		.prompt_edit(original.work_id.clone(), original.thread_id.clone())
		.await
		.map_err(|_| "Edit receipt is unavailable")?;
	if !matches!(status.phase, PromptEditPhase::Applied | PromptEditPhase::Restored) {
		return Err("History edit is not yet confirmed. Keep the draft and recover again.");
	}
	let recovered = original.recover_receipt(
		&status,
		&PromptDraft::new(content.ok_or("Original input is unavailable")?)?,
	)?;
	let history =
		client.history(original.work_id.clone()).await.map_err(|_| "History refresh failed")?;
	let timeline = client
		.timeline(
			original.work_id.clone(),
			EntityId::new(original.thread_id.as_str()).map_err(|_| "Invalid history identity")?,
			None,
		)
		.await
		.map_err(|_| "Native history refresh failed")?;
	Ok((recovered, history, timeline))
}
