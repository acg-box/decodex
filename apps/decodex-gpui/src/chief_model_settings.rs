//! Source-checked native observations with explicit next-message choices kept separate.
use super::{mcp_forms::mcp_button, *};
use decodex_protocol::ChiefModelSettingsResult as State;
#[derive(Default)]
pub(super) struct Panel {
	observations: std::collections::BTreeMap<String, State>,
	work: Option<String>,
	task: Option<Task<()>>,
	read_at: Option<std::time::Instant>,
	epoch: u64,
}
impl ChiefSurface {
	pub(super) fn refresh_composer_model_settings(&mut self, cx: &mut Context<Self>) {
		if self.sending
			|| self.profile.is_none()
			|| self.model_settings.read_at.is_some_and(|at| at.elapsed().as_secs() < 2)
		{
			return;
		}
		if let Some(owner) = self.composer_manager.clone().or_else(|| self.root_id()) {
			self.read_model_settings(&owner, cx);
		}
	}

	pub(super) fn composer_model_label(&self, cx: &Context<Self>) -> String {
		let Some(model) = self.composer_model_value(cx) else { return "Task model".into() };
		if let Some(decodex_protocol::ChiefCapabilitiesResult::Available { models, .. }) =
			self.current_model_catalog(cx)
			&& let Some(entry) = models.iter().find(|entry| entry.model.as_str() == model)
		{
			return entry.name.clone();
		}
		model
	}

	pub(super) fn composer_model_value(&self, cx: &Context<Self>) -> Option<String> {
		let Some(owner) = self.composer_manager.clone().or_else(|| self.root_id()) else {
			return Some(self.model.read(cx).content().into());
		};
		if let Some(model) = self.draft_profiles.execution.choice(&owner).model {
			return Some(model.as_str().into());
		}
		if let Some(State::Available { model: Some(model), .. }) =
			self.model_settings.observations.get(&owner)
		{
			return Some(model.as_str().into());
		}
		None
	}

	pub(super) fn composer_effort_value(&self) -> String {
		let Some(owner) = self.composer_manager.clone().or_else(|| self.root_id()) else {
			return self.effort.as_str().into();
		};
		if let Some(effort) = self.draft_profiles.execution.choice(&owner).reasoning_effort {
			return effort.as_str().into();
		}
		if let Some(State::Available { reasoning_effort: Some(effort), .. }) =
			self.model_settings.observations.get(&owner)
		{
			return effort.as_str().into();
		}
		"Inherited".into()
	}

	fn adopt_composer_observation(
		&mut self,
		work: &str,
		state: &State,
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
		let State::Available { reasoning_effort, .. } = state else { return };
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

	pub(super) fn invalidate_model_settings(&mut self, next: &ChiefSnapshotDto) {
		if self.snapshot.as_ref().and_then(|s| s.runtime_source.as_ref())
			!= next.runtime_source.as_ref()
		{
			self.reset_model_settings();
			return;
		}
		let changed = self.model_settings.observations.keys().chain(self.model_settings.work.iter()).any(|work| {
			let before = self.snapshot.as_ref().and_then(|s| s.work_items.iter().find(|w| &w.id == work));
			let after = next.work_items.iter().find(|w| &w.id == work);
			!matches!((before,after),(Some(a),Some(b)) if a.codex_thread_id==b.codex_thread_id && a.active_turn_id==b.active_turn_id && a.dispatch_state==b.dispatch_state)
		});
		if changed {
			self.reset_model_settings();
		}
	}

	pub(super) fn model_settings_panel(
		&self,
		work: &ChiefWorkItemDto,
		cx: &mut Context<Self>,
	) -> gpui::AnyElement {
		let owner = work.id.clone();
		let mut panel = div().flex().flex_col().gap_2().child(mcp_button(
			"native-model-settings-read".into(),
			"Refresh task model".into(),
			self.model_settings.task.is_some(),
			cx,
			move |s, cx| s.read_model_settings(&owner, cx),
		));
		if self.model_settings.observations.contains_key(&work.id)
			|| self.model_settings.work.as_ref() == Some(&work.id)
		{
			let text = self
				.model_settings
				.observations
				.get(&work.id)
				.map(settings_text)
				.unwrap_or_else(|| "Reading native settings…".into());
			panel = panel
				.child(
					div().debug_selector(|| "native-model-settings-observation".into()).child(text),
				)
				.child(muted(
					"Last read of task settings. Individual turns may use different settings.",
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
		let composer = self.composer_manager.clone().or_else(|| self.root_id());
		self.model_settings
			.observations
			.retain(|key, _| self.selected.as_ref() == Some(key) || composer.as_ref() == Some(key));
		self.model_settings.work = Some(work.into());
		cx.notify();
		let Some(profile) = self.profile.clone() else {
			self.model_settings.observations.insert(work.into(), State::Unavailable);
			cx.notify();
			return;
		};
		let Some(snapshot) = self.snapshot.as_ref() else { return };
		let source = snapshot.runtime_source.clone();
		let Some(thread) = snapshot
			.work_items
			.iter()
			.find(|w| w.id == work)
			.and_then(|w| w.codex_thread_id.clone())
		else {
			return;
		};
		let Ok(work_id) = EntityId::new(work.to_owned()) else { return };
		let generation = self.generation;
		let selection = self.selected.clone();
		let intent_revision = self.draft_profiles.execution.revision();
		let input_at_read = self.model.read(cx).content().to_owned();
		self.model_settings.epoch = self.model_settings.epoch.wrapping_add(1);
		let epoch = self.model_settings.epoch;
		self.model_settings.read_at = Some(std::time::Instant::now());
		let work = work.to_owned();
		let future = cx.background_executor().spawn(async move {
			let runtime =
				tokio::runtime::Builder::new_current_thread().enable_all().build().ok()?;
			runtime.block_on(ChiefClient::new(profile).model_settings(work_id)).ok()
		});
		self.model_settings.task = Some(cx.spawn(async move |surface, cx| {
			let state = future.await.unwrap_or(State::Unavailable);
			let _ = surface.update(cx, |s, cx| {
				if s.model_settings.epoch != epoch { return; }
				if s.generation != generation || s.selected != selection
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
				let state = if matches!(&state,State::Available{thread_id,work_id,..} if thread_id.as_str()!=thread || work_id.as_str()!=work) {State::Unavailable} else {state};
				s.adopt_composer_observation(&work, &state, intent_revision, &input_at_read, cx);
				s.model_settings.observations.insert(work, state);
				s.model_settings.task = None;
				cx.notify();
			});
		}));
		cx.notify();
	}
}
fn settings_text(state: &State) -> String {
	match state {
		State::Available { account_id, model, reasoning_effort, model_provider, .. } => format!(
			"Account: {}\nModel provider: {}\nTask model: {}\nReasoning effort: {}",
			account_id.as_str(),
			model_provider.as_ref().map_or("Not reported", |v| v.as_str()),
			model.as_ref().map_or("Not reported", |v| v.as_str()),
			reasoning_effort.as_ref().map_or("Unset or not reported", |v| v.as_str())
		),
		State::NotReported => "This native server does not report thread model settings.".into(),
		State::Unavailable => "Task settings are unavailable. Refresh to try again.".into(),
	}
}

#[cfg(test)]
#[path = "chief_model_settings_wire_tests.rs"]
mod tests;
