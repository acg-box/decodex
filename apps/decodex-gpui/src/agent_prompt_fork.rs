//! Recover explicit branches through saved native identity; never create a second fork.
use super::*;
use decodex_protocol::{PromptForkBoundary, PromptForkPhase, PromptForkResult};

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
		let (send, receive) = tokio::sync::oneshot::channel();
		let started =
			std::thread::Builder::new().name("prompt-fork-recovery".into()).spawn(move || {
				let result = (|| {
					let runtime = tokio::runtime::Builder::new_current_thread()
						.enable_all()
						.build()
						.map_err(|_| "Could not start branch recovery")?;
					runtime.block_on(async {
						let client = AgentClient::new(profile);
						let _ = client
							.execute(
								AgentActionDto::RecoverPromptFork {
									work_id: original.work_id.clone(),
									review_token: original.review_token.clone(),
								},
								IdempotencyKey::new(unique_command())
									.map_err(|_| "Invalid recovery identity")?,
							)
							.await;
						let result = client
							.prompt_fork(original.work_id.clone(), original.review_token.clone())
							.await
							.map_err(|_| "Branch receipt is unavailable")?;
						let PromptForkResult::Available(Some(status)) = result else {
							return Err(
								"Branch receipt is unavailable. Keep this draft and read the receipt again.",
							);
						};
						let intent = original.fork.as_ref().ok_or("Branch intent is missing")?;
						if status.thread_id != original.thread_id
							|| status.target_work_id != intent.target_work_id
							|| status.boundary != intent.boundary
						{
							return Err("Branch receipt does not match this draft");
						}
						match status.phase {
							PromptForkPhase::Uncertain =>
								return Err(
									"Branch acceptance is uncertain. Keep the saved draft; do not create the branch again.",
								),
							PromptForkPhase::Acknowledged =>
								return Err(
									"Branch identity is saved. Read the receipt again to finish history recovery.",
								),
							PromptForkPhase::Rejected => {
								let mut retained = original;
								retained.fork = None;
								retained.confirmation_key = None;
								retained.handback_pending = false;
								return Ok((retained, None));
							},
							PromptForkPhase::Forked => {},
						}
						let target_thread =
							status.target_thread_id.ok_or("Branch identity is missing")?;
						let mut retained = original;
						retained.fork = None;
						retained.confirmation_key = None;
						retained.handback_pending = false;
						if status.boundary == PromptForkBoundary::BeforeInput {
							let (edit, content) = client
								.prompt_edit(status.target_work_id.clone(), target_thread.clone())
								.await
								.map_err(|_| "Branch input receipt is unavailable")?;
							let input =
								PromptDraft::new(content.ok_or("Branch input is unavailable")?)?;
							retained.work_id = status.target_work_id.clone();
							retained.thread_id = target_thread.clone();
							retained = retained.recover_receipt(&edit, &input)?;
							if retained.receipt_id != status.edit_receipt_id {
								return Err("Branch draft receipt changed");
							}
						}
						let snapshot =
							client.query().await.map_err(|_| "Branch task list is unavailable")?;
						let AgentSnapshotResult::Available(ref available) = snapshot else {
							return Err("Branch task list is unavailable");
						};
						if !available.work_items.iter().any(|w| {
							w.id == status.target_work_id.as_str()
								&& w.codex_thread_id.as_deref() == Some(target_thread.as_str())
						}) {
							return Err("Branch task is not ready");
						}
						Ok((
							retained,
							Some((status.target_work_id, target_thread, status.boundary, snapshot)),
						))
					})
				})();
				let _ = send.send(result);
			});
		if started.is_err() {
			self.prompt_edit.feedback = "Could not start branch recovery".into();
			cx.notify();
			return;
		}
		self.prompt_edit.feedback = "Reading branch identity and history…".into();
		self.prompt_edit.task = Some(cx.spawn(async move |surface, cx| {
			let result = receive.await.unwrap_or(Err("Branch recovery stopped"));
			let _ = surface.update(cx, |s, cx| {
				if s.prompt_edit.key != key || !s.prompt_editor_source_current() { return }
				s.prompt_edit.task = None;
				if s.prompt_edit.draft.as_ref() != Some(&expected) { s.prompt_edit.feedback = "Draft changed. Read the branch receipt again.".into(); cx.notify(); return }
				match result {
					Ok((retained, target)) => {
						if let Err(message) = s.stage_prompt_editor(retained.clone(), cx) { s.prompt_edit.feedback = message.into(); cx.notify(); return }
						match target {
							None => { s.prompt_edit.draft = Some(retained); s.prompt_edit.feedback = "Branch was rejected. Original conversation and edited draft retained.".into(); },
							Some((work, thread, boundary, snapshot)) => {
								s.apply_result(Ok(snapshot));
								s.open_page(work.as_str(), cx);
								if boundary == PromptForkBoundary::BeforeInput {
									s.prompt_edit = Panel { work: work.as_str().into(), thread: thread.as_str().into(), profile: s.profile.clone(), ..Default::default() };
									s.prompt_edit.feedback = match s.install_prompt_editors(retained, cx) { Ok(()) => "Branch created. Original conversation unchanged. Finish restoring the edited draft before sending.", Err(message) => message }.into();
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
