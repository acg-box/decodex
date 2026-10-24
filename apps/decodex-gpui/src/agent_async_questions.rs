//! Questions share the input surface while retaining a separate answer draft.
use crate::ui_feedback::Feedback;
use std::{cell::Cell, rc::Rc};

use gpui::{AnyElement, AppContext as _, Bounds, KeyDownEvent, Point, prelude::FluentBuilder as _};

use crate::{
	shell::agent_surface::{
		self, AgentActionDto, AgentHistoryResult, AgentSurface, AgentWorkItemDto, Context,
		EntityId, InteractiveElement, IntoElement, ParentElement, Role, SharedString,
		StatefulInteractiveElement, Styled, Window,
		composer::ComposerTip,
		selectable_text::SelectableText,
		workspace_symbols::{self, Symbol},
	},
	ui_motion::{self, SmoothControl as _},
	ui_theme,
};
use decodex_protocol::{
	AgentAsyncQuestionDto, DesktopQuestionDraft, HistoryText, IdempotencyKey, WireText,
};

#[derive(Clone, PartialEq, Eq)]
struct QuestionTarget {
	work: String,
	thread: String,
	question: String,
}

pub(super) struct Editor {
	target: QuestionTarget,
	profile: Option<agent_surface::ClientProfile>,
	input: agent_surface::Entity<crate::composer_input::ComposerInput>,
}

pub(super) struct Submission {
	target: QuestionTarget,
	key: IdempotencyKey,
}

#[derive(Clone)]
enum QuestionAction {
	Send { answer: String, visible: Rc<Cell<bool>> },
	Skip,
}

impl AgentSurface {
	fn composer_question_target(&self) -> Option<QuestionTarget> {
		if self.native_agents.selected.is_some() {
			return None;
		}
		let (id, AgentHistoryResult::Available { questions, questions_recovering: false, .. }) =
			self.history.as_ref()?
		else {
			return None;
		};
		let work = self.conversation_work()?;
		if id != &work.id {
			return None;
		}
		let thread = work.codex_thread_id?;
		questions.iter().find_map(|question| {
			let target = QuestionTarget {
				work: work.id.clone(),
				thread: thread.clone(),
				question: question.id.clone(),
			};
			(!self.question_submitted(&target)).then_some(target)
		})
	}

	fn question_submitted(&self, target: &QuestionTarget) -> bool {
		self.async_question_submissions.iter().any(|submission| &submission.target == target)
	}

	pub(super) fn begin_question_submission(
		&mut self,
		action: &AgentActionDto,
		key: &IdempotencyKey,
		cx: &Context<Self>,
	) {
		let (work, question) = match action {
			AgentActionDto::AnswerQuestion { work_id, question_id, .. }
			| AgentActionDto::SkipQuestion { work_id, question_id, .. } => (work_id, question_id),
			_ => return,
		};
		let Some(thread) = self
			.snapshot
			.as_ref()
			.and_then(|s| s.work_items.iter().find(|w| w.id == work.as_str()))
			.and_then(|w| w.codex_thread_id.clone())
		else {
			return;
		};
		let target = QuestionTarget {
			work: work.as_str().into(),
			thread,
			question: question.as_str().into(),
		};
		if let Some(editor) = self.async_question_editor.as_ref().filter(|e| e.target == target) {
			let answer = editor.input.read(cx).content().to_owned();
			self.store_question_answer(&target, &answer);
		}
		// Close on submission, before transport and history refresh can race the
		// next streamed response. Keep the answer draft until history confirms it.
		self.async_question_submissions.push(Submission { target, key: key.clone() });
	}

	pub(super) fn restore_question_submission(&mut self, key: Option<&IdempotencyKey>) {
		self.async_question_submissions.retain(|submission| Some(&submission.key) != key);
	}

	pub(super) fn async_question_composer(
		&self,
	) -> Option<&agent_surface::Entity<crate::composer_input::ComposerInput>> {
		let editor = self.async_question_editor.as_ref()?;
		(self.composer_question_target().as_ref() == Some(&editor.target)
			&& editor.profile == self.profile)
			.then_some(&editor.input)
	}

