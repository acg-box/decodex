//! Shared hook consent and readback. Effective trust is distinct from durable write outcomes.
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
use crate::shell::agent_surface::{
	self, AgentActionDto, AgentClient, AgentCommandResponse, AgentSnapshotDto, AgentSurface,
	AgentWorkItemDto, Context, EntityId, IdempotencyKey, IntoElement, ParentElement, Styled, Task,
	WireText, mcp_forms,
};
use decodex_protocol::{AgentHookChange, AgentHookDto, AgentHookSettingsState};

#[derive(Default)]
pub(super) struct Panel {
	state: Option<AgentHookSettingsState>,
	work: Option<String>,
	task: Option<Task<()>>,
	epoch: u64,
	feedback: String,
	reviewed: bool,
}
impl AgentSurface {
	pub(super) fn reset_hook_settings(&mut self) {
		self.hook_settings =
			Panel { epoch: self.hook_settings.epoch.wrapping_add(1), ..Default::default() };
	}

	pub(super) fn invalidate_hook_settings(&mut self, next: &AgentSnapshotDto) {
		let Some(work) = &self.hook_settings.work else {
			return;
		};
		let before =
			self.snapshot.as_ref().and_then(|s| s.work_items.iter().find(|w| &w.id == work));
		let after = next.work_items.iter().find(|w| &w.id == work);

		if !matches!((before,after),(Some(a),Some(b)) if a.codex_thread_id==b.codex_thread_id && a.dispatch_state==b.dispatch_state && a.active_turn_id==b.active_turn_id)
			|| self.snapshot.as_ref().is_none_or(|s| s.runtime_source != next.runtime_source)
		{
			self.reset_hook_settings();
		}
	}

	fn hook_setting_action(
		&self,
		work: &EntityId,
		thread: &str,
		hook_key: WireText,
		change: AgentHookChange,
	) -> Option<AgentActionDto> {
		let AgentHookSettingsState::Available {
			work_id,
			thread_id,
			review_token,
			hooks,
			can_update: true,
			..
		} = self.hook_settings.state.as_ref()?
		else {
			return None;
		};

		if work_id != work
			|| thread_id.as_str() != thread
			|| self.hook_settings.work.as_deref() != Some(work.as_str())
		{
			return None;
		}

		let hook = hooks.iter().find(|h| h.key == hook_key)?;

		if !editable(hook)
			|| match change {
				AgentHookChange::Trust =>
					hook.saved_hash.as_deref() == Some(hook.current_hash.as_str()),
				AgentHookChange::Enabled(value) => hook.saved_enabled == Some(value),
			} {
			return None;
		}

		Some(AgentActionDto::SetHookSetting {
			work_id: work.clone(),
			thread_id: thread_id.clone(),
			review_token: review_token.clone(),
			hook_key,
			change,
		})
	}

	fn update_hook_settings(
		&mut self,
		work: String,
		selection: Option<(WireText, AgentHookChange)>,
		cx: &mut Context<Self>,
	) {
		if self.hook_settings.task.is_some()
			|| self.selected.as_ref() != Some(&work)
			|| !self.command_connection_ready()
			|| self.native_agents.selected.is_some()
			|| (selection.is_some() && !self.hook_settings.reviewed)
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
		let action = if let Some((hook_key, change)) = selection {
			let Some(action) = self.hook_setting_action(&work_id, &thread, hook_key, change) else {
				return;
			};

			Some(action)
		} else {
			None
		};
		let saving = action.is_some();

		self.hook_settings.epoch = self.hook_settings.epoch.wrapping_add(1);

		let epoch = self.hook_settings.epoch;

		self.hook_settings.work = Some(work.clone());
		self.hook_settings.state = None;
		self.hook_settings.reviewed = false;
		self.hook_settings.feedback = if saving {
			"Submitting shared hook setting…"
		} else {
			"Reading native hook settings…"
		}
		.into();

		let key =
			IdempotencyKey::new(agent_surface::unique_command()).expect("bounded command identity");
		let future = cx.background_executor().spawn(async move {
			let runtime = Builder::new_current_thread().enable_all().build().ok()?;
			let client = AgentClient::new(profile);
			let outcome = action.map(|action| runtime.block_on(client.execute(action, key)));
			let state = runtime
				.block_on(client.hook_settings(work_id))
				.unwrap_or(AgentHookSettingsState::Unavailable);

			Some((outcome, state))
		});

		self.hook_settings.task = Some(cx.spawn(async move |surface, cx| {
			let result = future.await;
			let _ = surface.update(cx, |s, cx| {
				if s.hook_settings.epoch != epoch {
					return;
				}

				s.hook_settings.task = None;

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
					s.reset_hook_settings();
					cx.notify();

					return;
				}

				let (outcome, state) =
					result.unwrap_or((None, AgentHookSettingsState::Unavailable));

				s.hook_settings.reviewed = !saving;
				s.hook_settings.feedback = match outcome {
					Some(Ok(AgentCommandResponse::Accepted { .. })) =>
						"Write acknowledged. Read the saved outcome and effective native hook state below.",
					Some(Ok(AgentCommandResponse::Rejected { .. })) =>
						"Hook change was not accepted. Review current settings before choosing again.",
					Some(_) => "Hook change could not be confirmed. It was not retried.",
					None if saving =>
						"Hook change could not be confirmed. Refresh its saved state.",
					None => "These settings are shared by tasks using this native config file.",
				}
				.into();
				s.hook_settings.state = Some(match state {
					AgentHookSettingsState::Available { ref work_id, ref thread_id, .. }
						if work_id.as_str() != work || thread_id.as_str() != thread =>
						AgentHookSettingsState::Unavailable,
					other => other,
				});

				cx.notify();
			});
		}));

