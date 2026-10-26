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
		let action = if let Some((feature, enabled)) = change {
			let Some(AgentContextSettingsResult::Available {
				work_id,
				review_token,
				preferences,
				..
			}) = &self.context_settings.state
			else {
				return;
			};

			if work_id != &owner || !preferences.iter().any(|p| p.feature == feature) {
				return;
			}

			Some(AgentActionDto::SetContextPreference {
				work_id: owner.clone(),
				review_token: review_token.clone(),
				feature,
				enabled,
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

				s.context_settings.feedback = feedback(change, outcome.as_ref(), &state);
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
		let mut panel = gpui::div().flex().flex_col().gap_2().child(mcp_forms::mcp_button(
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

		panel = panel
			.child("Uses the same native config.toml as manual edits. Applies to new conversations. Project or managed settings can override these defaults; loaded conversations keep their settings. These experimental features remain subject to native runtime eligibility and capacity limits.")
			.children(ui_feedback::inline_feedback("context-settings-feedback", "Context settings", &self.context_settings.feedback));

		if self.context_settings.task.is_some() {
			return panel.into_any_element();
		}

		let owner = work.to_owned();

		panel = panel.child(mcp_forms::mcp_button(
			"context-settings-refresh".into(),
			"Refresh context settings".into(),
			false,
			cx,
			move |s, cx| s.update_context_settings(&owner, None, cx),
		));

		if let Some(AgentContextSettingsResult::Available { preferences, .. }) =
			&self.context_settings.state
		{
			for preference in preferences {
				panel = panel
					.child(feature_label(preference.feature))
					.child(feature_detail(preference.feature))
					.child(format!(
						"New conversation default: {} · Saved user value: {}",
						on_off(preference.effective),
						preference.preference.map_or("Native default (off)", on_off)
					));

				for enabled in [false, true] {
					let owner = work.to_owned();
					let feature = preference.feature;
					let epoch = self.context_settings.epoch;

					panel = panel.child(mcp_forms::mcp_button(
						format!("context-choice-{}-{enabled}", feature_key(feature)),
						on_off(enabled).into(),
						preference.preference == Some(enabled),
						cx,
						move |s, cx| {
							if s.context_settings.epoch == epoch {
								s.update_context_settings(&owner, Some((feature, enabled)), cx);
							}
						},
					));
				}
			}
		}

		panel.into_any_element()
	}
}

fn feedback(
	change: Option<(AgentContextFeature, bool)>,
	outcome: Option<&Result<AgentCommandResponse, ClientFailure>>,
	state: &AgentContextSettingsResult,
) -> Feedback {
	if let Some((feature, enabled)) = change {
		if matches!(outcome, Some(Ok(AgentCommandResponse::Accepted { .. })))
			&& let AgentContextSettingsResult::Available { preferences, .. } = state
			&& let Some(AgentContextPreference { effective, preference: Some(saved), .. }) =
				preferences.iter().find(|p| p.feature == feature)
			&& *saved == enabled
		{
			return if *effective == enabled {
				Feedback::success("Context default saved for new conversations.")
			} else {
				Feedback::info(
					"Saved. Higher-priority configuration selects a different default for this project.",
				)
			};
		}

		return Feedback::warning(
			"The save could not be confirmed. Review the refreshed settings before trying again.",
		);
	}

	if matches!(state, AgentContextSettingsResult::Available { .. }) {
		Feedback::default()
	} else {
		Feedback::warning("Context settings are unavailable. Refresh to try again.")
	}
}

fn on_off(enabled: bool) -> &'static str {
	if enabled { "On" } else { "Off" }
}
fn feature_key(feature: AgentContextFeature) -> &'static str {
	match feature {
		AgentContextFeature::AsyncCompaction => "async",
		AgentContextFeature::RetainClientToolOutputs => "retain",
	}
}
fn feature_label(feature: AgentContextFeature) -> &'static str {
	match feature {
		AgentContextFeature::AsyncCompaction => "Background compaction",
		AgentContextFeature::RetainClientToolOutputs =>
			"Keep delegation instructions after compaction",
	}
}
fn feature_detail(feature: AgentContextFeature) -> &'static str {
	match feature {
		AgentContextFeature::AsyncCompaction =>
			"Prepare a summary before context fills. Can add requests; native eligibility and thresholds apply.",
		AgentContextFeature::RetainClientToolOutputs =>
			"Ask Codex to retain Decodex delegation instructions within its budget. Ordinary tool results, wakes and retries are not selected.",
	}
}

#[cfg(test)]
#[path = "agent_context_settings_wire_tests.rs"]
mod tests;