	pub(super) fn sync_question_editor(&mut self, cx: &mut Context<Self>) {
		let target = self.composer_question_target();
		if self.async_question_editor.as_ref().map(|e| &e.target) == target.as_ref()
			&& self.async_question_editor.as_ref().is_none_or(|e| e.profile == self.profile)
		{
			return;
		}
		self.async_question_editor = None;
		let Some(target) = target else {
			return;
		};
		let saved = self
			.restored_question_drafts
			.iter()
			.find(|d| {
				d.work_id.as_str() == target.work
					&& d.thread_id.as_str() == target.thread
					&& d.question_id.as_str() == target.question
			})
			.and_then(saved_custom_answer)
			.unwrap_or_default()
			.to_owned();
		let input = cx.new(|cx| {
			crate::composer_input::ComposerInput::message(
				36,
				"Type your own answer…",
				"Question answer",
				cx,
			)
		});
		input.update(cx, |input, cx| input.set_content(&saved, cx));
		let saved_target = target.clone();
		let profile = self.profile.clone();
		cx.subscribe(&input, move |surface, input, event, cx| {
			if *event != crate::composer_input::ComposerEvent::Changed || surface.profile != profile
			{
				return;
			}
			let answer = input.read(cx).content().to_owned();
			surface.store_question_answer(&saved_target, &answer);
			surface.save_draft_document(cx);
			cx.notify();
		})
		.detach();
		self.async_question_editor = Some(Editor { target, profile: self.profile.clone(), input });
	}

	fn store_question_answer(&mut self, target: &QuestionTarget, text: &str) {
		self.restored_question_drafts.retain(|d| {
			d.work_id.as_str() != target.work
				|| d.thread_id.as_str() != target.thread
				|| d.question_id.as_str() != target.question
		});
		if text.is_empty() {
			return;
		}
		let (Ok(work_id), Ok(thread_id), Ok(question_id)) = (
			EntityId::new(&target.work),
			WireText::new(&target.thread),
			WireText::new(&target.question),
		) else {
			return;
		};
		self.restored_question_drafts.push(DesktopQuestionDraft {
			work_id,
			thread_id,
			question_id,
			text: text.into(),
			custom: None,
			selected: None,
			collapsed: false,
		});
	}

	pub(super) fn submit_question_answer(&mut self, cx: &mut Context<Self>) -> bool {
		self.sync_question_editor(cx);
		let Some(editor) = &self.async_question_editor else {
			return false;
		};
		let target = editor.target.clone();
		let answer = editor.input.read(cx).content().trim().to_owned();
		if answer.is_empty() {
			return true;
		}
		let Some(source) = self.pending_async_question(&target) else {
			return true;
		};
		if let Err(message) = decodex_protocol::agent_async_question_reply(source, &answer) {
			self.feedback = Feedback::warning(message);
			cx.notify();
			return true;
		}
		let (Ok(work_id), Ok(question_id), Ok(answer)) =
			(EntityId::new(target.work), WireText::new(target.question), HistoryText::new(answer))
		else {
			return true;
		};
		self.execute(AgentActionDto::AnswerQuestion { work_id, question_id, answer }, None, cx);
		true
	}

	pub(super) fn prune_async_question_drafts(&mut self, work: &str, history: &AgentHistoryResult) {
		let AgentHistoryResult::Available {
			questions,
			questions_truncated,
			questions_recovering: false,
			..
		} = history
		else {
			return;
		};
		let Some(owner) = self
			.snapshot
			.as_ref()
			.and_then(|snapshot| snapshot.work_items.iter().find(|item| item.id == work))
		else {
			return;
		};
		self.async_question_submissions.retain(|submission| {
			submission.target.work != work
				|| (owner.codex_thread_id.as_deref() == Some(submission.target.thread.as_str())
					&& (*questions_truncated
						|| questions.iter().any(|q| q.id == submission.target.question)))
		});
		// Old custom answers remain recoverable until their native question is resolved.
		self.restored_question_drafts.retain(|saved| {
			saved.work_id.as_str() != work
				|| (owner.codex_thread_id.as_deref() == Some(saved.thread_id.as_str())
					&& (*questions_truncated
						|| questions.iter().any(|q| q.id == saved.question_id.as_str())))
		});
	}

	fn pending_async_question(&self, target: &QuestionTarget) -> Option<&AgentAsyncQuestionDto> {
		if self.question_submitted(target)
			|| !self.conversation_matches(&target.work, &target.thread)
		{
			return None;
		}
		let (owner, AgentHistoryResult::Available { questions, questions_recovering: false, .. }) =
			self.history.as_ref()?
		else {
			return None;
		};
		questions.iter().find(|q| owner == &target.work && q.id == target.question)
	}

