//! Recover explicit branches through saved native identity; never create a second fork.

use tokio::sync::oneshot;

#[cfg(test)] use crate::shell::agent_surface::prompt_edit::Entity;
use crate::shell::{
	agent_surface,
	agent_surface::prompt_edit::{
		AgentActionDto, AgentClient, AgentSnapshotResult, AgentSurface, Context,
		DesktopPromptEditDraft, EntityId, IdempotencyKey, Panel, PromptDraft, WireText,
	},
};
use decodex_protocol::{PromptForkBoundary, PromptForkPhase, PromptForkResult};

type BranchRecovery = Result<
	(DesktopPromptEditDraft, Option<(EntityId, WireText, PromptForkBoundary, AgentSnapshotResult)>),
	&'static str,
>;

impl AgentSurface {
	pub(super) fn recover_prompt_branch(
		&mut self,
		expected: DesktopPromptEditDraft,
		cx: &mut Context<Self>,
	) {
		if self.prompt_edit.task.is_some()
			|| !self.prompt_editor_source_current()
			|| self.prompt_edit.draft.as_ref() != Some(&expected)
			|| !self.command_connection_ready()
		{
			return;
		}

		let Some(profile) = self.profile.clone() else { return };
		let key = self.prompt_edit.key.clone();
		let original = expected.clone();
		let (send, receive) = oneshot::channel();
		let started =
			std::thread::Builder::new().name("prompt-fork-recovery".into()).spawn(move || {
				let result = (|| {
					let runtime = tokio::runtime::Builder::new_current_thread()
						.enable_all()
						.build()
						.map_err(|_| "Could not start fork recovery")?;

					runtime.block_on(recover_branch(AgentClient::new(profile), original))
				})();
				let _ = send.send(result);
			});

		if started.is_err() {
			self.prompt_edit.feedback = "Could not start fork recovery".into();

			cx.notify();

			return;
		}

		self.prompt_edit.feedback = "Reading fork identity and history…".into();
		self.prompt_edit.task = Some(cx.spawn(async move |surface, cx| {
			let result = receive.await.unwrap_or(Err("Fork recovery stopped"));
			let _ = surface.update(cx, |s, cx| {
				if s.prompt_edit.key != key || !s.prompt_editor_source_current() {
					return;
				}

				s.prompt_edit.task = None;

				if s.prompt_edit.draft.as_ref() != Some(&expected) {
					s.prompt_edit.feedback =
						"Draft changed. Check the new conversation again.".into();

					cx.notify();

					return;
				}

				match result {
					Ok((retained, target)) => {
						if let Err(message) = s.stage_prompt_editor(retained.clone(), cx) {
							s.prompt_edit.feedback = message.into();

							cx.notify();

							return;
						}

						match target {
							None => {
								s.prompt_edit.draft = Some(retained);
								s.prompt_edit.feedback =
									"Fork rejected. Original and draft kept.".into();
							},
							Some((work, thread, boundary, snapshot)) => {
								if boundary != PromptForkBoundary::BeforeInput
									&& let Err(message) = s.discard_prompt_editor(&retained, cx)
								{
									s.prompt_edit.feedback = message.into();

									cx.notify();

									return;
								}

								let continuation = (boundary == PromptForkBoundary::AfterTurn
									&& s.composer_manager.as_deref().or(s.selected.as_deref())
										== Some(retained.work_id.as_str()))
								.then(|| {
									(
										s.composer.read(cx).content().to_owned(),
										s.attachments.clone(),
										s.task_references.clone(),
									)
								});

								s.apply_result(Ok(snapshot));
								s.open_page(work.as_str(), cx);
								if let Some((text, attachments, references)) = continuation {
									s.composer.update(cx, |input, cx| input.set_content(&text, cx));
									s.attachments = attachments;
									s.task_references = references;
									s.save_draft_document(cx);
								}

								if boundary == PromptForkBoundary::BeforeInput {
									s.prompt_edit = Panel {
										work: work.as_str().into(),
										thread: thread.as_str().into(),
										profile: s.profile.clone(),
										..Default::default()
									};
									s.prompt_edit.feedback = match s
										.install_prompt_editors(retained, cx)
									{
										Ok(()) => "Fork created. Restore the draft before sending.",
										Err(message) => message,
									}
									.into();
								}
							},
						}
					},
					Err(message) => s.prompt_edit.feedback = message.into(),
				}

				cx.notify();
			});
		}));

		cx.notify();
	}
}

async fn recover_branch(client: AgentClient, original: DesktopPromptEditDraft) -> BranchRecovery {
	let _ = client
		.execute(
			AgentActionDto::RecoverPromptFork {
				work_id: original.work_id.clone(),
				review_token: original.review_token.clone(),
			},
			IdempotencyKey::new(agent_surface::unique_command())
				.map_err(|_| "Invalid recovery identity")?,
		)
		.await;
	let result = client
		.prompt_fork(original.work_id.clone(), original.review_token.clone())
		.await
		.map_err(|_| "New conversation status unavailable")?;
	let PromptForkResult::Available(Some(status)) = result else {
		return Err("New conversation unavailable. Keep your draft and check again.");
	};
	let intent = original.fork.as_ref().ok_or("Fork intent is missing")?;

	if status.thread_id != original.thread_id
		|| status.target_work_id != intent.target_work_id
		|| status.boundary != intent.boundary
		|| status.model_provider != intent.model_provider
	{
		return Err("Saved fork does not match this draft");
	}

	match status.phase {
		PromptForkPhase::Uncertain =>
			return Err("Fork unconfirmed. Keep the draft; do not create the fork again."),
		PromptForkPhase::Acknowledged =>
			return Err("New conversation saved. Check status to finish restoring history."),
		PromptForkPhase::Rejected => {
			let mut retained = original;

			retained.fork = None;
			retained.confirmation_key = None;
			retained.handback_pending = false;

			return Ok((retained, None));
		},
		PromptForkPhase::Forked => {},
	}

	let target_thread = status.target_thread_id.ok_or("Fork identity is missing")?;
	let mut retained = original;

	retained.fork = None;
	retained.confirmation_key = None;
	retained.handback_pending = false;

	if status.boundary == PromptForkBoundary::BeforeInput {
		let (edit, content) = client
			.prompt_edit(status.target_work_id.clone(), target_thread.clone())
			.await
			.map_err(|_| "Copied message status unavailable")?;
		let input = PromptDraft::new(content.ok_or("Fork input is unavailable")?)?;

		retained.work_id = status.target_work_id.clone();
		retained.thread_id = target_thread.clone();
		retained = retained.recover_receipt(&edit, &input)?;

		if retained.receipt_id != status.edit_receipt_id {
			return Err("The copied draft changed");
		}
	}

	let snapshot = client.query().await.map_err(|_| "Fork task list is unavailable")?;
	let AgentSnapshotResult::Available(ref available) = snapshot else {
		return Err("Fork task list is unavailable");
	};

	if !available.work_items.iter().any(|w| {
		w.id == status.target_work_id.as_str()
			&& w.codex_thread_id.as_deref() == Some(target_thread.as_str())
	}) {
		return Err("Fork task is not ready");
	}

	Ok((retained, Some((status.target_work_id, target_thread, status.boundary, snapshot))))
}

#[cfg(test)]
#[path = "agent_prompt_fork_wire_tests.rs"]
mod tests;
