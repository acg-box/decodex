//! Manual task recap presentation. The service owns history and inference.
#[path = "agent_recap_automatic.rs"] mod automatic;
#[path = "agent_recap_request.rs"] mod request;

pub(super) use automatic::Automatic;

use gpui::AnyElement;
use tokio::sync::watch::{self, Sender};

use crate::shell::agent_surface::{
	AgentActionDto, AgentClient, AgentCommandResponse, AgentDispatchStateDto, AgentSnapshotDto,
	AgentSurface, ClientProfile, Context, EntityId, IdempotencyKey, IntoElement, ParentElement,
	Styled, Task, Window, WireText, div, markdown, mcp_forms, px, unique_command,
};
#[cfg(test)]
use crate::shell::agent_surface::{
	AgentSnapshotResult, AgentWorkItemDto, Entity, LoadState, Render,
};
use decodex_protocol::{TaskRecapPhase as Phase, TaskRecapStatus};

#[derive(Default)]
pub(super) struct Panel {
	automatic: bool,
	work: Option<String>,
	state: Option<TaskRecapStatus>,
	cancel: Option<Sender<bool>>,
	task: Option<Task<()>>,
	epoch: u64,
	feedback: String,
}
impl Panel {
	fn busy(&self) -> bool {
		self.task.is_some()
			&& self
				.state
				.as_ref()
				.is_none_or(|s| matches!(s.phase, Phase::Pending | Phase::Cancelling))
	}
}

impl AgentSurface {
	pub(super) fn reset_recap(&mut self) {
		self.recap = Panel { epoch: self.recap.epoch.wrapping_add(1), ..Default::default() };
	}

	pub(super) fn invalidate_recap(&mut self, next: &AgentSnapshotDto) {
		self.invalidate_automatic_recap(next);

		let Some(work) = &self.recap.work else { return };
		let before =
			self.snapshot.as_ref().and_then(|s| s.work_items.iter().find(|w| &w.id == work));
		let after = next.work_items.iter().find(|w| &w.id == work);

		if self.snapshot.as_ref().is_none_or(|s| s.runtime_source != next.runtime_source)
			|| !matches!((before, after), (Some(a), Some(b)) if a.codex_thread_id == b.codex_thread_id && a.active_turn_id == b.active_turn_id)
		{
			self.reset_recap();
		}
	}

	pub(super) fn open_recap(&mut self, work: &str, cx: &mut Context<Self>) {
		if self.recap.work.as_deref() == Some(work)
			&& (self.recap.state.is_some() || self.recap.busy())
		{
			return;
		}

		self.read_recap(work, false, cx);
	}

	fn read_recap(&mut self, work: &str, generate: bool, cx: &mut Context<Self>) {
		self.request_recap(work, generate, false, cx);
	}

	fn request_recap(
		&mut self,
		work: &str,
		generate: bool,
		automatic: bool,
		cx: &mut Context<Self>,
	) {
		if self.selected.as_deref() != Some(work)
			|| self.native_agents.selected.is_some()
			|| self.recap.busy()
			|| (generate && !automatic && self.recap.state.is_none())
		{
			return;
		}

		let Some(profile) = self.profile.clone() else {
			self.recap.work = Some(work.into());
			self.recap.feedback = "Connect to the service to load this recap.".into();

			cx.notify();

			return;
		};

		if !self.command_connection_ready() {
			self.recap.work = Some(work.into());
			self.recap.feedback =
				"Wait for the service connection before loading or generating a recap.".into();

			cx.notify();

			return;
		}

		let Some(item) =
			self.snapshot.as_ref().and_then(|s| s.work_items.iter().find(|w| w.id == work))
		else {
			return;
		};
		let Some(thread) =
			item.codex_thread_id.as_ref().and_then(|s| WireText::new(s.clone()).ok())
		else {
			self.recap.work = Some(work.into());
			self.recap.feedback =
				"A recap is available after this agent starts a conversation.".into();

			cx.notify();

			return;
		};
		let Ok(owner) = EntityId::new(work) else { return };

		self.reset_recap();

		self.recap.work = Some(work.into());
		self.recap.automatic = automatic;
		self.recap.feedback = if generate { "Generating recap…" } else { "Reading recap…" }.into();

		let epoch = self.recap.epoch;
		let (cancel, cancellation) = watch::channel(false);
		let (updates, mut results) = watch::channel(None);

		self.recap.cancel = Some(cancel);

		if std::thread::Builder::new()
			.name("task-recap-io".into())
			.spawn(move || {
				if let Ok(runtime) =
					tokio::runtime::Builder::new_current_thread().enable_all().build()
				{
					runtime.block_on(request::run(
						profile,
						owner,
						thread,
						generate,
						cancellation,
						updates,
					));
				}
			})
			.is_err()
		{
			self.recap.cancel = None;
			self.recap.feedback = "Recap worker could not start. Refresh to try again.".into();

			cx.notify();

			return;
		}

		self.recap.task = Some(cx.spawn(async move |surface, cx| {
			while results.changed().await.is_ok() {
				let result = results.borrow_and_update().clone();

				if surface
					.update(cx, |s, cx| {
						if s.recap.epoch != epoch {
							return;
						}

						if let Some((state, feedback)) = result {
							if state.is_some()
								&& s.recap.cancel.as_ref().is_some_and(|cancel| *cancel.borrow())
							{
								return;
							}

							if let Some(state) = &state {
								s.record_recap_result(state);
							}

							s.recap.state = state;
							s.recap.feedback = feedback;

							cx.notify();
						}
					})
					.is_err()
				{
					break;
				}
			}

			let _ = surface.update(cx, |s, cx| {
				if s.recap.epoch == epoch {
					s.recap.task = None;
					s.recap.cancel = None;

					cx.notify();
				}
			});
		}));

		cx.notify();
	}