	fn perform_question_action(
		&mut self,
		target: &QuestionTarget,
		action: &QuestionAction,
		_window: &mut Window,
		cx: &mut Context<Self>,
	) {
		let Some(source) = self.pending_async_question(target) else {
			return;
		};
		if self.sending || self.uncertain {
			return;
		}
		let (Ok(work_id), Ok(question_id)) =
			(EntityId::new(&target.work), WireText::new(&target.question))
		else {
			return;
		};
		let command = match action {
			QuestionAction::Send { answer, visible } => {
				if !source.options.contains(answer) {
					return;
				}
				if !visible.get() {
					self.feedback =
						Feedback::warning("Show the entire suggested answer before sending.");
					cx.notify();
					return;
				}
				if let Err(message) = decodex_protocol::agent_async_question_reply(source, answer) {
					self.feedback = Feedback::warning(message);
					cx.notify();
					return;
				}
				let Ok(answer) = HistoryText::new(answer) else {
					return;
				};
				AgentActionDto::AnswerQuestion { work_id, question_id, answer }
			},
			QuestionAction::Skip => {
				let Ok(thread_id) = WireText::new(&target.thread) else {
					return;
				};
				AgentActionDto::SkipQuestion { work_id, thread_id, question_id }
			},
		};
		// Direct replies never consume or clear the ordinary message draft.
		self.execute(command, None, cx);
	}

	fn question_button(
		&self,
		target: &QuestionTarget,
		action: QuestionAction,
		id: String,
		label: String,
		cx: &mut Context<Self>,
	) -> gpui::Stateful<gpui::Div> {
		let tooltip = match action {
			QuestionAction::Send { .. } => "Send answer",
			QuestionAction::Skip => "Skip question",
		};
		let target = target.clone();
		let key_target = target.clone();
		let key_action = action.clone();
		gpui::div()
			.id(SharedString::from(id.clone()))
			.debug_selector(move || id)
			.role(Role::Button)
			.tab_index(0)
			.aria_label(label.clone())
			.tooltip(move |_, cx| cx.new(|_| ComposerTip(tooltip.into())).into())
			.rounded(gpui::px(6.))
			.cursor_pointer()
			.hover(|style| style.bg(crate::ui_theme::rgba(ui_theme::HOVER_FILL)))
			.focus_visible(|style| style.bg(crate::ui_theme::rgba(ui_theme::PRESSED_FILL)))
			.on_click(cx.listener(move |s, _, window, cx| {
				s.perform_question_action(&target, &action, window, cx);
				cx.stop_propagation();
			}))
			.on_key_down(cx.listener(move |s, event: &KeyDownEvent, window, cx| {
				if matches!(event.keystroke.key.as_str(), "enter" | "space") {
					if !event.is_held {
						s.perform_question_action(&key_target, &key_action, window, cx);
					}
					cx.stop_propagation();
				}
			}))
	}

	fn async_choice_button(
		&self,
		target: &QuestionTarget,
		index: usize,
		option: &str,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let visible = Rc::new(Cell::new(false));
		let direct = QuestionAction::Send { answer: option.into(), visible: visible.clone() };
		let send = self
			.question_button(
				target,
				direct.clone(),
				format!("async-option-{}-{index}", target.question),
				format!("Send: {option}"),
				cx,
			)
			.flex_1()
			.min_w_0()
			.flex()
			.items_start()
			.gap_2()
			.px_2()
			.py(gpui::px(6.))
			.min_h(gpui::px(32.))
			.child(
				gpui::div()
					.w(gpui::px(16.))
					.flex_none()
					.text_center()
					.text_color(crate::ui_theme::rgb(ui_theme::TEXT_MUTED))
					.child((index + 1).to_string()),
			)
			.child(gpui::div().flex_1().min_w_0().whitespace_normal().child(option.to_owned()))
			.child(
				gpui::canvas(
					move |bounds, window, _| {
						let mask = window
							.content_mask()
							.bounds
							.intersect(&Bounds::new(Point::default(), window.viewport_size()));
						visible.set(
							bounds.size.width > gpui::px(0.)
								&& bounds.size.height > gpui::px(0.)
								&& mask.intersect(&bounds) == bounds,
						);
					},
					|_, _, _, _| {},
				)
				.absolute()
				.inset_0(),
			)
			.smooth()
			.enabled(!self.sending && !self.uncertain);
		gpui::div().w_full().flex().child(send).into_any_element()
	}

	pub(super) fn update_question_placeholder(&self, cx: &mut Context<Self>) {
		self.composer.update(cx, |input, cx| {
			input.set_placeholder(agent_surface::prompts::session_quote(), cx)
		});
	}

