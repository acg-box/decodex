//! Explicit one-shot sampling with an editor separate from the conversation draft.
#[path = "agent_sample_request.rs"] mod request;
#[path = "agent_prompt_suggestions.rs"] mod suggestions;

use gpui::{AnyElement, AppContext as _, StatefulInteractiveElement as _};
use tokio::sync::watch::{self, Sender};

use crate::{
	shell::agent_surface::{
		AgentSnapshotDto, AgentSurface, ComposerInput, Context, Entity, EntityId,
		InteractiveElement, IntoElement, ParentElement, Styled, SubmitComposer, Task, WireText,
		markdown, mcp_forms, unique_command,
	},
	ui_theme,
};
use decodex_protocol::{AgentSamplePhase, AgentSampleStatus, MAX_AGENT_SAMPLE_PROMPT_BYTES};

const NEXT_PROMPT: &str = "Suggest one useful next message that I could send in this conversation. \
Write it from my perspective, in the language I have been using. Keep it concise and specific to \
the latest conversation. Return only the suggested message, without quotes, a heading, or an \
explanation. Do not execute the task or call tools.";

#[derive(Default)]
pub(super) struct Panel {
	work: Option<String>,
	thread: Option<String>,
	input: Option<Entity<ComposerInput>>,
	state: Option<AgentSampleStatus>,
	closed: Option<Sender<bool>>,
	task: Option<Task<()>>,
	epoch: u64,
	feedback: String,
	suggestion: bool,
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

	pub(super) fn open_sample(&mut self, cx: &mut Context<Self>) {
		self.open_sample_panel(false, true, cx);
	}

	fn open_sample_panel(&mut self, suggestion: bool, show_latest: bool, cx: &mut Context<Self>) {
		let Some(work) = self.selected.clone() else { return };

		if self.sample.busy()
			|| (self.sample.work.as_ref() == Some(&work) && self.sample.suggestion == suggestion)
		{
			return;
		}

		let Some(item) =
			self.snapshot.as_ref().and_then(|s| s.work_items.iter().find(|w| w.id == work))
		else {
			return;
		};
		let Some(thread) = item
			.codex_thread_id
			.clone()
			.filter(|_| item.parent_goal_id.is_none() && self.native_agents.selected.is_none())
		else {
			return;
		};

		self.reset_sample();

		self.sample.work = Some(work);
		self.sample.thread = Some(thread);
		self.sample.suggestion = suggestion;
		self.sample.feedback = "Reconnect to generate an answer.".into();
		self.sample.input = (!suggestion).then(|| {
			cx.new(|cx| {
				ComposerInput::with_placeholder(
					41,
					"Ask for another perspective…",
					"Separate question",
					cx,
				)
			})
		});

		self.request_sample(suggestion, cx);
		if show_latest {
			self.jump_to_latest(cx);
		}
		cx.notify();
	}

	fn request_sample(&mut self, generate: bool, cx: &mut Context<Self>) {
		if self.sample.busy() || !self.command_connection_ready() {
			return;
		}

		let operation = if generate {
			let text = if self.sample.suggestion {
				NEXT_PROMPT.to_owned()
			} else {
				self.sample
					.input
					.as_ref()
					.map(|input| input.read(cx).content().to_owned())
					.unwrap_or_default()
			};

			if text.trim().is_empty() {
				self.sample.feedback = "Enter a question.".into();

				cx.notify();

				return;
			}
			if text.len() > MAX_AGENT_SAMPLE_PROMPT_BYTES {
				self.sample.feedback = format!(
					"Question exceeds {MAX_AGENT_SAMPLE_PROMPT_BYTES} bytes. Shorten it and try again."
				);

				cx.notify();

				return;
			}

			request::Operation::Generate(WireText::new(text).expect("bounded separate question"))
		} else {
			request::Operation::Read
		};

		self.run_sample_request(operation, cx);
	}

	fn suggested_prompt(&self) -> Option<&str> {
		let state = self.sample.state.as_ref()?;
		if !self.sample.suggestion
			|| state.phase != AgentSamplePhase::Ready
			|| state.prompt.as_ref().is_none_or(|prompt| prompt.as_str() != NEXT_PROMPT)
		{
			return None;
		}
		state.answer.as_ref()?.text.as_deref().filter(|text| !text.trim().is_empty())
	}