		cx.notify();
	}

	pub(super) fn hook_settings_panel(
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
		let mut panel = gpui::div().flex().flex_col().gap_2().child("Shared hooks").child(
			mcp_forms::mcp_button(
				"hook-settings-read".into(),
				"Review / refresh shared hooks".into(),
				false,
				cx,
				move |s, cx| s.update_hook_settings(owner.clone(), None, cx),
			),
		);

		if self.hook_settings.work.as_ref() != Some(&work.id) {
			return panel.into_any_element();
		}

		panel = panel.child(self.hook_settings.feedback.clone());

		match &self.hook_settings.state {
			Some(AgentHookSettingsState::Available {
				config_file,
				hooks,
				notices,
				can_update,
				last_edit,
				..
			}) => {
				panel=panel.child(format!("Shared config: {}",config_file.as_str())).child("Trust approves only the displayed content hash. Enablement is separate. Other tasks using this file are also affected. Native policy and task plugin exclusions still apply.");

				if let Some(edit) = last_edit {
					panel = panel.child(format!(
						"Last shared edit: {} · {} · task {} · account {}",
						edit.outcome,
						edit.hook.as_str(),
						edit.work_id.as_str(),
						edit.account_id.as_str()
					));

					if matches!(edit.outcome.as_str(), "reserved" | "unknown") {
						panel = panel.child(
							"A shared edit is unconfirmed. Refresh to read its result; it will not be resent.",
						);
					}
				}

				for notice in notices {
					panel = panel.child(notice.clone());
				}
				for (index, hook) in hooks.iter().enumerate() {
					panel = panel
						.child(format!(
							"{} · {} · effective {}",
							hook.key.as_str(),
							hook.trust_status,
							if hook.enabled { "enabled" } else { "disabled" }
						))
						.child(format!("Reviewed hash: {}", hook.current_hash.as_str()))
						.child(hook.details.clone());
					panel = panel.child(format!(
						"Saved enablement: {}",
						hook.saved_enabled.map_or("inherited", |v| if v {
							"enabled"
						} else {
							"disabled"
						})
					));

					if *can_update
						&& self.hook_settings.reviewed
						&& self.hook_settings.task.is_none()
						&& editable(hook)
					{
						let mut changes = vec![];

						if matches!(hook.trust_status.as_str(), "untrusted" | "modified")
							&& hook.saved_hash.as_deref() != Some(hook.current_hash.as_str())
						{
							changes.push((AgentHookChange::Trust, "Trust this reviewed content"));
						}

						for enabled in [true, false] {
							if hook.saved_enabled != Some(enabled) {
								changes.push((
									AgentHookChange::Enabled(enabled),
									if enabled { "Save enabled" } else { "Save disabled" },
								));
							}
						}
						for (action_index, (change, label)) in changes.into_iter().enumerate() {
							let owner = work.id.clone();
							let key = hook.key.clone();

							panel = panel.child(mcp_forms::mcp_button(
								format!("hook-setting-{index}-{action_index}"),
								label.into(),
								false,
								cx,
								move |s, cx| {
									s.update_hook_settings(
										owner.clone(),
										Some((key.clone(), change)),
										cx,
									)
								},
							));
						}
					}
				}

				if hooks.is_empty() {
					panel = panel.child("No hooks were reported for this directory.");
				}
			},
			Some(AgentHookSettingsState::Unavailable) =>
				panel = panel.child(
					"Current hook settings are unavailable. Reconnect and refresh to review them.",
				),
			None => {},
		}

		panel.into_any_element()
	}
}
fn editable(hook: &AgentHookDto) -> bool {
	!hook.managed && matches!(hook.trust_status.as_str(), "trusted" | "modified" | "untrusted")
}
#[cfg(test)]
#[path = "agent_hooks_wire_tests.rs"]
mod wire_tests;