	pub(super) fn composer_question_panel(&self, cx: &mut Context<Self>) -> AnyElement {
		let work = self.conversation_work();
		let panel = work.as_ref().and_then(|work| self.render_async_questions(work, cx));
		ui_motion::disclosure(
			"composer-question-panel",
			panel.is_some(),
			panel.unwrap_or_else(|| gpui::div().into_any_element()),
		)
		.into_any_element()
	}

	pub(super) fn async_question_item_text(
		&self,
		work: &AgentWorkItemDto,
		item: &str,
	) -> Option<AnyElement> {
		let (id, AgentHistoryResult::Available { questions, questions_recovering: false, .. }) =
			self.history.as_ref()?
		else {
			return None;
		};
		if id != &work.id {
			return None;
		}
		let titles: Vec<_> = questions
			.iter()
			.filter(|q| question_source(&q.id).as_deref() == Some(item))
			.map(|q| {
				gpui::div().child(SelectableText::plain(
					format!("question-history-{}", q.id),
					q.title.clone(),
				))
			})
			.collect();
		(!titles.is_empty())
			.then(|| gpui::div().flex().flex_col().gap_2().children(titles).into_any_element())
	}

	fn render_async_questions(
		&self,
		work: &AgentWorkItemDto,
		cx: &mut Context<Self>,
	) -> Option<AnyElement> {
		let target = self.composer_question_target()?;
		let (
			id,
			AgentHistoryResult::Available {
				questions,
				questions_truncated,
				questions_recovering: false,
				..
			},
		) = self.history.as_ref()?
		else {
			return None;
		};
		if id != &work.id || target.work != work.id {
			return None;
		}
		let question = questions.iter().find(|q| q.id == target.question)?;
		let mut panel = gpui::div()
			.id("composer-question-options")
			.w_full()
			.min_w_0()
			.max_h(gpui::px(240.))
			.overflow_y_scroll()
			.flex()
			.flex_col()
			.gap_3();
		if let Some(card) = self.async_question_card(work, question, cx) {
			panel = panel.child(card);
		}
		if *questions_truncated {
			panel = panel.child(agent_surface::muted(
				"More questions are available after these are answered.",
			));
		}
		Some(
			gpui::div()
				.w_full()
				.min_w_0()
				.flex()
				.flex_col()
				.pt_1()
				.pb_1()
				.child(panel)
				.child(
					gpui::div().h(gpui::px(1.)).mx_2().mt_2().bg(crate::ui_theme::rgba(0xffffff14)),
				)
				.into_any_element(),
		)
	}

	fn async_question_card(
		&self,
		work: &AgentWorkItemDto,
		question: &AgentAsyncQuestionDto,
		cx: &mut Context<Self>,
	) -> Option<AnyElement> {
		let target = QuestionTarget {
			work: work.id.clone(),
			thread: work.codex_thread_id.clone()?,
			question: question.id.clone(),
		};
		let skip = self
			.question_button(
				&target,
				QuestionAction::Skip,
				format!("async-skip-{}", question.id),
				"Skip question".into(),
				cx,
			)
			.size(gpui::px(24.))
			.flex_none()
			.flex()
			.items_center()
			.justify_center()
			.text_color(crate::ui_theme::rgb(ui_theme::TEXT_MUTED))
			.child(workspace_symbols::icon_sized(Symbol::Close, 12.))
			.smooth()
			.enabled(!self.sending && !self.uncertain);
		let mut card = gpui::div().w_full().min_w_0().flex().flex_col().gap(gpui::px(2.)).child(
			gpui::div()
				.flex()
				.items_start()
				.px_2()
				.gap_3()
				.when(!question.options.is_empty(), |header| header.mb_2())
				.child(
					gpui::div()
						.id(SharedString::from(format!("async-question-title-{}", question.id)))
						.role(Role::Heading)
						.aria_level(3)
						.aria_label(question.title.clone())
						.flex_1()
						.min_w_0()
						.pt(gpui::px(2.5))
						.whitespace_normal()
						.font_weight(gpui::FontWeight::MEDIUM)
						.child(SelectableText::plain(
							format!("question-title-selection-{}", question.id),
							question.title.clone(),
						)),
				)
				.child(skip),
		);
		for (index, option) in question.options.iter().enumerate() {
			card = card.child(self.async_choice_button(&target, index, option, cx));
		}
		Some(card.into_any_element())
	}
}

