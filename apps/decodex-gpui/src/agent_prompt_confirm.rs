//! Explicit history confirmation waits for the existing local draft writer.
use std::{
	fs::{self, File},
	path::Path,
	time::Duration,
};

use tokio::sync::oneshot::{self, Receiver, Sender, error::RecvError};

use crate::shell::agent_surface::prompt_edit::{
	AgentActionDto, AgentClient, AgentCommandResponse, AgentSurface, Context,
	DesktopPromptEditDraft, EntityId, IdempotencyKey, PromptDraft, unique_command,
};
use decodex_protocol::{AgentExecutionOverrides, ClientFailure, PromptForkIntent};

type ConfirmationReply = Result<AgentCommandResponse, ClientFailure>;

impl AgentSurface {
	pub(super) fn confirm_prompt_editor(
		&mut self,
		expected: DesktopPromptEditDraft,
		cx: &mut Context<Self>,
	) {
		self.confirm_prompt_choice(expected, None, cx);
	}

	pub(super) fn confirm_prompt_branch(
		&mut self,
		expected: DesktopPromptEditDraft,
		boundary: decodex_protocol::PromptForkBoundary,
		cx: &mut Context<Self>,
	) {
		self.confirm_prompt_choice(expected, Some(boundary), cx);
	}

	fn confirm_prompt_choice(
		&mut self,
		expected: DesktopPromptEditDraft,
		boundary: Option<decodex_protocol::PromptForkBoundary>,
		cx: &mut Context<Self>,
	) {
		if !self.prompt_confirmation_eligible(&expected) {
			return;
		}

		let Some(profile) = self.profile.clone() else {
			return;
		};
		let resuming = expected.confirmation_key.is_some();
		let pending = if resuming {
			expected.clone()
		} else {
			let Ok(mut pending) = expected.begin_confirmation(
				IdempotencyKey::new(unique_command()).expect("bounded command identity"),
			) else {
				return;
			};

			if let Some(boundary) = boundary {
				pending.fork = Some(PromptForkIntent {
					target_work_id: EntityId::new(unique_command())
						.expect("bounded branch identity"),
					boundary,
				});
			}

			pending
		};
		let execution = self.draft_profiles.execution.choice(expected.work_id.as_str());
		let expected_execution = execution.clone();
		let worker_draft = pending.clone();
		let panel_key = self.prompt_edit.key.clone();
		let (checked, check) = oneshot::channel();
		let (permit, permitted) = oneshot::channel();
		let (completed, completion) = oneshot::channel();
		let started =
			std::thread::Builder::new().name("prompt-confirm-io".into()).spawn(move || {
				let Ok(runtime) =
					tokio::runtime::Builder::new_current_thread().enable_all().build()
				else {
					let _ = checked.send(Err("Could not start history confirmation"));

					return;
				};

				runtime.block_on(confirm_worker(
					AgentClient::new(profile),
					worker_draft,
					execution,
					(checked, permitted, completed),
				));
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

					s.stage_prompt_confirmation(
						&expected,
						&pending,
						&expected_execution,
						checked,
						cx,
					)
				})
				.unwrap_or(false);

			if !staged {
				return;
			}

			let deadline = std::time::Instant::now() + Duration::from_secs(5);

			loop {
				let ready = surface
					.update(cx, |s, cx| {
						if s.prompt_edit.key != panel_key || !s.prompt_editor_source_current() {
							return Some(false);
						}

						s.prompt_confirmation_ready(
							&pending,
							&expected_execution,
							resuming,
							deadline,
							cx,
						)
					})
					.unwrap_or(Some(false));

				if let Some(ready) = ready {
					if !ready {
						return;
					}

					break;
				}

				cx.background_executor().timer(Duration::from_millis(50)).await;
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

				s.apply_prompt_confirmation_reply(&pending, resuming, result, cx);
			});
		}));

		cx.notify();
	}

	fn apply_prompt_confirmation_reply(
		&mut self,
		pending: &DesktopPromptEditDraft,
		resuming: bool,
		result: Result<ConfirmationReply, RecvError>,
		cx: &mut Context<Self>,
	) {
		self.prompt_edit.task = None;

		match result {
			Ok(Ok(AgentCommandResponse::Rejected { .. })) => self
				.cancel_unsent_prompt_confirmation(
					pending,
					resuming,
					"Confirmation was rejected. Draft retained; read any saved receipt before reviewing again.",
					cx,
				),
			Ok(Err(_)) if !resuming => self.cancel_unsent_prompt_confirmation(
				pending,
				false,
				"Confirmation failed before dispatch. Draft retained.",
				cx,
			),
			_ => {
				// Accepted and uncertain replies both require native receipt recovery.
				if let Some(current) = self.prompt_edit.draft.clone() {
					self.recover_prompt_editor(current, cx);
				}
			},
		}

		cx.notify();
	}

	fn prompt_confirmation_eligible(&self, expected: &DesktopPromptEditDraft) -> bool {
		!(self.prompt_edit.task.is_some()
			|| !self.prompt_editor_source_current()
			|| self.prompt_edit.draft.as_ref() != Some(expected)
			|| self.prompt_edit.confirmation.as_ref() != Some(expected)
			|| expected.receipt_id.is_some()
			|| !self.command_connection_ready())
	}

	fn prompt_confirmation_ready(
		&mut self,
		pending: &DesktopPromptEditDraft,
		expected_execution: &AgentExecutionOverrides,
		resuming: bool,
		deadline: std::time::Instant,
		cx: &mut Context<Self>,
	) -> Option<bool> {
		if self.prompt_edit.draft.as_ref() != Some(pending)
			|| self.draft_profiles.execution.choice(pending.work_id.as_str()) != *expected_execution
			|| !self.command_connection_ready()
			|| std::time::Instant::now() >= deadline
		{
			self.cancel_unsent_prompt_confirmation(
				pending,
				resuming,
				"Confirmation stopped before dispatch. Draft retained.",
				cx,
			);

			return Some(false);
		}
		if self.prompt_editor_saved(pending) {
			return Some(true);
		}

		None
	}

	fn stage_prompt_confirmation(
		&mut self,
		expected: &DesktopPromptEditDraft,
		pending: &DesktopPromptEditDraft,
		expected_execution: &AgentExecutionOverrides,
		checked: Result<(), &'static str>,
		cx: &mut Context<Self>,
	) -> bool {
		let result = checked.and_then(|()| {
			if self.prompt_edit.draft.as_ref() != Some(expected)
				|| self.draft_profiles.execution.choice(expected.work_id.as_str())
					!= *expected_execution
			{
				return Err("Draft or settings changed. Review the current edit again.");
			}

			self.stage_prompt_editor(pending.clone(), cx)?;

			self.prompt_edit.draft = Some(pending.clone());

			Ok(())
		});

		if let Err(message) = result {
			self.prompt_edit.task = None;
			self.prompt_edit.feedback = message.into();

			cx.notify();

			return false;
		}

		self.prompt_edit.feedback = "Saving the confirmation record…".into();

		cx.notify();

		true
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
			current.fork = None;
			current.handback_pending = false;

			if self.stage_prompt_editor(current.clone(), cx).is_ok() {
				self.prompt_edit.draft = Some(current);
			}
		}

		self.prompt_edit.feedback = message.into();

		cx.notify();
	}
}

