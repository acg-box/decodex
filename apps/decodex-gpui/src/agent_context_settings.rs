//! Choose native context defaults without changing loaded conversations.
use gpui::AnyElement;
#[cfg(test)] use gpui::px;
use tokio::runtime::Builder;

#[cfg(test)] use crate::shell::agent_surface::ClientProfile;
#[cfg(test)] use crate::shell::agent_surface::Entity;
#[cfg(test)] use crate::shell::agent_surface::Render;
#[cfg(test)] use crate::shell::agent_surface::Window;
use crate::{
	shell::agent_surface::{
		self, AgentActionDto, AgentClient, AgentCommandResponse, AgentSnapshotDto, AgentSurface,
		Context, EntityId, IdempotencyKey, IntoElement, ParentElement, Styled, Task, mcp_forms,
	},
	ui_feedback::{self, Feedback},
};
use decodex_protocol::{
	AgentContextFeature, AgentContextPreference, AgentContextSettingsResult, ClientFailure,
};

#[derive(Default)]
pub(super) struct Panel {
	work: Option<String>,
	state: Option<AgentContextSettingsResult>,
	task: Option<Task<()>>,
	epoch: u64,
	feedback: Feedback,
}

impl AgentSurface {
	pub(super) fn invalidate_context_settings(&mut self, next: &AgentSnapshotDto) {
		let Some(work) = &self.context_settings.work else { return };
		let before =
			self.snapshot.as_ref().and_then(|s| s.work_items.iter().find(|w| &w.id == work));
		let after = next.work_items.iter().find(|w| &w.id == work);

		if self.snapshot.as_ref().is_none_or(|s| s.runtime_source != next.runtime_source)
			|| !matches!((before, after), (Some(a), Some(b)) if a.codex_thread_id == b.codex_thread_id)
		{
			self.reset_context_settings();
		}
	}

	pub(super) fn reset_context_settings(&mut self) {
		self.context_settings =
			Panel { epoch: self.context_settings.epoch.wrapping_add(1), ..Default::default() };
	}

	fn update_context_settings(
		&mut self,
		work: &str,
		change: Option<(AgentContextFeature, bool)>,
		cx: &mut Context<Self>,
	) {
		if self.selected.as_deref() != Some(work)
			|| self.context_settings.task.is_some()
			|| self.native_agents.selected.is_some()
		{
			return;
		}

		let Some(profile) = self.profile.clone() else {
			self.context_settings.feedback = Feedback::error("No service connection is available.");

			cx.notify();

			return;
		};
		let Ok(owner) = EntityId::new(work) else { return };
		let action = if let Some((feature, enabled)) = &change {
			let Some(AgentContextSettingsResult::Available {
				work_id,
				review_token,
				preferences,
				..
			}) = &self.context_settings.state
			else {
				return;
			};

			if work_id != &owner || !preferences.iter().any(|p| &p.feature == feature) {
				return;
			}

			Some(AgentActionDto::SetContextPreference {
				work_id: owner.clone(),
				review_token: review_token.clone(),
				feature: feature.clone(),
				enabled: *enabled,
			})
		} else {
			None
		};

		self.context_settings.work = Some(work.into());
		self.context_settings.epoch = self.context_settings.epoch.wrapping_add(1);

		let epoch = self.context_settings.epoch;
		let generation = self.generation;

		self.context_settings.state = None;
		self.context_settings.feedback = Feedback::loading(if change.is_some() {
			"Saving context…"
		} else {
			"Reading context settings…"
		});

		let key =
			IdempotencyKey::new(agent_surface::unique_command()).expect("bounded command identity");
		let query_owner = owner.clone();
		let future = cx.background_executor().spawn(async move {
			let runtime = Builder::new_current_thread().enable_all().build().ok()?;
			let client = AgentClient::new(profile);
			let outcome = action.map(|action| runtime.block_on(client.execute(action, key)));
			let state = runtime
				.block_on(client.context_settings(query_owner))
				.unwrap_or(AgentContextSettingsResult::Unavailable);

			Some((outcome, state))
		});

		self.context_settings.task = Some(cx.spawn(async move |surface, cx| {
			let result = future.await;
			let _ = surface.update(cx, |s, cx| {
				if s.generation != generation
					|| s.context_settings.epoch != epoch
					|| s.selected.as_deref() != Some(owner.as_str())
				{
					return;
				}

				s.context_settings.task = None;

				let (outcome, state) =
					result.unwrap_or((None, AgentContextSettingsResult::Unavailable));

				s.context_settings.feedback = feedback(change.as_ref(), outcome.as_ref(), &state);
				s.context_settings.state = Some(state);

				cx.notify();
			});
		}));

		cx.notify();
	}

	pub(super) fn context_settings_panel(&self, work: &str, cx: &mut Context<Self>) -> AnyElement {
		if self.native_agents.selected.is_some() {
			return gpui::div().into_any_element();
		}

		let owner = work.to_owned();
		let opened = self.context_settings.work.as_deref() == Some(work);
		let panel = gpui::div().flex().flex_col().gap_2().child(mcp_forms::mcp_button(
			"context-settings-toggle".into(),
			"Context defaults".into(),
			opened,
			cx,
			move |s, cx| {
				if s.context_settings.work.as_deref() == Some(&owner) {
					s.reset_context_settings();
					cx.notify();
				} else {
					s.update_context_settings(&owner, None, cx);
				}
			},
		));

		if !opened {
			return panel.into_any_element();
		}

		panel.child(self.context_settings_controls(work, cx)).into_any_element()
	}