fn saved_custom_answer(saved: &DesktopQuestionDraft) -> Option<&str> {
	let answer = if saved.selected.as_deref() != Some(&saved.text) && !saved.text.trim().is_empty()
	{
		Some(saved.text.as_str())
	} else {
		saved.custom.as_deref()
	};
	answer.filter(|text| !text.trim().is_empty())
}

fn question_source(id: &str) -> Option<String> {
	let (kind, source, _): (String, String, usize) = serde_json::from_str(id).ok()?;
	(kind == "request_user_input_async").then_some(source)
}

#[cfg(test)]
mod tests {
	use super::{QuestionAction, QuestionTarget, saved_custom_answer};
	use crate::shell::agent_surface::{
		AgentCommandResponse, AgentDispatchStateDto, AgentHistoryResult, AgentSnapshotDto,
		AgentSnapshotResult, AgentSurface, AgentWorkItemDto, AgentWorkStatusDto, Context,
		PendingCommand,
	};
	use decodex_protocol::{
		AgentActionDto, DesktopQuestionDraft, EntityId, HistoryText, IdempotencyKey, WireText,
	};
	use std::{cell::Cell, rc::Rc};

	fn settle_choices(visual: &mut gpui::VisualTestContext) {
		visual.update(|window, cx| {
			window.draw(cx).clear(cx);
		});
		std::thread::sleep(std::time::Duration::from_millis(240));
		visual.update(|window, cx| {
			window.draw(cx).clear(cx);
		});
	}

	fn target() -> QuestionTarget {
		QuestionTarget { work: "root".into(), thread: "thread".into(), question: "q1".into() }
	}

	fn answer_command() -> AgentActionDto {
		AgentActionDto::AnswerQuestion {
			work_id: EntityId::new("root").unwrap(),
			question_id: WireText::new("q1").unwrap(),
			answer: HistoryText::new("Markdown").unwrap(),
		}
	}

	fn pending_reply(s: &AgentSurface, key: &IdempotencyKey) -> PendingCommand {
		PendingCommand {
			recovery: None,
			key: Some(key.clone()),
			steer: None,
			execution_intent: None,
			epoch: s.command_epoch,
			draft: None,
			owner: Some("root".into()),
			attachments: None,
			references: None,
		}
	}

	#[gpui::test]
	fn submitted_question_closes_before_acceptance_and_stale_history(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
		let key = IdempotencyKey::new("reply-before-history").unwrap();
		surface.update(visual, |s, cx| {
			install_question_fixture(s, cx);
			s.composer.update(cx, |input, cx| input.set_content("Keep this ordinary draft", cx));
			cx.notify();
		});
		settle_choices(visual);
		let expanded = visual.debug_bounds("agent-composer").unwrap().size.height;
		assert!(visual.debug_bounds("async-option-q1-0").is_some());
		surface.update(visual, |s, cx| {
			s.begin_question_submission(&answer_command(), &key, cx);
			s.sending = true;
			s.sync_question_editor(cx);
			assert!(s.composer_question_target().is_none());
			assert_eq!(s.conversation_composer().entity_id(), s.composer.entity_id());
			cx.notify();
		});
		visual.update(|w, cx| w.draw(cx).clear(cx));
		assert!(
			visual.debug_bounds("async-option-q1-0").is_none(),
			"remove choices before any server response"
		);
		settle_choices(visual);
		assert!(visual.debug_bounds("agent-composer").unwrap().size.height < expanded);
		surface.update(visual, |s, cx| {
			s.finish_command(
				pending_reply(s, &key),
				Ok(AgentCommandResponse::Accepted { work_id: EntityId::new("root").unwrap() }),
				cx,
			);
			let (_, stale) = s.history.clone().unwrap();
			s.prune_async_question_drafts("root", &stale);
			assert!(
				s.composer_question_target().is_none(),
				"a stale read must not reopen an accepted question"
			);
			assert_eq!(s.composer.read(cx).content(), "Keep this ordinary draft");
			s.snapshot.as_mut().unwrap().work_items[0].codex_thread_id =
				Some("another-thread".into());
			assert!(s.composer_question_target().is_some(), "submission belongs to one thread");
			s.snapshot.as_mut().unwrap().work_items[0].codex_thread_id = Some("thread".into());
			let mut fresh = stale;
			if let AgentHistoryResult::Available { questions, .. } = &mut fresh {
				questions.clear();
			}
			s.prune_async_question_drafts("root", &fresh);
			assert!(s.async_question_submissions.is_empty());
		});
	}

