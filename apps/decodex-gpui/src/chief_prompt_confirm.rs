//! Explicit history confirmation waits for the existing local draft writer.
use super::*;

impl ChiefSurface {
	pub(super) fn confirm_prompt_editor(
		&mut self,
		expected: DesktopPromptEditDraft,
		cx: &mut Context<Self>,
	) {
		if self.prompt_edit.task.is_some()
			|| !self.prompt_editor_source_current()
			|| self.prompt_edit.draft.as_ref() != Some(&expected)
			|| self.prompt_edit.confirmation.as_ref() != Some(&expected)
			|| expected.receipt_id.is_some()
			|| !self.command_connection_ready()
		{
			return;
		}
		let Some(profile) = self.profile.clone() else {
			return;
		};
		let resuming = expected.confirmation_key.is_some();
		let pending = if resuming {
			expected.clone()
		} else {
			let Ok(pending) = expected.begin_confirmation(
				IdempotencyKey::new(unique_command()).expect("bounded command identity"),
			) else {
				return;
			};
			pending
		};
		let execution = self.draft_profiles.execution.choice(expected.work_id.as_str());
		let expected_execution = execution.clone();
		let worker_draft = pending.clone();
		let panel_key = self.prompt_edit.key.clone();
		let (checked, check) = tokio::sync::oneshot::channel();
		let (permit, permitted) = tokio::sync::oneshot::channel();
		let (completed, completion) = tokio::sync::oneshot::channel();
		let started =
			std::thread::Builder::new().name("prompt-confirm-io".into()).spawn(move || {
				let Ok(runtime) =
					tokio::runtime::Builder::new_current_thread().enable_all().build()
				else {
					let _ = checked.send(Err("Could not start history confirmation"));
					return;
				};
				runtime.block_on(async move {
					let client = ChiefClient::new(profile);
					let result = match readable_local_media(&worker_draft.input) {
						Err(message) => Err(message),
						Ok(()) => client
							.preflight_prompt_input(
								worker_draft.work_id.clone(),
								worker_draft.thread_id.clone(),
								&worker_draft.input,
								&execution,
							)
							.await
							.map_err(
								|_| "Input or thread settings could not be checked. History was not changed.",
							),
					};
					let qualified = result.is_ok();
					if checked.send(result).is_err() || !qualified || permitted.await.is_err() {
						return;
					}
					let result = client
						.execute(
							ChiefActionDto::ConfirmPromptEdit {
								work_id: worker_draft.work_id,
								thread_id: worker_draft.thread_id,
								review_token: worker_draft.review_token,
							},
							worker_draft.confirmation_key.expect("retained confirmation identity"),
						)
						.await;
					let _ = completed.send(result);
				});
			});
		if started.is_err() {
			self.prompt_edit.feedback = "Could not start history confirmation".into();
			cx.notify();
			return;
		}
		self.prompt_edit.confirmation = None;
		self.prompt_edit.feedback = "Checking edited input before changing history…".into();
		self.prompt_edit.task = Some(cx.spawn(async move |surface, cx| {
			let checked =
				check.await.unwrap_or(Err("History confirmation stopped before dispatch"));
			let staged = surface
				.update(cx, |s, cx| {
					if s.prompt_edit.key != panel_key || !s.prompt_editor_source_current() {
						return false;
					}
					let result = checked.and_then(|()| {
						if s.prompt_edit.draft.as_ref() != Some(&expected)
							|| s.draft_profiles.execution.choice(expected.work_id.as_str())
								!= expected_execution
						{
							return Err(
								"Draft or settings changed. Review the current edit again.",
							);
						}
						s.stage_prompt_editor(pending.clone(), cx)?;
						s.prompt_edit.draft = Some(pending.clone());
						Ok(())
					});
					if let Err(message) = result {
						s.prompt_edit.task = None;
						s.prompt_edit.feedback = message.into();
						cx.notify();
						return false;
					}
					s.prompt_edit.feedback = "Saving the confirmation record…".into();
					cx.notify();
					true
				})
				.unwrap_or(false);
			if !staged {
				return;
			}
			let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
			loop {
				let ready = surface
					.update(cx, |s, cx| {
						if s.prompt_edit.key != panel_key || !s.prompt_editor_source_current() {
							return Some(false);
						}
						if s.prompt_edit.draft.as_ref() != Some(&pending)
							|| s.draft_profiles.execution.choice(pending.work_id.as_str())
								!= expected_execution
							|| !s.command_connection_ready()
							|| std::time::Instant::now() >= deadline
						{
							s.cancel_unsent_prompt_confirmation(
								&pending,
								resuming,
								"Confirmation stopped before dispatch. Draft retained.",
								cx,
							);
							return Some(false);
						}
						if s.prompt_editor_saved(&pending) {
							return Some(true);
						}
						None
					})
					.unwrap_or(Some(false));
				if let Some(ready) = ready {
					if !ready {
						return;
					}
					break;
				}
				cx.background_executor().timer(std::time::Duration::from_millis(50)).await;
			}
			if permit.send(()).is_err() {
				let _ = surface.update(cx, |s, cx| {
					s.cancel_unsent_prompt_confirmation(
						&pending,
						resuming,
						"Confirmation worker stopped before dispatch.",
						cx,
					)
				});
				return;
			}
			let result = completion.await;
			let _ = surface.update(cx, |s, cx| {
				if s.prompt_edit.key != panel_key || !s.prompt_editor_source_current() {
					return;
				}
				s.prompt_edit.task = None;
				match result {
					Ok(Ok(ChiefCommandResponse::Rejected { .. })) => s
						.cancel_unsent_prompt_confirmation(
							&pending,
							false,
							"Confirmation was rejected. Draft retained; review the history again.",
							cx,
						),
					Ok(Err(_)) if !resuming => s.cancel_unsent_prompt_confirmation(
						&pending,
						false,
						"Confirmation failed before dispatch. Draft retained.",
						cx,
					),
					_ => {
						// Accepted and uncertain replies both require native receipt recovery.
						if let Some(current) = s.prompt_edit.draft.clone() {
							s.recover_prompt_editor(current, cx);
						}
					},
				}
				cx.notify();
			});
		}));
		cx.notify();
	}