	pub(super) fn can_accept_prompt_suggestion(&self, cx: &Context<Self>) -> bool {
		self.suggested_prompt().is_some()
			&& self.can_open_sample()
			&& self.sample.work == self.selected
			&& self
				.composer_manager
				.as_ref()
				.is_none_or(|owner| Some(owner) == self.selected.as_ref())
			&& self.draft_owner_available()
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
			})
	}

	pub(super) fn accept_prompt_suggestion(&mut self, cx: &mut Context<Self>) {
		if !self.can_accept_prompt_suggestion(cx) {
			return;
		}
		let text = self.suggested_prompt().expect("current suggestion").to_owned();
		if self.composer.update(cx, |input, cx| input.append_recovered_text(&text, cx)) {
			self.remember_draft_document(cx);
			self.reset_sample();
			cx.notify();
		}
	}

	fn cancel_sample(&mut self, cx: &mut Context<Self>) {
		let Some(state) =
			self.sample.state.as_ref().filter(|s| s.phase == AgentSamplePhase::Pending)
		else {
			return;
		};
		let Some(id) = state.request_id.clone() else { return };

		self.run_sample_request(request::Operation::Cancel(id), cx);
	}

	fn run_sample_request(&mut self, operation: request::Operation, cx: &mut Context<Self>) {
		if !self.command_connection_ready() {
			return;
		}

		let Some(profile) = self.profile.clone() else { return };
		let (Some(work), Some(thread)) = (&self.sample.work, &self.sample.thread) else { return };

		if self.selected.as_ref() != Some(work) {
			return;
		}

		let (Ok(work), Ok(thread)) = (EntityId::new(work), WireText::new(thread)) else { return };
		let feedback = match &operation {
			request::Operation::Read => "Checking answer status…",
			request::Operation::Generate(_) if self.sample.suggestion => "Generating suggestion…",
			request::Operation::Generate(_) => "Asking separately…",
			request::Operation::Cancel(_) => "Requesting cancellation…",
		};

		self.sample.epoch = self.sample.epoch.wrapping_add(1);
		self.sample.state = None;
		self.sample.feedback = feedback.into();

		let epoch = self.sample.epoch;
		let (closed, closure) = watch::channel(false);
		let (updates, mut results) = watch::channel(None);

		self.sample.closed = Some(closed);

		if std::thread::Builder::new()
			.name("separate-answer-io".into())
			.spawn(move || {
				if let Ok(runtime) =
					tokio::runtime::Builder::new_current_thread().enable_all().build()
				{
					runtime
						.block_on(request::run(profile, work, thread, operation, closure, updates));
				}
			})
			.is_err()
		{
			self.sample.closed = None;
			self.sample.feedback = "Could not start separate answer.".into();

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

	pub(super) fn sample_panel(&self, work: &str, cx: &mut Context<Self>) -> AnyElement {
		if self.sample.work.as_deref() != Some(work) {
			return gpui::div().into_any_element();
		}

		let mut panel = gpui::div().id("sample-panel").debug_selector(|| "sample-panel".into()).w_full().min_w_0().flex().flex_col().gap_2().p_3().rounded_lg().bg(ui_theme::rgba(0xffffff06))
			.child(if self.sample.suggestion { "Suggested next message" } else { "Ask separately" })
			.child(if self.sample.suggestion {
				"Uses this conversation’s model and quota. Review the suggestion, then use it in an empty draft. Nothing is sent automatically."
			} else { "Uses this conversation’s model and quota, without tools. The original conversation stays unchanged." })
			.child(self.sample.feedback.clone());

		if !self.sample.suggestion
			&& let Some(prompt) = self.sample.state.as_ref().and_then(|s| s.prompt.as_ref())
		{
			panel = panel.child(format!("Question: {}", prompt.as_str()));
		}
		if let Some(answer) = self
			.sample
			.state
			.as_ref()
			.filter(|s| {
				!self.sample.suggestion
					|| s.prompt.as_ref().is_some_and(|p| p.as_str() == NEXT_PROMPT)
			})
			.and_then(|s| s.answer.as_ref())
		{
			if let Some(text) = &answer.text {
				panel = panel
					.child(
						gpui::div()
							.id("sample-answer")
							.max_h(gpui::px(320.))
							.overflow_y_scroll()
							.child(markdown::render_process(text, "separate-answer")),
					)
					.child(markdown::copy_button(
						"sample-copy",
						"Copy separate answer",
						text.clone(),
					));
			}
			if let (Some(input), Some(output)) = (answer.input_tokens, answer.output_tokens) {
				panel = panel.child(format!("{input} input tokens · {output} output tokens"));
			}
		}
		if self.can_accept_prompt_suggestion(cx) {
			panel = panel.child("Press Tab in the message field to use this suggestion.");
		} else if self.suggested_prompt().is_some() && !self.composer.read(cx).content().is_empty()
		{
			panel = panel.child("Clear the current draft to use this suggestion, or copy it.");
		}

		if !self.sample.busy() {
			if let Some(input) = &self.sample.input {
				panel = panel.child(
					gpui::div()
						.id("sample-question")
						.debug_selector(|| "sample-question".into())
						.on_action(cx.listener(|s, _: &SubmitComposer, _, cx| {
							cx.stop_propagation();
							s.request_sample(true, cx);
						}))
						.child(input.clone()),
				);
			}

			if self.command_connection_ready() {
				panel = panel
					.child(mcp_forms::mcp_button(
						"sample-ask".into(),
						if self.sample.suggestion { "Generate suggestion" } else { "Ask" }.into(),
						false,
						cx,
						|s, cx| s.request_sample(true, cx),
					))
					.child(mcp_forms::mcp_button(
						"sample-refresh".into(),
						"Refresh status".into(),
						false,
						cx,
						|s, cx| s.request_sample(false, cx),
					));
			}
		}
		if self.command_connection_ready()
			&& self
				.sample
				.state
				.as_ref()
				.is_some_and(|s| s.phase == AgentSamplePhase::Pending && s.request_id.is_some())
		{
			panel = panel.child(mcp_forms::mcp_button(
				"sample-cancel".into(),
				"Cancel answer".into(),
				false,
				cx,
				|s, cx| s.cancel_sample(cx),
			));
		}

		panel
			.child(mcp_forms::mcp_button(
				"sample-close".into(),
				"Close".into(),
				false,
				cx,
				|s, cx| {
					s.reset_sample();
					cx.notify();
				},
			))
			.into_any_element()
	}
}

#[cfg(test)]
#[path = "agent_sample_tests.rs"]
mod tests;