	#[gpui::test]
	fn failed_question_submission_restores_answer_without_replacing_ordinary_draft(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
		surface.update(visual, |s, cx| {
			install_question_fixture(s, cx);
			s.composer.update(cx, |input, cx| input.set_content("Ordinary draft", cx));
			s.sync_question_editor(cx);
			s.conversation_composer()
				.clone()
				.update(cx, |input, cx| input.set_content("My custom answer", cx));
			let key = IdempotencyKey::new("failed-question").unwrap();
			s.begin_question_submission(&answer_command(), &key, cx);
			s.sync_question_editor(cx);
			assert!(s.composer_question_target().is_none());
			s.finish_command(pending_reply(s, &key), Err("Nothing was sent".into()), cx);
			s.sync_question_editor(cx);
			assert_eq!(s.conversation_composer().read(cx).content(), "My custom answer");
			assert_eq!(s.composer.read(cx).content(), "Ordinary draft");
			assert!(s.pending_async_question(&target()).is_some());
		});
	}

	#[gpui::test]
	fn clicking_option_submits_immediately_without_consuming_draft(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
		surface.update(visual, |s, cx| {
			install_question_fixture(s, cx);
			s.composer.update(cx, |input, cx| input.set_content("Keep my draft", cx));
		});
		visual.update(|window, cx| {
			window.resize(gpui::size(gpui::px(1_180.), gpui::px(1_200.)));
			window.draw(cx).clear(cx);
		});
		settle_choices(visual);
		assert!(visual.debug_bounds("async-question-toggle").is_none());
		assert!(visual.debug_bounds("async-option-q1-2").is_none());
		let option = visual.debug_bounds("async-option-q1-1").unwrap();
		let composer = visual.debug_bounds("agent-composer").unwrap();
		assert_eq!(composer.intersect(&option), option);
		assert!(visual.debug_bounds("async-append-q1-1").is_none());
		assert!(visual.debug_bounds("async-send-q1-1").is_none());
		visual.simulate_click(option.center(), gpui::Modifiers::default());
		surface.update(visual, |s, cx| {
			assert_eq!(
				s.feedback.message, "No service profile is configured.",
				"one click reaches submission"
			);
			assert_eq!(s.composer.read(cx).content(), "Keep my draft");
			assert!(
				s.composer_question_target().is_some(),
				"a rejected preflight must keep the question open"
			);
			s.feedback.clear();
		});
		visual.simulate_event(gpui::KeyDownEvent {
			keystroke: gpui::Keystroke::parse("enter").unwrap(),
			is_held: true,
			prefer_character_input: false,
		});
		surface.read_with(visual, |s, _| assert!(s.feedback.is_empty()));
		visual.simulate_keystrokes("enter");
		surface.read_with(visual, |s, cx| {
			assert_eq!(s.feedback.message, "No service profile is configured.");
			assert_eq!(s.composer.read(cx).content(), "Keep my draft");
		});
	}

	#[gpui::test]
	fn restore_saved_answer_preserves_normal_composer(cx: &mut gpui::TestAppContext) {
		cx.update(crate::composer_input::bind_keys);
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
		surface.update(visual, |s, cx| {
			install_question_fixture(s, cx);
			s.composer.update(cx, |input, cx| input.set_content("Please keep it concise.", cx));
			s.restored_question_drafts.push(DesktopQuestionDraft {
				work_id: EntityId::new("root").unwrap(),
				thread_id: WireText::new("thread").unwrap(),
				question_id: WireText::new("q1").unwrap(),
				text: "Markdown".into(),
				selected: None,
				custom: None,
				collapsed: false,
			});
		});
		visual.update(|window, cx| {
			window.resize(gpui::size(gpui::px(1_180.), gpui::px(1_200.)));
			window.draw(cx).clear(cx);
		});
		settle_choices(visual);
		assert!(visual.debug_bounds("async-restore-q1").is_none());
		surface.read_with(visual, |s, cx| {
			assert_eq!(s.composer.read(cx).content(), "Please keep it concise.");
			assert_eq!(s.conversation_composer().read(cx).content(), "Markdown");
			assert_ne!(s.conversation_composer().entity_id(), s.composer.entity_id());
		});
	}