	pub(super) fn recap_panel(&self, work: &str, cx: &mut Context<Self>) -> AnyElement {
		if self.native_agents.selected.is_some() {
			return div().into_any_element();
		}

		let owner = work.to_owned();
		let opened = self.recap.work.as_deref() == Some(work);
		let mut panel = div()
			.w_full()
			.text_size(px(12.))
			.line_height(px(18.))
			.flex()
			.flex_col()
			.items_start()
			.gap_2()
			.child(mcp_forms::mcp_button(
				"recap-toggle".into(),
				"Task recap".into(),
				opened,
				cx,
				move |s, cx| {
					if s.recap.work.as_deref() == Some(&owner) {
						s.reset_recap();
						cx.notify();
					} else {
						s.read_recap(&owner, false, cx);
					}
				},
			));

		if !opened {
			return panel.into_any_element();
		}
		if self.recap.state.as_ref().is_none_or(|s| s.phase != Phase::Ready) {
			panel = panel.child(self.recap.feedback.clone());
		}

		if let Some(recap) = self.recap.state.as_ref().and_then(|s| s.recap.as_ref()) {
			panel =
				panel.child(markdown::render_process(recap.summary.as_str(), "task-recap-summary"));

			if let Some(next) = &recap.next_action {
				panel = panel.child(format!("Next: {}", next.as_str()));
			}
		}

		if self.recap.busy() {
			panel = panel.child(mcp_forms::mcp_button(
				"recap-cancel".into(),
				"Cancel recap".into(),
				false,
				cx,
				|s, cx| {
					if let Some(cancel) = &s.recap.cancel {
						let _ = cancel.send(true);
					}

					s.recap.state = None;
					s.recap.feedback = "Cancelling recap…".into();

					cx.notify();
				},
			));
		} else {
			let mut actions = div().flex().gap_2();

			for (id, label, generate) in [
				("recap-refresh", "Refresh recap", false),
				("recap-generate", "Generate recap", true),
			] {
				if generate && self.recap.state.is_none() {
					continue;
				}

				let owner = work.to_owned();

				actions = actions.child(mcp_forms::mcp_button(
					id.into(),
					label.into(),
					false,
					cx,
					move |s, cx| s.read_recap(&owner, generate, cx),
				));
			}

			panel = panel.child(actions);
		}

		panel.into_any_element()
	}
}

#[cfg(test)]
#[path = "agent_recap_tests.rs"]
mod tests;

#[cfg(any(test, feature = "visual-capture"))]
impl AgentSurface {
	pub(super) fn visual_recap(&mut self) {
		let Some(work) = self.selected.clone() else { return };

		if let Some(item) =
			self.snapshot.as_mut().and_then(|s| s.work_items.iter_mut().find(|w| w.id == work))
		{
			item.codex_thread_id = Some("fixture-thread".into());
		}

		self.recap.work = Some(work.clone());
		self.recap.feedback = "Task recap".into();
		self.recap.state = Some(TaskRecapStatus {
   work_id: EntityId::new(work).expect("fixture owner"),
   thread_id: Some(WireText::new("fixture-thread").expect("fixture thread")),
   request_id: Some(WireText::new("fixture-request").expect("fixture request")),
   phase: Phase::Ready,
   recap: Some(decodex_protocol::TaskRecap {
    summary: WireText::new("You asked to complete the release checks. Existing sessions now reopen without another sign-in. Fresh-install verification is still running; the release has not been published.").expect("fixture summary"),
    next_action: Some(WireText::new("Review the fresh-install result before publishing.").expect("fixture next action")),
   }),
  });
	}
}
