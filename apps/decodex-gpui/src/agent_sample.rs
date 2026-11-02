//! Next-message suggestions use the retained native one-shot sampling interface.
#[path = "agent_sample_request.rs"] mod request;
#[path = "agent_prompt_suggestions.rs"] mod suggestions;

use tokio::sync::watch::{self, Sender};

use crate::shell::agent_surface::{
	AgentSnapshotDto, AgentSurface, Context, EntityId, Task, WireText, unique_command,
};
use decodex_protocol::{AgentSamplePhase, AgentSampleStatus};

const NEXT_PROMPT: &str = "Suggest one useful next message that I could send in this conversation. \
Write it from my perspective, in the language I have been using. Keep it concise and specific to \
the latest conversation. Return only the suggested message, without quotes, a heading, or an \
explanation. Do not execute the task or call tools.";

#[derive(Default)]
pub(super) struct Panel {
	work: Option<String>,
	thread: Option<String>,
	state: Option<AgentSampleStatus>,
	closed: Option<Sender<bool>>,
	task: Option<Task<()>>,
	epoch: u64,
	feedback: String,
	automatic: suggestions::Automatic,
}
impl Panel {
	fn busy(&self) -> bool {
		self.task.is_some()
			&& self.state.as_ref().is_none_or(|s| {
				matches!(s.phase, AgentSamplePhase::Pending | AgentSamplePhase::Cancelling)
			})
	}
}

impl AgentSurface {
	pub(super) fn can_open_sample(&self) -> bool {
		self.native_agents.selected.is_none()
			&& self
				.snapshot
				.as_ref()
				.and_then(|s| s.work_items.iter().find(|w| Some(&w.id) == self.selected.as_ref()))
				.is_some_and(|w| w.parent_goal_id.is_none() && w.codex_thread_id.is_some())
	}

	pub(super) fn reset_sample(&mut self) {
		self.sample = Panel {
			epoch: self.sample.epoch.wrapping_add(1),
			automatic: std::mem::take(&mut self.sample.automatic),
			..Default::default()
		};
	}

	pub(super) fn invalidate_sample(&mut self, next: &AgentSnapshotDto) {
		let Some(work) = &self.sample.work else { return };
		let before =
			self.snapshot.as_ref().and_then(|s| s.work_items.iter().find(|w| &w.id == work));
		let after = next.work_items.iter().find(|w| &w.id == work);

		if self.snapshot.as_ref().is_none_or(|s| s.runtime_source != next.runtime_source)
			|| !matches!((before,after),(Some(a),Some(b)) if a.codex_thread_id == b.codex_thread_id && a.active_turn_id == b.active_turn_id)
		{
			self.reset_sample();
		}
	}

	fn open_prompt_suggestion(&mut self, cx: &mut Context<Self>) {
		let Some(work) = self.selected.clone() else { return };
		if self.sample.busy() || self.sample.work.as_ref() == Some(&work) {
			return;
		}
		let Some(thread) = self
			.snapshot
			.as_ref()
			.and_then(|snapshot| snapshot.work_items.iter().find(|item| item.id == work))
			.filter(|item| item.parent_goal_id.is_none() && self.native_agents.selected.is_none())
			.and_then(|item| item.codex_thread_id.clone())
		else {
			return;
		};
		self.reset_sample();
		self.sample.work = Some(work);
		self.sample.thread = Some(thread);
		self.sample.feedback = "Reconnect to generate a suggestion.".into();
		self.run_sample_request(cx);
		cx.notify();
	}

	fn suggested_prompt(&self) -> Option<&str> {
		let state = self.sample.state.as_ref()?;
		if state.phase != AgentSamplePhase::Ready
			|| state.prompt.as_ref().is_none_or(|prompt| prompt.as_str() != NEXT_PROMPT)
		{
			return None;
		}
		state.answer.as_ref()?.text.as_deref().filter(|text| !text.trim().is_empty())
	}

	pub(super) fn prompt_suggestion(&self, cx: &Context<Self>) -> Option<&str> {
		let available = self.can_open_sample()
			&& self.sample.work == self.selected
			&& self
				.composer_manager
				.as_ref()
				.is_none_or(|owner| Some(owner) == self.selected.as_ref())
			&& self.draft_owner_available()
			&& self.async_question_composer().is_none()
			&& !self.voice_on_current_conversation()
			&& self.dictation.is_none()
			&& !self.sending
			&& !self.uncertain
			&& self.composer.read(cx).content().is_empty()
			&& !self.composer.read(cx).is_composing()
			&& self.snapshot.as_ref().is_some_and(|snapshot| {
				snapshot.work_items.iter().any(|work| {
					Some(&work.id) == self.selected.as_ref()
						&& work.codex_thread_id == self.sample.thread
						&& work.active_turn_id.is_none()
				})
			});

		available.then(|| self.suggested_prompt()).flatten()
	}

	pub(super) fn accept_prompt_suggestion(&mut self, cx: &mut Context<Self>) {
		let Some(text) = self.prompt_suggestion(cx).map(str::to_owned) else { return };
		if self.composer.update(cx, |input, cx| input.append_recovered_text(&text, cx)) {
			self.remember_draft_document(cx);
			self.reset_sample();
			cx.notify();
		}
	}

	fn run_sample_request(&mut self, cx: &mut Context<Self>) {
		if self.sample.busy() || !self.command_connection_ready() {
			return;
		}

		let Some(profile) = self.profile.clone() else { return };
		let (Some(work), Some(thread)) = (&self.sample.work, &self.sample.thread) else { return };

		if self.selected.as_ref() != Some(work) {
			return;
		}

		let (Ok(work), Ok(thread)) = (EntityId::new(work), WireText::new(thread)) else { return };
		self.sample.epoch = self.sample.epoch.wrapping_add(1);
		self.sample.state = None;
		self.sample.feedback = "Generating suggestion…".into();

		let epoch = self.sample.epoch;
		let (closed, closure) = watch::channel(false);
		let (updates, mut results) = watch::channel(None);

		self.sample.closed = Some(closed);

		if std::thread::Builder::new()
			.name("prompt-suggestion-io".into())
			.spawn(move || {
				if let Ok(runtime) =
					tokio::runtime::Builder::new_current_thread().enable_all().build()
				{
					runtime.block_on(request::run(
						profile,
						work,
						thread,
						WireText::new(NEXT_PROMPT).expect("bounded suggestion prompt"),
						closure,
						updates,
					));
				}
			})
			.is_err()
		{
			self.sample.closed = None;
			self.sample.feedback = "Could not generate a suggestion.".into();

			cx.notify();

			return;
		}

		self.sample.task = Some(cx.spawn(async move |surface, cx| {
			while results.changed().await.is_ok() {
				let update = results.borrow_and_update().clone();

				if surface
					.update(cx, |s, cx| {
						if s.sample.epoch == epoch
							&& let Some((state, feedback)) = update
							&& (s.sample.state != state || s.sample.feedback != feedback)
						{
							s.sample.state = state;
							s.sample.feedback = feedback;

							cx.notify();
						}
					})
					.is_err()
				{
					return;
				}
			}

			let _ = surface.update(cx, |s, cx| {
				if s.sample.epoch == epoch {
					s.sample.task = None;
					s.sample.closed = None;

					cx.notify();
				}
			});
		}));

		cx.notify();
	}
}

#[cfg(test)]
#[path = "agent_sample_tests.rs"]
mod tests;