	#[gpui::test]
	fn free_answer_is_separate_and_restores_the_ordinary_draft(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
		surface.update(visual, |s, cx| {
			install_question_fixture(s, cx);
			let (_, AgentHistoryResult::Available { questions, .. }) = s.history.as_mut().unwrap()
			else {
				panic!("fixture")
			};
			questions[0].options.clear();
			s.composer.update(cx, |input, cx| input.set_content("Keep my ordinary draft", cx));
			s.sync_question_editor(cx);
			assert!(s.conversation_composer().read(cx).content().is_empty());
			s.submit(cx);
			assert!(s.feedback.is_empty(), "an empty answer must never send the ordinary draft");
			s.conversation_composer()
				.clone()
				.update(cx, |input, cx| input.set_content("For the librarians", cx));
		});
		visual.update(|window, cx| window.draw(cx).clear(cx));
		surface.update(visual, |s, cx| {
			assert_eq!(s.restored_question_drafts[0].text, "For the librarians");
			s.submit(cx);
			assert_eq!(s.feedback.message, "No service profile is configured.");
			assert_eq!(
				s.conversation_composer().read(cx).content(),
				"For the librarians",
				"a failed send retains the answer"
			);
			assert_eq!(s.composer.read(cx).content(), "Keep my ordinary draft");
			let (_, AgentHistoryResult::Available { questions, .. }) = s.history.as_mut().unwrap()
			else {
				panic!("fixture")
			};
			questions.clear();
			s.sync_question_editor(cx);
			assert_eq!(s.conversation_composer().entity_id(), s.composer.entity_id());
			assert_eq!(s.conversation_composer().read(cx).content(), "Keep my ordinary draft");
		});
	}

	#[gpui::test]
	fn accepted_direct_reply_preserves_draft_and_attachments(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
		surface.update(visual, |s, cx| {
			install_question_fixture(s, cx);
			s.composer.update(cx, |input, cx| input.set_content("Unsent ordinary draft", cx));
			s.attachments.push(decodex_protocol::AgentAttachmentDto {
				path: decodex_protocol::ConversationWorkingDirectory::new("/tmp/note.png").unwrap(),
				image: true,
				skill_name: None,
			});
			s.finish_command(
				PendingCommand {
					recovery: None,
					key: None,
					steer: None,
					execution_intent: None,
					epoch: s.command_epoch,
					draft: None,
					owner: Some("root".into()),
					attachments: None,
					references: None,
				},
				Ok(AgentCommandResponse::Accepted { work_id: EntityId::new("root").unwrap() }),
				cx,
			);
			assert_eq!(s.composer.read(cx).content(), "Unsent ordinary draft");
			assert_eq!(s.attachments.len(), 1);
		});
	}

	#[gpui::test]
	fn stale_or_hidden_question_cannot_submit(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
		visual.update(|window, cx| {
			surface.update(cx, |s, cx| {
				install_question_fixture(s, cx);
				let visible = Rc::new(Cell::new(false));
				let action =
					QuestionAction::Send { answer: "Markdown".into(), visible: visible.clone() };
				s.perform_question_action(&target(), &action, window, cx);
				assert_eq!(s.feedback.message, "Show the entire suggested answer before sending.");
				s.feedback.clear();
				visible.set(true);
				s.snapshot.as_mut().unwrap().work_items[0].codex_thread_id =
					Some("new-thread".into());
				s.perform_question_action(&target(), &action, window, cx);
				assert!(s.feedback.is_empty());
				assert!(s.composer.read(cx).content().is_empty());
				assert!(!s.sending);
			})
		});
	}

