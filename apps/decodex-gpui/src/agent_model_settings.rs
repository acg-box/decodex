//! Source-checked native observations with explicit next-message choices kept separate.
use std::collections::BTreeMap;

#[cfg(test)] use gpui::px;
use gpui::{AnyElement, prelude::FluentBuilder as _};
use tokio::runtime::Builder;

#[cfg(test)] use crate::shell::agent_surface::AgentDispatchStateDto;
#[cfg(test)] use crate::shell::agent_surface::AgentSnapshotResult;
#[cfg(test)] use crate::shell::agent_surface::AgentWorkStatusDto;
#[cfg(test)] use crate::shell::agent_surface::ClientProfile;
#[cfg(test)] use crate::shell::agent_surface::ConversationModel;
#[cfg(test)] use crate::shell::agent_surface::ConversationReasoningEffort;
#[cfg(test)] use crate::shell::agent_surface::Entity;
#[cfg(test)] use crate::shell::agent_surface::Render;
#[cfg(test)] use crate::shell::agent_surface::Window;
#[cfg(test)] use crate::shell::agent_surface::WireText;
use crate::{
	shell::agent_surface::{
		self, AgentClient, AgentSnapshotDto, AgentSurface, AgentWorkItemDto, Context, EntityId,
		InteractiveElement, IntoElement, ParentElement, Styled, Task, mcp_forms,
	},
	ui_feedback::{self, Feedback},
};
use decodex_protocol::{AgentCapabilitiesResult, AgentModelDto, AgentModelSettingsResult};

#[derive(Default)]
pub(super) struct Panel {
	observations: BTreeMap<String, AgentModelSettingsResult>,
	work: Option<String>,
	task: Option<Task<()>>,
	read_at: BTreeMap<String, std::time::Instant>,
	epoch: u64,
}
impl Panel {
	fn observe(&mut self, work: String, state: AgentModelSettingsResult) -> bool {
		// A failed read contains no new model observation. Keep the last native
		// value for this binding; source or thread changes invalidate it separately.
		if state == AgentModelSettingsResult::Unavailable
			&& matches!(
				self.observations.get(&work),
				Some(AgentModelSettingsResult::Available { .. })
			) {
			return false;
		}
		if self.observations.get(&work) == Some(&state) {
			return false;
		}

		self.observations.insert(work, state);

		true
	}
}

impl AgentSurface {
	pub(super) fn refresh_composer_model_settings(&mut self, cx: &mut Context<Self>) {
		if self.is_new_conversation() || self.sending || self.profile.is_none() {
			return;
		}

		if let Some(owner) = self.composer_manager.clone().or_else(|| self.root_id())
			&& self.model_settings.read_at.get(&owner).is_none_or(|at| at.elapsed().as_secs() >= 2)
		{
			self.read_model_settings(&owner, cx);
		}
	}

	pub(super) fn composer_model_label(&self, cx: &Context<Self>) -> String {
		let Some(model) = self.composer_model_value(cx) else {
			let owner = self.composer_manager.clone().or_else(|| self.root_id());

			return if owner
				.as_ref()
				.is_some_and(|owner| self.model_settings.observations.contains_key(owner))
			{
				"Model unavailable"
			} else {
				"Loading model…"
			}
			.into();
		};

		if let Some(AgentCapabilitiesResult::Available { models, .. }) =
			self.current_model_catalog(cx)
			&& let Some(entry) = models.iter().find(|entry| entry.model.as_str() == model)
		{
			return entry.name.clone();
		}

		model
	}

	fn composer_model_settings(&self, cx: &Context<Self>) -> (Option<String>, Option<String>) {
		let (model, effort) = if let Some(owner) =
			self.composer_manager.clone().or_else(|| self.root_id())
		{
			let choice = self.draft_profiles.execution.choice(&owner);
			let (model, effort) = match self.model_settings.observations.get(&owner) {
				Some(AgentModelSettingsResult::Available { model, reasoning_effort, .. }) => (
					model.as_ref().map(|value| value.as_str().to_owned()),
					reasoning_effort.as_ref().map(|value| value.as_str().to_owned()),
				),
				_ => (None, None),
			};

			(
				choice.model.map(|value| value.as_str().to_owned()).or(model),
				choice.reasoning_effort.map(|value| value.as_str().to_owned()).or(effort),
			)
		} else {
			(
				Some(self.model.read(cx).content().to_owned()),
				self.creation_effort().map(|value| value.as_str().to_owned()),
			)
		};
		let effort = effort.or_else(|| {
			let AgentCapabilitiesResult::Available { models, .. } =
				self.current_model_catalog(cx)?
			else {
				return None;
			};

			models
				.iter()
				.find(|entry| Some(entry.model.as_str()) == model.as_deref())?
				.default_effort
				.as_ref()
				.map(|value| value.as_str().to_owned())
		});

		(model, effort)
	}