	pub(crate) fn native_defaults_settings(&self, cx: &mut Context<Self>) -> AnyElement {
		let Some(work) = self.selected.as_deref().filter(|_| self.native_agents.selected.is_none())
		else {
			return gpui::div()
				.child("Select a connected Agent to read native defaults.")
				.into_any_element();
		};

		self.context_settings_controls(work, cx)
	}

	pub(crate) fn read_native_defaults(&mut self, cx: &mut Context<Self>) {
		if let Some(work) = self.selected.clone() {
			self.update_context_settings(&work, None, cx);
		}
	}

	fn context_settings_controls(&self, work: &str, cx: &mut Context<Self>) -> AnyElement {
		let mut panel = gpui::div().flex().flex_col().gap_2();

		panel = panel
			.child(crate::ui_theme::settings_description("Native defaults apply to new conversations. The selected Agent’s project and managed settings can override saved values. Experimental features depend on Codex support."))
			.children(ui_feedback::inline_feedback("context-settings-feedback", "Context settings", &self.context_settings.feedback));

		if self.context_settings.task.is_some() {
			return panel.into_any_element();
		}

		let owner = work.to_owned();

		panel = panel.child(mcp_forms::mcp_button(
			"context-settings-refresh".into(),
			"Read native settings".into(),
			false,
			cx,
			move |s, cx| s.update_context_settings(&owner, None, cx),
		));

		if self.context_settings.work.as_deref() == Some(work)
			&& let Some(AgentContextSettingsResult::Available { preferences, .. }) =
				&self.context_settings.state
		{
			for preference in preferences {
				let feature = preference.feature.clone();
				let owner = work.to_owned();
				let epoch = self.context_settings.epoch;
				let key = feature_key(&feature);
				let label = feature_label(&feature);
				let detail = format!(
					"{} Effective: {} · Saved: {}.",
					feature_detail(&feature),
					on_off(preference.effective),
					preference.preference.map_or("Native default (off)", on_off)
				);

				panel = panel.child(
					crate::ui_theme::settings_row()
						.child(crate::ui_theme::settings_label(label.clone(), detail))
						.child(
							crate::settings_surface::settings_switch(
								format!("context-choice-{key}"),
								format!("context-knob-{key}"),
								label,
								preference.preference.unwrap_or(preference.effective),
								true,
							)
							.on_change(cx.listener(
								move |s, enabled: &bool, _, cx| {
									if s.context_settings.epoch == epoch {
										s.update_context_settings(
											&owner,
											Some((feature.clone(), *enabled)),
											cx,
										);
									}
								},
							)),
						),
				);
			}

			panel = panel.child(crate::ui_theme::settings_description("Cached MCP catalogs are configured per user-owned server. No valid cache means normal discovery waits; tool calls still need a live connection. Manage plugin and project-only servers in Codex."));
		}

		panel.into_any_element()
	}
}

fn feedback(
	change: Option<&(AgentContextFeature, bool)>,
	outcome: Option<&Result<AgentCommandResponse, ClientFailure>>,
	state: &AgentContextSettingsResult,
) -> Feedback {
	if let Some((feature, enabled)) = change {
		if matches!(outcome, Some(Ok(AgentCommandResponse::Accepted { .. })))
			&& let AgentContextSettingsResult::Available { preferences, .. } = state
			&& let Some(AgentContextPreference { effective, preference: Some(saved), .. }) =
				preferences.iter().find(|p| &p.feature == feature)
			&& saved == enabled
		{
			return if effective == enabled {
				Feedback::success("Saved for new conversations.")
			} else {
				Feedback::info("Saved. Overridden by project or managed settings.")
			};
		}

		return Feedback::warning("Save unconfirmed. Refresh before retrying.");
	}

	if matches!(state, AgentContextSettingsResult::Available { .. }) {
		Feedback::default()
	} else {
		Feedback::warning("Context settings unavailable. Refresh to retry.")
	}
}

fn on_off(enabled: bool) -> &'static str {
	if enabled { "On" } else { "Off" }
}
fn feature_key(feature: &AgentContextFeature) -> String {
	match feature {
		AgentContextFeature::AsyncCompaction => "async".into(),
		AgentContextFeature::RetainClientToolOutputs => "retain".into(),
		AgentContextFeature::McpCatalogStartup { server } => format!("mcp-{}", server.as_str()),
	}
}
fn feature_label(feature: &AgentContextFeature) -> String {
	match feature {
		AgentContextFeature::AsyncCompaction => "Background compaction".into(),
		AgentContextFeature::RetainClientToolOutputs =>
			"Keep delegation instructions after compaction".into(),
		AgentContextFeature::McpCatalogStartup { server } =>
			format!("Cached MCP catalog: {}", server.as_str()),
	}
}
fn feature_detail(feature: &AgentContextFeature) -> &'static str {
	match feature {
		AgentContextFeature::AsyncCompaction =>
			"Summarize before context fills. May use extra quota; availability depends on Codex.",
		AgentContextFeature::RetainClientToolOutputs =>
			"Retain Decodex delegation instructions after compaction, within Codex limits.",
		AgentContextFeature::McpCatalogStartup { .. } =>
			"Allow a valid cached tool catalog while this server connects.",
	}
}

#[cfg(test)]
#[path = "agent_context_settings_wire_tests.rs"]
mod tests;
