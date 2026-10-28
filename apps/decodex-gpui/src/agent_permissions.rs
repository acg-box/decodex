//! Native permission presets for the current Agent and independent creation defaults.
use gpui::AnyElement;
#[cfg(test)] use gpui::px;
use tokio::runtime::Builder;

#[cfg(test)] use crate::shell::agent_surface::AgentDispatchStateDto;
#[cfg(test)] use crate::shell::agent_surface::AgentSnapshotResult;
#[cfg(test)] use crate::shell::agent_surface::AgentWorkStatusDto;
#[cfg(test)] use crate::shell::agent_surface::ClientProfile;
#[cfg(test)] use crate::shell::agent_surface::Entity;
#[cfg(test)] use crate::shell::agent_surface::Render;
#[cfg(test)] use crate::shell::agent_surface::Window;
use crate::{
	shell::agent_surface::{
		self, AgentActionDto, AgentClient, AgentCommandResponse, AgentSnapshotDto, AgentSurface,
		Context, EntityId, IdempotencyKey, IntoElement, ParentElement, Styled, Task, WireText,
		mcp_forms,
	},
	ui_feedback::{self, Feedback},
};
use decodex_protocol::{AgentPermissionState, PermissionMode};

#[derive(Default)]
pub(super) struct Panel {
	state: Option<AgentPermissionState>,
	work: Option<String>,
	task: Option<Task<()>>,
	epoch: u64,
	feedback: Feedback,
	reviewed: bool,
}
impl AgentSurface {
	pub(super) fn reset_permission_profiles(&mut self) {
		self.permission_profiles =
			Panel { epoch: self.permission_profiles.epoch.wrapping_add(1), ..Default::default() };
	}

	pub(super) fn invalidate_permission_profiles(&mut self, next: &AgentSnapshotDto) {
		let Some(work) = &self.permission_profiles.work else {
			return;
		};
		let before =
			self.snapshot.as_ref().and_then(|s| s.work_items.iter().find(|w| &w.id == work));
		let after = next.work_items.iter().find(|w| &w.id == work);

		if !matches!((before,after),(Some(a),Some(b)) if a.codex_thread_id==b.codex_thread_id && a.dispatch_state==b.dispatch_state && a.active_turn_id==b.active_turn_id)
			|| self.snapshot.as_ref().is_none_or(|s| s.runtime_source != next.runtime_source)
		{
			self.reset_permission_profiles();
		}
	}

	fn permission_selection(
		&self,
		work_id: &EntityId,
		thread: &str,
		profile_id: WireText,
		mode: Option<PermissionMode>,
	) -> Option<AgentActionDto> {
		let AgentPermissionState::Available {
			work_id: reviewed_work,
			thread_id,
			review_token,
			profiles,
			can_update: true,
			..
		} = self.permission_profiles.state.as_ref()?
		else {
			return None;
		};

		if reviewed_work != work_id
			|| thread_id.as_str() != thread
			|| self.permission_profiles.work.as_deref() != Some(work_id.as_str())
			|| !profiles.iter().any(|p| p.id == profile_id && p.allowed && p.can_select)
		{
			return None;
		}

		Some(AgentActionDto::SelectPermissions {
			work_id: work_id.clone(),
			thread_id: thread_id.clone(),
			review_token: review_token.clone(),
			profile_id,
			mode,
		})
	}

