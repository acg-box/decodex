//! Explicit native configured-settings observations, separate from composer selections.
use super::{mcp_forms::mcp_button, *};
use decodex_protocol::ChiefModelSettingsResult as State;
#[derive(Default)]
pub(super) struct Panel {
	state: Option<State>,
	work: Option<String>,
	task: Option<Task<()>>,
	epoch: u64,
}
impl ChiefSurface {
	pub(super) fn reset_model_settings(&mut self) {
		self.model_settings =
			Panel { epoch: self.model_settings.epoch.wrapping_add(1), ..Default::default() };
	}

	pub(super) fn invalidate_model_settings(&mut self, next: &ChiefSnapshotDto) {
		let Some(work) = self.model_settings.work.as_ref() else { return };
		let before =
			self.snapshot.as_ref().and_then(|s| s.work_items.iter().find(|w| &w.id == work));
		let after = next.work_items.iter().find(|w| &w.id == work);
		if !matches!((before,after),(Some(a),Some(b)) if a.codex_thread_id==b.codex_thread_id && a.active_turn_id==b.active_turn_id && a.dispatch_state==b.dispatch_state)
		{
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
			"Read native model settings".into(),
			self.model_settings.task.is_some(),
			cx,
			move |s, cx| s.read_model_settings(&owner, cx),
		));
		if self.model_settings.work.as_ref() == Some(&work.id) {
			let text = self
				.model_settings
				.state
				.as_ref()
				.map(settings_text)
				.unwrap_or_else(|| "Reading native settings…".into());
			panel=panel.child(div().debug_selector(||"native-model-settings-observation".into()).child(text))
    .child(muted("Last read of thread configuration. Refresh after changes. This is not per-turn execution history."));
		}
		panel.into_any_element()
	}

	fn read_model_settings(&mut self, work: &str, cx: &mut Context<Self>) {
		if self.selected.as_deref() != Some(work) || self.model_settings.task.is_some() {
			return;
		}
		self.model_settings.work = Some(work.into());
		self.model_settings.state = Some(State::Unavailable);
		cx.notify();
		let Some(profile) = self.profile.clone() else {
			self.model_settings.state = Some(State::Unavailable);
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
		self.model_settings.epoch = self.model_settings.epoch.wrapping_add(1);
		let epoch = self.model_settings.epoch;
		self.model_settings.state = None;
		let work = work.to_owned();
		let future = cx.background_executor().spawn(async move {
			let runtime =
				tokio::runtime::Builder::new_current_thread().enable_all().build().ok()?;
			runtime.block_on(ChiefClient::new(profile).model_settings(work_id)).ok()
		});
		self.model_settings.task = Some(cx.spawn(async move |surface, cx| {
			let state = future.await.unwrap_or(State::Unavailable);
			let _ = surface.update(cx, |s, cx| {
				if s.generation != generation
					|| s.model_settings.epoch != epoch
					|| s.selected.as_deref() != Some(&work)
					|| s.snapshot.as_ref().and_then(|v| v.runtime_source.clone()) != source
				{
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
				s.model_settings.state = Some(
					if matches!(&state,State::Available{thread_id,..} if thread_id.as_str()!=thread)
					{
						State::Unavailable
					} else {
						state
					},
				);
				s.model_settings.task = None;
				cx.notify();
			});
		}));
		cx.notify();
	}
}
fn settings_text(state: &State) -> String {
	match state {
		State::Available { account_id, model, reasoning_effort, .. } => format!(
			"Account: {}\nConfigured model: {}\nReasoning effort: {}",
			account_id.as_str(),
			model.as_ref().map_or("Not reported", |v| v.as_str()),
			reasoning_effort.as_ref().map_or("Unset or not reported", |v| v.as_str())
		),
		State::NotReported => "This native server does not report thread model settings.".into(),
		State::Unavailable => "Native settings are unavailable, or the task source changed.".into(),
	}
}

#[cfg(test)]
#[path = "chief_model_settings_wire_tests.rs"]
mod tests;