	fn cancel_unsent_prompt_confirmation(
		&mut self,
		pending: &DesktopPromptEditDraft,
		retain_prior_uncertainty: bool,
		message: &str,
		cx: &mut Context<Self>,
	) {
		if !self.prompt_editor_source_current()
			|| self.prompt_edit.draft.as_ref().is_none_or(|current| {
				current.work_id != pending.work_id
					|| current.thread_id != pending.thread_id
					|| current.confirmation_key != pending.confirmation_key
			}) {
			return;
		}
		self.prompt_edit.task = None;
		if !retain_prior_uncertainty
			&& let Some(mut current) = self.prompt_edit.draft.clone()
			&& current.confirmation_key == pending.confirmation_key
			&& current.receipt_id.is_none()
		{
			current.confirmation_key = None;
			current.handback_pending = false;
			if self.stage_prompt_editor(current.clone(), cx).is_ok() {
				self.prompt_edit.draft = Some(current);
			}
		}
		self.prompt_edit.feedback = message.into();
		cx.notify();
	}
}

fn readable_local_media(input: &PromptDraft) -> Result<(), &'static str> {
	for part in input.parts() {
		if !matches!(part["type"].as_str(), Some("localImage" | "localAudio")) {
			continue;
		}
		let path = part["path"].as_str().ok_or("A local media path is missing")?;
		let path = std::path::Path::new(path);
		if !path.is_absolute() {
			return Err(
				"A local media path is relative. Keep this draft and resolve the file location before changing history.",
			);
		}
		if !std::fs::metadata(path).is_ok_and(|metadata| metadata.is_file())
			|| std::fs::File::open(path).is_err()
		{
			return Err("A local media file is unavailable. History was not changed.");
		}
	}
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn prompt_confirm_media_check_rejects_missing_or_nonfile_paths() {
		let directory = tempfile::tempdir().unwrap();
		let file = directory.path().join("image.png");
		std::fs::write(&file, b"fixture").unwrap();
		for kind in ["localImage", "localAudio"] {
			let input =
				PromptDraft::new(vec![serde_json::json!({"type":kind,"path":file})]).unwrap();
			assert!(readable_local_media(&input).is_ok());
			for path in [
				directory.path().to_path_buf(),
				directory.path().join("missing"),
				std::path::PathBuf::from("relative.png"),
			] {
				let input =
					PromptDraft::new(vec![serde_json::json!({"type":kind,"path":path})]).unwrap();
				assert!(readable_local_media(&input).is_err());
			}
		}
	}
}