	pub(super) fn update_permission_profiles(
		&mut self,
		work: String,
		profile_id: Option<WireText>,
		mode: Option<PermissionMode>,
		cx: &mut Context<Self>,
	) {
		if self.permission_profiles.task.is_some()
			|| self.selected.as_ref() != Some(&work)
			|| !self.command_connection_ready()
			|| self.native_agents.selected.is_some()
			|| (profile_id.is_some() && !self.permission_profiles.reviewed)
		{
			return;
		}

		let Some(profile) = self.profile.clone() else {
			return;
		};
		let Some(snapshot) = &self.snapshot else {
			return;
		};
		let Some(source) = snapshot.runtime_source.clone() else {
			return;
		};
		let Some(thread) = snapshot
			.work_items
			.iter()
			.find(|w| w.id == work)
			.and_then(|w| w.codex_thread_id.clone())
		else {
			return;
		};
		let Ok(work_id) = EntityId::new(work.clone()) else {
			return;
		};
		let action = if let Some(profile_id) = profile_id {
			let Some(action) = self.permission_selection(&work_id, &thread, profile_id, mode)
			else {
				return;
			};

			Some(action)
		} else {
			None
		};
		let saving = action.is_some();
		let epoch = self.begin_permission_read(&work, saving);
		let key =
			IdempotencyKey::new(agent_surface::unique_command()).expect("bounded command identity");
		let future = cx.background_executor().spawn(async move {
			let runtime = Builder::new_current_thread().enable_all().build().ok()?;
			let client = AgentClient::new(profile);
			let outcome = action.map(|action| runtime.block_on(client.execute(action, key)));
			let state = runtime.block_on(read_selection(
				&client,
				work_id,
				matches!(outcome, Some(Ok(AgentCommandResponse::Accepted { .. }))),
			));

			Some((outcome, state))
		});

		self.permission_profiles.task = Some(cx.spawn(async move |surface, cx| {
			let result = future.await;
			let _ = surface.update(cx, |s, cx| {
				if s.permission_profiles.epoch != epoch {
					return;
				}

				s.permission_profiles.task = None;

				let current = s.command_connection_ready()
					&& s.native_agents.selected.is_none()
					&& s.selected.as_ref() == Some(&work)
					&& s.snapshot.as_ref().is_some_and(|snapshot| {
						snapshot.runtime_source.as_ref() == Some(&source)
							&& snapshot.work_items.iter().any(|w| {
								w.id == work && w.codex_thread_id.as_deref() == Some(&thread)
							})
					});

				if !current {
					s.reset_permission_profiles();
					cx.notify();

					return;
				}

				let (outcome, state) = result.unwrap_or((None, AgentPermissionState::Unavailable));

				s.permission_profiles.reviewed =
					matches!(state, AgentPermissionState::Available { .. });
				s.permission_profiles.feedback = match outcome {
					Some(Ok(AgentCommandResponse::Accepted { .. }))
						if matches!(
							state,
							AgentPermissionState::Available {
								last_outcome: Some(
									decodex_protocol::AgentPermissionOutcome::TargetObserved
								),
								..
							}
						) =>
						Feedback::success("Permissions updated"),
					Some(Ok(AgentCommandResponse::Accepted { .. })) =>
						Feedback::info("Selection queued. Check current permissions below."),
					Some(Ok(AgentCommandResponse::Rejected { .. })) =>
						Feedback::error("Selection rejected. Refresh before retrying."),
					Some(_) => Feedback::warning("Selection unconfirmed. No automatic retry."),
					None if saving =>
						Feedback::warning("Selection unconfirmed. Refresh before retrying."),
					None => Feedback::default(),
				};
				s.permission_profiles.state = Some(match state {
					AgentPermissionState::Available { ref work_id, ref thread_id, .. }
						if work_id.as_str() != work || thread_id.as_str() != thread =>
						AgentPermissionState::Unavailable,
					other => other,
				});

				cx.notify();
			});
		}));

		cx.notify();
	}

	fn begin_permission_read(&mut self, work: &str, saving: bool) -> u64 {
		self.permission_profiles.epoch = self.permission_profiles.epoch.wrapping_add(1);

		let epoch = self.permission_profiles.epoch;

		self.permission_profiles.work = Some(work.to_owned());
		self.permission_profiles.state = None;
		self.permission_profiles.reviewed = false;
		self.permission_profiles.feedback = Feedback::loading(if saving {
			"Updating permissions…"
		} else {
			"Reading permissions…"
		});

		epoch
	}

	fn permission_owner(&self) -> Option<String> {
		if self.is_new_conversation() {
			return None;
		}
		self.composer_manager.clone().or_else(|| self.root_id())
	}

	pub(super) fn creation_permission_mode(&self) -> PermissionMode {
		if self.is_new_conversation() {
			self.workspace.new_conversation_permissions.unwrap_or(self.permission_mode)
		} else {
			self.permission_mode
		}
	}

	pub(super) fn permission_label(&self) -> String {
		if self.permission_owner().is_none() {
			return self.creation_permission_mode().label().into();
		}
		if self.permission_profiles.work != self.permission_owner() {
			return "Permissions".into();
		}
		match &self.permission_profiles.state {
			Some(AgentPermissionState::Available { mode: Some(mode), .. }) => mode.label().into(),
			Some(AgentPermissionState::Available { .. }) => "Custom permissions".into(),
			Some(AgentPermissionState::Pending { .. }) => "Updating permissions…".into(),
			_ => "Permissions".into(),
		}
	}