	pub(super) fn composer_model_value(&self, cx: &Context<Self>) -> Option<String> {
		self.composer_model_settings(cx).0
	}

	pub(super) fn composer_effort_value(&self, cx: &Context<Self>) -> Option<String> {
		self.composer_model_settings(cx).1
	}

	fn adopt_composer_observation(
		&mut self,
		work: &str,
		state: &AgentModelSettingsResult,
		revision: u64,
		input_at_read: &str,
		cx: &mut Context<Self>,
	) {
		if self.composer_manager.clone().or_else(|| self.root_id()).as_deref() != Some(work)
			|| self.draft_profiles.execution.revision() != revision
			|| self.model.read(cx).content() != input_at_read
		{
			return;
		}

		let AgentModelSettingsResult::Available { reasoning_effort, .. } = state else { return };
		let choice = self.draft_profiles.execution.choice(work);

		if choice.reasoning_effort.is_none()
			&& let Some(effort) = reasoning_effort
			&& let Ok(effort) = serde_json::from_value(serde_json::json!(effort.as_str()))
		{
			self.effort = effort;
		}
	}

	pub(super) fn reset_model_settings(&mut self) {
		self.model_settings =
			Panel { epoch: self.model_settings.epoch.wrapping_add(1), ..Default::default() };
	}

	pub(super) fn cancel_model_settings_read(&mut self) {
		self.model_settings.epoch = self.model_settings.epoch.wrapping_add(1);

		if self.model_settings.task.is_some()
			&& let Some(work) = &self.model_settings.work
		{
			self.model_settings.read_at.remove(work);
		}

		self.model_settings.task = None;
		self.model_settings.work = None;
	}

	pub(super) fn invalidate_model_settings(&mut self, next: &AgentSnapshotDto) {
		if self.snapshot.as_ref().and_then(|s| s.runtime_source.as_ref())
			!= next.runtime_source.as_ref()
		{
			self.reset_model_settings();

			return;
		}

		let rebound =
			self.model_settings.observations.keys().chain(self.model_settings.work.iter()).any(
				|work| {
					let before = self
						.snapshot
						.as_ref()
						.and_then(|s| s.work_items.iter().find(|w| &w.id == work));
					let after = next.work_items.iter().find(|w| &w.id == work);

					!matches!((before,after),(Some(a),Some(b)) if a.codex_thread_id==b.codex_thread_id)
				},
			);

		if rebound {
			self.reset_model_settings();

			return;
		}

		let lifecycle_changed = self.model_settings.read_at.keys().any(|work| {
			let before = self.snapshot.as_ref().and_then(|s| s.work_items.iter().find(|w| &w.id == work));
			let after = next.work_items.iter().find(|w| &w.id == work);

			!matches!((before, after), (Some(a), Some(b)) if a.active_turn_id == b.active_turn_id && a.dispatch_state == b.dispatch_state)
		});

		if lifecycle_changed {
			// The same thread keeps its last observation while a fresh read completes.
			self.cancel_model_settings_read();
			self.model_settings.read_at.clear();
		}
	}

	pub(super) fn model_settings_panel(
		&self,
		work: &AgentWorkItemDto,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let owner = work.id.clone();
		let mut panel = gpui::div().flex().flex_col().gap_2().child(mcp_forms::mcp_button(
			"native-model-settings-read".into(),
			"Refresh model".into(),
			self.model_settings.task.is_some()
				&& !self.model_settings.observations.contains_key(&work.id),
			cx,
			move |s, cx| s.read_model_settings(&owner, cx),
		));

		if self.model_settings.observations.contains_key(&work.id)
			|| self.model_settings.work.as_ref() == Some(&work.id)
		{
			let state = self.model_settings.observations.get(&work.id);
			let text = self
				.model_settings
				.observations
				.get(&work.id)
				.map(settings_text)
				.unwrap_or_else(|| "Reading settings…".into());
			let feedback = match state {
				Some(AgentModelSettingsResult::Available { .. }) => None,
				Some(AgentModelSettingsResult::Unavailable) =>
					Some(Feedback::warning(text.clone())),
				_ => Some(Feedback::info(text.clone())),
			};

			panel = panel
				.child(
					gpui::div()
						.debug_selector(|| "native-model-settings-observation".into())
						.when(feedback.is_none(), |d| d.child(text))
						.children(feedback.as_ref().and_then(|feedback| {
							ui_feedback::inline_feedback(
								"model-settings-feedback",
								"Model settings",
								feedback,
							)
						})),
				)
				.child(agent_surface::muted(
					"Last known conversation settings. Individual turns can differ.",
				));
		}

		panel.into_any_element()
	}

