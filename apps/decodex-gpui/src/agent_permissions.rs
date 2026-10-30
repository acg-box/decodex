//! Native permission presets for the current Agent and independent creation defaults.
use std::rc::Rc;

#[cfg(test)] use gpui::px;
use gpui::{
	AnyElement,
	prelude::{FluentBuilder as _, InteractiveElement as _, StatefulInteractiveElement as _},
};
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
	},
	ui_feedback::{self, Feedback},
	ui_motion::SmoothControl as _,
	ui_theme,
};
use decodex_protocol::{AgentPermissionState, ClientFailure, PermissionMode};

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
		let request = profile_id.map_or(Request::Read, |profile| Request::Select { profile, mode });
		self.request_permission_profiles(work, request, cx);
	}

	fn request_permission_profiles(
		&mut self,
		work: String,
		request: Request,
		cx: &mut Context<Self>,
	) {
		if self.permission_profiles.task.is_some()
			|| self.selected.as_ref() != Some(&work)
			|| !self.command_connection_ready()
			|| self.native_agents.selected.is_some()
			|| (matches!(request, Request::Select { .. }) && !self.permission_profiles.reviewed)
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
		let saving = matches!(request, Request::Select { .. });
		let action = match request {
			Request::Read => None,
			Request::Select { profile, mode } => {
				let Some(action) = self.permission_selection(&work_id, &thread, profile, mode)
				else {
					return;
				};
				Some(action)
			},
		};
		if saving && self.next_permission_mode().is_some() {
			self.draft_profiles
				.execution
				.change(work.clone(), |choice| choice.permission_mode = None);
			self.save_draft_document(cx);
		}
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
				saving && matches!(outcome, Some(Ok(AgentCommandResponse::Accepted { .. }))),
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
					s.notify_composer_controls(cx);

					return;
				}

				let (outcome, state) = result.unwrap_or((None, AgentPermissionState::Unavailable));

				s.permission_profiles.reviewed =
					matches!(state, AgentPermissionState::Available { .. });
				s.permission_profiles.feedback =
					permission_feedback(outcome.as_ref(), &state, saving);
				s.permission_profiles.state = Some(match state {
					AgentPermissionState::Available { ref work_id, ref thread_id, .. }
						if work_id.as_str() != work || thread_id.as_str() != thread =>
						AgentPermissionState::Unavailable,
					other => other,
				});

				s.notify_composer_controls(cx);
			});
		}));

		self.notify_composer_controls(cx);
	}

	fn begin_permission_read(&mut self, work: &str, saving: bool) -> u64 {
		self.permission_profiles.epoch = self.permission_profiles.epoch.wrapping_add(1);

		let epoch = self.permission_profiles.epoch;

		// Keep this conversation's last observation mounted while reading. The
		// in-flight task already blocks writes; clearing it made the menu flash.
		if self.permission_profiles.work.as_deref() != Some(work) {
			self.permission_profiles.state = None;
			self.permission_profiles.reviewed = false;
		}
		self.permission_profiles.work = Some(work.to_owned());
		self.permission_profiles.feedback = if matches!(
			self.permission_profiles.state,
			Some(AgentPermissionState::Available { .. })
		) {
			Feedback::default()
		} else {
			Feedback::loading(if saving {
				"Updating permissions…"
			} else {
				"Loading permissions…"
			})
		};

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
		if let Some(mode) = self.next_permission_mode() {
			return mode.label().into();
		}
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

	pub(super) fn permission_symbol(&self) -> crate::shell::workspace_symbols::Symbol {
		use crate::shell::workspace_symbols::Symbol;
		let mode = self.next_permission_mode().or_else(|| {
			if self.permission_owner().is_none() {
				Some(self.creation_permission_mode())
			} else if self.permission_profiles.work == self.permission_owner() {
				match &self.permission_profiles.state {
					Some(AgentPermissionState::Available { mode, .. }) => *mode,
					_ => None,
				}
			} else {
				None
			}
		});
		mode.map_or(Symbol::Shield, crate::session_settings::permission_symbol)
	}

	pub(super) fn refresh_composer_permissions(&mut self, force: bool, cx: &mut Context<Self>) {
		let force = force
			&& !matches!(self.permission_profiles.state, Some(AgentPermissionState::NotConnected));
		if let Some(work) = self.permission_owner()
			&& self.native_agents.selected.is_none()
			&& (force || self.permission_profiles.work.as_ref() != Some(&work))
			&& self.permission_profiles.task.is_none()
		{
			self.update_permission_profiles(work, None, None, cx);
		}
	}

	fn choose_permission_mode(&mut self, mode: PermissionMode, cx: &mut Context<Self>) {
		if self.native_agents.selected.is_some() {
			return;
		}
		if let Some(work) = self.permission_owner() {
			let deferred = self.permissions_deferred();
			if deferred {
				self.draft_profiles
					.execution
					.change(work.clone(), |choice| choice.permission_mode = Some(mode));
				self.save_draft_document(cx);
			}
			if !deferred {
				self.update_permission_profiles(
					work,
					Some(WireText::new(mode.profile()).expect("native preset")),
					Some(mode),
					cx,
				);
			}
		} else {
			if self.is_new_conversation() {
				self.workspace.new_conversation_permissions = Some(mode);
			} else {
				self.permission_mode = mode;
			}
			self.save_draft_document(cx);
		}
		self.notify_composer_controls(cx);
	}

	fn next_permission_mode(&self) -> Option<PermissionMode> {
		if self.native_agents.selected.is_some() {
			return None;
		}
		self.permission_owner()
			.and_then(|owner| self.draft_profiles.execution.choice(&owner).permission_mode)
	}

	fn permissions_deferred(&self) -> bool {
		self.native_agents.selected.is_none()
			&& self.permission_owner().is_some()
			&& self.permission_profiles.work == self.permission_owner()
			&& matches!(self.permission_profiles.state, Some(AgentPermissionState::NotConnected))
	}

	pub(super) fn permission_options(&self, cx: &mut Context<Self>) -> AnyElement {
		let creating = self.permission_owner().is_none();
		let deferred = self.permissions_deferred();
		let current = self.next_permission_mode().or_else(|| {
			if creating {
				Some(self.creation_permission_mode())
			} else {
				match &self.permission_profiles.state {
					Some(AgentPermissionState::Available { mode, .. }) => *mode,
					_ => None,
				}
			}
		});
		let available = |mode: PermissionMode| {
			creating
				|| deferred
				|| (self.permission_profiles.task.is_none()
					&& matches!(&self.permission_profiles.state,
			Some(AgentPermissionState::Available { profiles, can_update: true, .. })
			if self.permission_profiles.reviewed && profiles.iter().any(|p| p.id.as_str() == mode.profile() && p.allowed && p.can_select)))
		};
		let mut menu =
			gpui::div().flex().flex_col().child(crate::session_settings::permission_choices(
				current,
				available,
				Self::choose_permission_mode,
				cx,
			));
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
						menu = menu.child(permission_action(
							format!("permission-profile-{index}"),
							id.as_str().into(),
							self.permission_profiles.reviewed
								&& self.permission_profiles.task.is_none(),
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
				&& self.permission_profiles.task.is_none()
				&& matches!(
					self.permission_profiles.state,
					Some(AgentPermissionState::Unavailable | AgentPermissionState::Pending { .. })
				)
				&& let Some(work) = self.permission_owner()
			{
				menu = menu.child(permission_action(
					"permission-profiles-read".into(),
					"Retry".into(),
					true,
					cx,
					move |s, cx| s.update_permission_profiles(work.clone(), None, None, cx),
				));
			}
		}
		menu.into_any_element()
	}
}

fn permission_action(
	id: String,
	label: String,
	enabled: bool,
	cx: &mut Context<AgentSurface>,
	action: impl Fn(&mut AgentSurface, &mut Context<AgentSurface>) + 'static,
) -> AnyElement {
	let action = Rc::new(action);
	let click = action.clone();

	ui_theme::menu_row(id.clone(), enabled)
		.debug_selector(move || id)
		.aria_label(label.clone())
		.when(!enabled, |row| row.opacity(0.4))
		.child(label)
		.on_click(cx.listener(move |s, _, _, cx| {
			if enabled {
				click(s, cx);
			}
		}))
		.on_key_down(cx.listener(move |s, key: &gpui::KeyDownEvent, _, cx| {
			if enabled && !key.is_held && ["enter", "space"].contains(&key.keystroke.key.as_str()) {
				cx.stop_propagation();
				action(s, cx);
			}
		}))
		.smooth()
		.enabled(enabled)
		.into_any_element()
}

enum Request {
	Read,
	Select { profile: WireText, mode: Option<PermissionMode> },
}

fn permission_feedback(
	outcome: Option<&Result<AgentCommandResponse, ClientFailure>>,
	state: &AgentPermissionState,
	saving: bool,
) -> Feedback {
	match outcome {
		Some(Ok(AgentCommandResponse::Accepted { .. }))
			if matches!(
				state,
				AgentPermissionState::Available {
					last_outcome: Some(decodex_protocol::AgentPermissionOutcome::TargetObserved),
					..
				}
			) =>
			Feedback::default(),
		Some(Ok(AgentCommandResponse::Accepted { .. })) =>
			Feedback::info("Selection queued. Check current permissions below."),
		Some(Ok(AgentCommandResponse::Rejected { .. })) =>
			Feedback::error("Permissions were not changed. Try again."),
		Some(_) => Feedback::warning("Selection unconfirmed. No automatic retry."),
		None if saving => Feedback::warning("Permission change is not confirmed."),
		None if matches!(state, AgentPermissionState::Unavailable) =>
			Feedback::error("Couldn't load permissions."),
		None if matches!(state, AgentPermissionState::Unsupported) =>
			Feedback::neutral("Permissions aren't available for this conversation."),
		None => Feedback::default(),
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