	pub(super) fn refresh_composer_permissions(&mut self, cx: &mut Context<Self>) {
		if let Some(work) = self.permission_owner()
			&& self.native_agents.selected.is_none()
			&& self.permission_profiles.work.as_ref() != Some(&work)
			&& self.permission_profiles.task.is_none()
		{
			self.update_permission_profiles(work, None, None, cx);
		}
	}

	fn choose_permission_mode(&mut self, mode: PermissionMode, cx: &mut Context<Self>) {
		if let Some(work) = self.permission_owner() {
			self.update_permission_profiles(
				work,
				Some(WireText::new(mode.profile()).expect("native preset")),
				Some(mode),
				cx,
			);
		} else {
			if self.is_new_conversation() {
				self.workspace.new_conversation_permissions = Some(mode);
			} else {
				self.permission_mode = mode;
			}
			self.save_draft_document(cx);
		}
		cx.notify();
	}

	pub(super) fn permission_options(&self, cx: &mut Context<Self>) -> AnyElement {
		let creating = self.permission_owner().is_none();
		let current = if creating {
			Some(self.creation_permission_mode())
		} else {
			match &self.permission_profiles.state {
				Some(AgentPermissionState::Available { mode, .. }) => *mode,
				_ => None,
			}
		};
		let available = |mode: PermissionMode| {
			creating
				|| matches!(&self.permission_profiles.state,
			Some(AgentPermissionState::Available { profiles, can_update: true, .. })
			if self.permission_profiles.reviewed && profiles.iter().any(|p| p.id.as_str() == mode.profile() && p.allowed && p.can_select))
		};
		let mut menu = crate::ui_motion::menu_surface(gpui::div()).p_1().flex().flex_col().child(
			crate::session_settings::permission_choices(
				current,
				true,
				available,
				Self::choose_permission_mode,
				cx,
			),
		);
		if !creating {
			menu = menu.children(ui_feedback::inline_feedback(
				"permission-settings-feedback",
				"",
				&self.permission_profiles.feedback,
			));
			if let Some(AgentPermissionState::Available { profiles, can_update, .. }) =
				&self.permission_profiles.state
			{
				for (index, profile) in
					profiles.iter().enumerate().filter(|(_, p)| !p.id.as_str().starts_with(':'))
				{
					let (work, id) =
						(self.permission_owner().expect("existing Agent"), profile.id.clone());
					if *can_update && profile.allowed && profile.can_select {
						menu = menu.child(mcp_forms::mcp_button(
							format!("permission-profile-{index}"),
							id.as_str().into(),
							false,
							cx,
							move |s, cx| {
								s.update_permission_profiles(
									work.clone(),
									Some(id.clone()),
									None,
									cx,
								)
							},
						));
					}
				}
			}
			if !self.permission_profiles.reviewed
				&& let Some(work) = self.permission_owner()
			{
				menu = menu.child(mcp_forms::mcp_button(
					"permission-profiles-read".into(),
					"Refresh permissions".into(),
					false,
					cx,
					move |s, cx| s.update_permission_profiles(work.clone(), None, None, cx),
				));
			}
		}
		menu.into_any_element()
	}
}

async fn read_selection(
	client: &AgentClient,
	work: EntityId,
	wait_for_accepted: bool,
) -> AgentPermissionState {
	let mut state =
		client.permission_profiles(work.clone()).await.unwrap_or(AgentPermissionState::Unavailable);
	for _ in 0..6 {
		if !wait_for_accepted
			|| !matches!(
				state,
				AgentPermissionState::Pending {
					state: decodex_protocol::AgentPermissionOutcome::Reserved
						| decodex_protocol::AgentPermissionOutcome::Queued,
					..
				}
			) {
			break;
		}
		tokio::time::sleep(std::time::Duration::from_millis(100)).await;
		let next = client
			.permission_profiles(work.clone())
			.await
			.unwrap_or(AgentPermissionState::Unavailable);
		if matches!(next, AgentPermissionState::Unavailable) {
			break;
		}
		state = next;
	}
	state
}

#[cfg(test)]
#[path = "agent_permissions_wire_tests.rs"]
mod wire_tests;