	#[gpui::test]
	fn legacy_custom_answer_is_recoverable_without_taking_over_composer(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
		surface.update(visual, |s, cx| {
			install_question_fixture(s, cx);
			s.restored_question_drafts.push(DesktopQuestionDraft {
				work_id: EntityId::new("root").unwrap(),
				thread_id: WireText::new("thread").unwrap(),
				question_id: WireText::new("q1").unwrap(),
				text: "PDF".into(),
				selected: Some("PDF".into()),
				custom: Some("Plain text, please.".into()),
				collapsed: true,
			});
			s.composer.update(cx, |input, cx| input.set_content("Keep this draft", cx));
			let (_, mut history) = s.history.clone().unwrap();
			s.prune_async_question_drafts("root", &history);
			assert_eq!(
				saved_custom_answer(&s.restored_question_drafts[0]),
				Some("Plain text, please.")
			);
			assert_eq!(s.composer.read(cx).content(), "Keep this draft");
			let AgentHistoryResult::Available {
				questions,
				questions_recovering,
				questions_truncated,
				..
			} = &mut history
			else {
				panic!("fixture");
			};
			questions.clear();
			*questions_recovering = true;
			*questions_truncated = true;
			s.prune_async_question_drafts("root", &history);
			assert_eq!(s.restored_question_drafts.len(), 1);
			if let AgentHistoryResult::Available { questions_recovering, .. } = &mut history {
				*questions_recovering = false;
			}
			s.prune_async_question_drafts("root", &history);
			assert_eq!(s.restored_question_drafts.len(), 1);
			if let AgentHistoryResult::Available { questions_truncated, .. } = &mut history {
				*questions_truncated = false;
			}
			s.prune_async_question_drafts("root", &history);
			assert!(s.restored_question_drafts.is_empty());
		});
	}
	fn install_question_fixture(s: &mut AgentSurface, _cx: &mut Context<AgentSurface>) {
		s.apply_result(Ok(AgentSnapshotResult::Available(AgentSnapshotDto {
			archived_work_ids: Default::default(),
			agent_slots: Default::default(),
			context_references: vec![],
			connection_initializing: false,
			runtime_source: None,
			workspaces: vec![],
			dependencies: vec![],
			pending_events: vec![],
			work_items: vec![AgentWorkItemDto {
				id: "root".into(),
				parent_goal_id: None,
				kind: decodex_protocol::AgentWorkKindDto::Goal,
				title: "Agent".into(),
				codex_thread_id: Some("thread".into()),
				active_turn_id: Some("turn".into()),
				dispatch_state: AgentDispatchStateDto::Running,
				status: AgentWorkStatusDto::Open,
				next_check_at_micros: None,
				created_at_micros: 1,
				updated_at_micros: 1,
			}],
		})));

		let history = AgentHistoryResult::Available {
			questions: vec![decodex_protocol::AgentAsyncQuestionDto {
				arrived_live: false,
				id: "q1".into(),
				title: "Choose a format".into(),
				options: vec!["PDF".into(), "Markdown".into()],
			}],
			questions_truncated: false,
			questions_recovering: false,
			misalignment: None,
			usage: None,
			entries: vec![],
			has_more: false,
			next_before: None,
			live: vec![],
		};

		s.history = Some(("root".into(), history));
	}
	#[gpui::test]
	fn question_title_can_be_selected_and_copied_without_submitting(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
		surface.update(visual, |s, cx| {
			install_question_fixture(s, cx);
			cx.notify();
		});
		visual.update(|window, cx| {
			window.resize(gpui::size(gpui::px(1400.), gpui::px(1400.)));
			window.draw(cx).clear(cx);
		});
		for _ in 0..20 {
			std::thread::sleep(std::time::Duration::from_millis(16));
			visual.update(|w, cx| w.draw(cx).clear(cx));
		}
		let title = visual.debug_bounds("question-title-selection-q1").expect("question title");
		visual.simulate_click(title.center(), Default::default());
		visual.simulate_keystrokes("cmd-a cmd-c");
		visual.update(|_, cx| {
			assert_eq!(
				cx.read_from_clipboard().and_then(|item| item.text()).as_deref(),
				Some("Choose a format")
			);
			assert!(surface.read(cx).async_question_submissions.is_empty());
		});
	}

	#[gpui::test]
	fn question_actions_stay_in_composer_across_history_pages(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
		surface.update(visual, |s, cx| {
			install_question_fixture(s, cx);
			let (_, history) = s.history.as_mut().unwrap();
			let AgentHistoryResult::Available { questions, .. } = history else {
				panic!("fixture history");
			};
			questions[0].id = decodex_protocol::agent_async_question_id("question-source", 0);
			let work = s.snapshot.as_ref().unwrap().work_items[0].clone();
			assert!(s.render_async_questions(&work, cx).is_some());
			s.timeline.native.entries.push(decodex_protocol::AgentTimelineEntry {
				position: 1,
				content: decodex_protocol::AgentTimelineContent::Item {
					turn_id: "turn".into(),
					item_id: "question-source".into(),
					kind: "agentMessage".into(),
					phase: Some("final_answer".into()),
					text: "Choose a format".into(),
					truncated: false,
					collaboration: None,
					activity: None,
					app_ui: false,
					attachments: vec![],
				},
			});
			assert!(s.async_question_item_text(&work, "question-source").is_some());
			assert!(s.async_question_item_text(&work, "other-message").is_none());
			assert!(
				s.render_async_questions(&work, cx).is_some(),
				"pending choices stay with the composer even when the source is visible"
			);
			s.timeline.native.entries.clear();
			assert!(
				s.render_async_questions(&work, cx).is_some(),
				"paging out the source must not hide a pending question"
			);
		});
	}
}
