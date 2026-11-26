//! One desktop intent for the tab button, the selection menu, and the keyboard shortcut.
use super::*;
impl AgentSurface {
	pub(in crate::shell::agent_surface) fn end_temporary_side_chat(
		&mut self,
		id: &str,
		root: String,
		thread: String,
		cx: &mut Context<Self>,
	) {
		let Some(profile) = self.profile.clone() else { return };
		if !self.workspace.ending_side_chats.insert(id.into()) {
			return;
		}
		let action = AgentActionDto::CloseSideChat {
			work_id: EntityId::new(id).expect("validated Work identity"),
			thread_id: WireText::new(thread).expect("validated thread identity"),
		};
		let id = id.to_owned();
		let connection = profile.clone();
		let worker = cx.background_executor().spawn(async move {
			let rt = tokio::runtime::Builder::new_current_thread()
				.enable_all()
				.build()
				.map_err(|_| "Could not connect to Decodex.".to_owned())?;
			rt.block_on(async {
				let client = AgentClient::new(connection);
				let key =
					IdempotencyKey::new(agent_surface::unique_command()).expect("command identity");
				match client.execute(action, key).await {
					Ok(AgentCommandResponse::Accepted { .. }) => Ok(()),
					Ok(AgentCommandResponse::Rejected { error }) =>
						Err(agent_surface::command_rejection_message(&error)),
					_ => Err("Could not confirm the side chat closed. Try again.".to_owned()),
				}
			})
		});
		cx.spawn(async move |surface, cx| {
			let result = worker.await;
			let _ = surface.update(cx, |s, cx| {
				if s.profile.as_ref() != Some(&profile) {
					return;
				}
				s.workspace.ending_side_chats.remove(&id);
				match result {
					Ok(()) => {
						if s.conversation_page().as_ref() == Some(&id) {
							s.open_page(&root, cx);
						}
						s.workspace.pages.retain(|v| v != &id);
						s.forget_deleted_conversation(&id, cx);
					},
					Err(message) => s.feedback = crate::ui_feedback::Feedback::error(message),
				}
				s.refresh(cx);
				cx.notify();
			});
		})
		.detach();
		cx.notify();
	}

	pub(in crate::shell::agent_surface) fn request_live_side_chat(
		&mut self,
		selected: &str,
		root: &str,
		reference: Option<String>,
		cx: &mut Context<Self>,
	) {
		let Some(profile) = self.profile.clone() else { return };
		let Some(current) =
			self.conversation_record(selected).and_then(|w| w.codex_thread_id.clone())
		else {
			return;
		};
		let Some(thread) = self
			.conversation_record(root)
			.and_then(|w| w.codex_thread_id.as_ref())
			.and_then(|id| WireText::new(id.clone()).ok())
		else {
			return;
		};
		let Ok(parent) = EntityId::new(root) else { return };
		let previous = (self.prompt_editor_source_current() && self.prompt_edit.work == selected)
			.then(|| self.prompt_edit.live_side_target.clone())
			.flatten();
		let key = self.begin_prompt_review(&profile, selected, &current);
		let target = EntityId::new(previous.unwrap_or_else(agent_surface::unique_command))
			.expect("side chat identity");
		self.prompt_edit.live_side_target = Some(target.as_str().into());
		self.prompt_edit.branch_choice = Some(PromptForkBoundary::TemporarySideChat);
		self.prompt_edit.fork_requested = true;
		self.prompt_edit.side_chat_reference = reference.clone();
		self.workspace.details_visible = false;
		let root = root.to_owned();
		let request_target = target.clone();
		let worker = cx.background_executor().spawn(async move {
			let runtime = tokio::runtime::Builder::new_current_thread()
				.enable_all()
				.build()
				.map_err(|_| ("Could not connect to Decodex.".to_owned(), false))?;
			runtime.block_on(async move {
				let client = AgentClient::new(profile);
				let action = AgentActionDto::OpenSideChat {
					work_id: parent.clone(),
					thread_id: thread,
					target_work_id: request_target.clone(),
				};
				let key =
					IdempotencyKey::new(agent_surface::unique_command()).expect("command identity");
				if let Ok(AgentCommandResponse::Rejected { error }) =
					client.execute(action, key).await
				{
					return Err((agent_surface::command_rejection_message(&error), true));
				}
				let family = client.attached_branches(parent).await.map_err(|_| {
					("Could not load the side chats for this conversation.".to_owned(), false)
				})?;
				let confirmed = match &family {
					decodex_protocol::AttachedBranchesResult::Available { branches, .. } =>
						branches
							.iter()
							.find(|b| b.target_work_id == request_target)
							.map(|b| b.phase),
					_ => None,
				};
				use decodex_protocol::PromptForkPhase;
				if confirmed != Some(PromptForkPhase::Forked) {
					return Err((
						"Retry to check whether the side chat opened.".to_owned(),
						confirmed == Some(PromptForkPhase::Rejected),
					));
				}
				let snapshot = client
					.query()
					.await
					.map_err(|_| ("Could not load the conversation.".to_owned(), false))?;
				Ok((snapshot, family))
			})
		});
		self.prompt_edit.task = Some(cx.spawn(async move |surface, cx| {
			let result = worker.await;
			let _ = surface.update(cx, |s, cx| {
				if s.prompt_edit.key != key || !s.prompt_editor_source_current() {
					return;
				}
				s.prompt_edit.task = None;
				match result {
					Ok((snapshot, family)) => {
						s.reset_prompt_edit();
						s.apply_result(Ok(snapshot));
						s.install_side_family(root, family);
						s.open_page(target.as_str(), cx);
						if let Some(quote) = reference {
							s.stage_side_chat_reference(target.as_str(), &quote, cx);
						}
						#[cfg(all(target_os = "macos", not(test)))]
						s.focus_native_message(cx);
					},
					Err((reason, rejected)) => {
						if rejected {
							s.prompt_edit.live_side_target = None;
						}
						s.prompt_edit.feedback = reason;
					},
				}
				cx.notify();
			});
		}));
		cx.notify();
	}
}