pub(super) fn readable_local_media(input: &PromptDraft) -> Result<(), &'static str> {
	for part in input.parts() {
		if !matches!(part["type"].as_str(), Some("localImage" | "localAudio")) {
			continue;
		}

		let path = part["path"].as_str().ok_or("A local media path is missing")?;
		let path = Path::new(path);

		if !path.is_absolute() {
			return Err(
				"A local media path is relative. Use Check edited input to resolve and save its location before continuing.",
			);
		}
		if !fs::metadata(path).is_ok_and(|metadata| metadata.is_file()) || File::open(path).is_err()
		{
			return Err("A local media file is unavailable. History was not changed.");
		}
	}

	Ok(())
}

async fn confirm_worker(
	client: AgentClient,
	worker_draft: DesktopPromptEditDraft,
	execution: AgentExecutionOverrides,
	channels: (Sender<Result<(), &'static str>>, Receiver<()>, Sender<ConfirmationReply>),
) {
	let (checked, permitted, completed) = channels;
	let result = if worker_draft
		.fork
		.as_ref()
		.is_some_and(|fork| fork.boundary == decodex_protocol::PromptForkBoundary::AfterTurn)
	{
		Ok(()) // No input is sent or restored for a branch after a completed turn.
	} else {
		match readable_local_media(&worker_draft.input) {
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
		}
	};
	let qualified = result.is_ok();

	if checked.send(result).is_err() || !qualified || permitted.await.is_err() {
		return;
	}

	let action = match worker_draft.fork {
		Some(fork) => AgentActionDto::ForkPromptEdit {
			work_id: worker_draft.work_id,
			thread_id: worker_draft.thread_id,
			review_token: worker_draft.review_token,
			target_work_id: fork.target_work_id,
			boundary: fork.boundary,
		},
		None => AgentActionDto::ConfirmPromptEdit {
			work_id: worker_draft.work_id,
			thread_id: worker_draft.thread_id,
			review_token: worker_draft.review_token,
		},
	};
	let result = client
		.execute(action, worker_draft.confirmation_key.expect("retained confirmation identity"))
		.await;
	let _ = completed.send(result);
}

#[cfg(test)]
mod tests {
	use crate::shell::agent_surface::prompt_edit::confirmation::*;

	use std::fs;

	#[test]
	fn prompt_confirm_media_check_rejects_missing_or_nonfile_paths() {
		let directory = tempfile::tempdir().unwrap();
		let file = directory.path().join("image.png");

		fs::write(&file, b"fixture").unwrap();

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
