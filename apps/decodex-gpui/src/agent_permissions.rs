//! Saved task permission profiles; current-turn reviewer controls remain separate.
#[cfg(test)] use gpui::px;
use gpui::{AnyElement, Div};
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
		AgentWorkItemDto, Context, EntityId, IdempotencyKey, IntoElement, ParentElement, Styled,
		Task, WireText, mcp_forms,
	},
	ui_feedback::{self, Feedback},
};
use decodex_protocol::{AgentPermissionOutcome, AgentPermissionState};

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
		})
	}

	fn update_permission_profiles(
		&mut self,
		work: String,
		profile_id: Option<WireText>,
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
			let Some(action) = self.permission_selection(&work_id, &thread, profile_id) else {
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
			let state = runtime
				.block_on(client.permission_profiles(work_id))
				.unwrap_or(AgentPermissionState::Unavailable);

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

				s.permission_profiles.reviewed = !saving;
				s.permission_profiles.feedback = match outcome {
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

	pub(super) fn permission_profiles_panel(
		&self,
		work: &AgentWorkItemDto,
		cx: &mut Context<Self>,
	) -> AnyElement {
		if work.codex_thread_id.is_none()
			|| self.native_agents.selected.is_some()
			|| !self.command_connection_ready()
		{
			return gpui::div().into_any_element();
		}

		let owner = work.id.clone();
		let mut panel = self.permission_profiles_header(owner, cx);

		if self.permission_profiles.work.as_ref() != Some(&work.id) {
			return panel.into_any_element();
		}

		panel = panel.children(ui_feedback::inline_feedback(
			"permission-settings-feedback",
			"Task permissions",
			&self.permission_profiles.feedback,
		));

		match &self.permission_profiles.state {
			Some(AgentPermissionState::Available {
				cwd,
				profile_id,
				profiles,
				can_update,
				last_outcome,
				..
			}) => {
				panel = panel
					.child(format!(
						"Configured profile: {}",
						profile_id.as_ref().map_or("Unnamed policy", |id| id.as_str())
					))
					.child(format!("Directory: {}", cwd.as_str()))
					.child("An active step keeps its existing permissions.");

				if let Some(state) = last_outcome {
					panel = panel.child(format!("Last selection: {}", label(*state)));
				}

				if !can_update {
					panel = panel.children(ui_feedback::inline_feedback(
						"permission-settings-read-only",
						"Read-only",
						&Feedback::neutral(
							"Wait for the task to settle before changing permissions.",
						),
					));
				}

				for (index, p) in profiles.iter().enumerate() {
					let description = p
						.description
						.as_ref()
						.map(|d| format!(" — {}", d.as_str()))
						.unwrap_or_default();
					let text = format!("{}{description}", p.id.as_str());

					if !p.allowed {
						panel = panel.child(format!("{text} · Unavailable by policy"));
					} else if profile_id.as_ref() == Some(&p.id) {
						panel = panel.child(format!("{text} · Configured"));
					} else if !p.can_select {
						panel =
							panel.child(format!("{text} · Unavailable in the current task state"));
					} else if *can_update
						&& self.permission_profiles.task.is_none()
						&& self.permission_profiles.reviewed
					{
						let (owner, id) = (work.id.clone(), p.id.clone());

						panel = panel.child(mcp_forms::mcp_button(
							format!("permission-profile-{index}"),
							text,
							false,
							cx,
							move |s, cx| {
								s.update_permission_profiles(owner.clone(), Some(id.clone()), cx)
							},
						));
					} else {
						panel = panel.child(text);
					}
				}
			},
			Some(AgentPermissionState::Pending { profile_id, state }) => {
				panel = panel
					.child(format!("{}: {}", profile_id.as_str(), label(*state)))
					.child("Refresh to check. The selection will not be resent.");
			},
			Some(AgentPermissionState::Unsupported) => {
				panel = panel.children(ui_feedback::inline_feedback(
					"permission-settings-unsupported",
					"Not available",
					&Feedback::info("Permission profiles unsupported by this Codex version."),
				));
			},
			Some(AgentPermissionState::Unavailable)
				if self.permission_profiles.feedback.is_empty() =>
			{
				panel = panel.children(ui_feedback::inline_feedback(
					"permission-settings-unavailable",
					"Permissions unavailable",
					&Feedback::warning("Permissions unavailable. Reconnect and refresh."),
				));
			},
			Some(AgentPermissionState::Unavailable) | None => {},
		}

		panel.into_any_element()
	}

	fn permission_profiles_header(&self, owner: String, cx: &mut Context<Self>) -> Div {
		gpui::div().flex().flex_col().gap_2().child("Task permissions").child(
			mcp_forms::mcp_button(
				"permission-profiles-read".into(),
				"Refresh permissions".into(),
				false,
				cx,
				move |s, cx| s.update_permission_profiles(owner.clone(), None, cx),
			),
		)
	}
}
fn label(state: AgentPermissionOutcome) -> &'static str {
	match state {
		AgentPermissionOutcome::Reserved => "Awaiting confirmation",
		AgentPermissionOutcome::Queued => "Queued · Awaiting confirmation",
		AgentPermissionOutcome::Unknown => "Unconfirmed",
		AgentPermissionOutcome::Rejected => "Rejected",
		AgentPermissionOutcome::TargetObserved => "Current profile matches",
		AgentPermissionOutcome::Superseded => "Replaced by current permissions",
	}
}

#[cfg(test)]
#[path = "agent_permissions_wire_tests.rs"]
mod wire_tests;