	fn read_model_settings(&mut self, work: &str, cx: &mut Context<Self>) {
		if (self.selected.as_deref() != Some(work)
			&& self.composer_manager.clone().or_else(|| self.root_id()).as_deref() != Some(work))
			|| self.model_settings.task.is_some()
		{
			return;
		}

		let initial = !self.model_settings.observations.contains_key(work);

		self.model_settings.work = Some(work.into());

		let binding = self.snapshot.as_ref().and_then(|snapshot| {
			let thread =
				snapshot.work_items.iter().find(|w| w.id == work)?.codex_thread_id.clone()?;

			Some((snapshot.runtime_source.clone(), thread))
		});
		let (Some(profile), Some((source, thread)), Ok(work_id)) =
			(self.profile.clone(), binding, EntityId::new(work.to_owned()))
		else {
			let previous = self
				.model_settings
				.observations
				.insert(work.into(), AgentModelSettingsResult::Unavailable);

			if previous != Some(AgentModelSettingsResult::Unavailable) {
				cx.notify();
			}

			return;
		};
		let selection = self.selected.clone();
		let intent_revision = self.draft_profiles.execution.revision();
		let input_at_read = self.model.read(cx).content().to_owned();

		self.model_settings.epoch = self.model_settings.epoch.wrapping_add(1);

		let epoch = self.model_settings.epoch;

		self.model_settings.read_at.insert(work.into(), std::time::Instant::now());

		let work = work.to_owned();
		let future = cx.background_executor().spawn(async move {
			let runtime = Builder::new_current_thread().enable_all().build().ok()?;

			runtime.block_on(AgentClient::new(profile).model_settings(work_id)).ok()
		});

		self.model_settings.task = Some(cx.spawn(async move |surface, cx| {
			let state = future.await.unwrap_or(AgentModelSettingsResult::Unavailable);
			let _ = surface.update(cx, |s, cx| {
				if s.model_settings.epoch != epoch { return; }
				if s.selected != selection
					|| s.snapshot.as_ref().and_then(|v| v.runtime_source.clone()) != source {
					s.reset_model_settings();
					cx.notify();

					return;
				}

				let bound = s
					.snapshot
					.as_ref()
					.and_then(|v| v.work_items.iter().find(|w| w.id == work))
					.and_then(|w| w.codex_thread_id.as_deref());

				if bound != Some(thread.as_str()) {
					s.reset_model_settings();
					cx.notify();

					return;
				}

				let state = if matches!(&state,AgentModelSettingsResult::Available{thread_id,work_id,..} if thread_id.as_str()!=thread || work_id.as_str()!=work) {AgentModelSettingsResult::Unavailable} else {state};
				let effort = s.effort.clone();

				s.adopt_composer_observation(&work, &state, intent_revision, &input_at_read, cx);

				let changed = s.model_settings.observe(work, state);

				s.model_settings.task = None;

				if changed || effort != s.effort {
					cx.notify();
				}
			});
		}));

		if initial {
			cx.notify();
		}
	}
}

/// Display native specialty only in choices; compact current-model controls keep their name.
pub(super) fn model_choice_label(model: &AgentModelDto) -> String {
	match model.specialty.as_deref() {
		Some("cyber") => format!("{} · Cybersecurity", model.name),
		Some(specialty) => format!("{} · {specialty}", model.name),
		None => model.name.clone(),
	}
}

fn settings_text(state: &AgentModelSettingsResult) -> String {
	match state {
		AgentModelSettingsResult::Available {
			account_id,
			model,
			reasoning_effort,
			model_provider,
			..
		} => format!(
			"Account: {}\nModel provider: {}\nModel: {}\nReasoning effort: {}",
			account_id.as_str(),
			model_provider.as_ref().map_or("Not reported", |v| v.as_str()),
			model.as_ref().map_or("Not reported", |v| v.as_str()),
			reasoning_effort.as_ref().map_or("Unset or not reported", |v| v.as_str())
		),
		AgentModelSettingsResult::NotReported =>
			"Conversation settings unsupported by this Codex version.".into(),
		AgentModelSettingsResult::Unavailable => "Settings unavailable. Refresh to retry.".into(),
	}
}

#[cfg(test)]
#[path = "agent_model_settings_wire_tests.rs"]
mod tests;
