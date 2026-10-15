//! Nonblocking model questions are explicit user messages, not approval callbacks.
use std::{cell::RefCell, collections::BTreeMap, mem};

use crate::{
	ui_motion::{self, SmoothControl as _},
	ui_theme,
};
use gpui::{AnyElement, AppContext as _, Bounds, Focusable, KeyDownEvent, Point, RenderOnce};

use crate::shell::agent_surface::{
	self, AgentActionDto, AgentHistoryResult, AgentSurface, AgentWorkItemDto, ComposerInput,
	Context, Entity, EntityId, InteractiveElement, IntoElement, ParentElement, Role, SharedString,
	StatefulInteractiveElement, Styled, SubmitComposer, Window, workspace_symbols,
	workspace_symbols::Symbol,
};
#[cfg(test)]
use crate::shell::agent_surface::{
	AgentDispatchStateDto, AgentSnapshotDto, AgentSnapshotResult, AgentWorkStatusDto,
};
use decodex_protocol::{AgentAsyncQuestionDto, HistoryText, WireText};

#[derive(Default)]
pub(super) struct ChoiceDraft {
	selected: Option<String>,
	custom: Option<Entity<ComposerInput>>,
	visible: RefCell<BTreeMap<String, std::rc::Rc<std::cell::Cell<bool>>>>,
}

#[derive(IntoElement)]
struct QuestionChoice {
	id: SharedString,
	selected: bool,
	index: usize,
	row: gpui::Stateful<gpui::Div>,
}
impl RenderOnce for QuestionChoice {
	fn render(self, window: &mut Window, cx: &mut gpui::App) -> impl IntoElement {
		let selected = ui_motion::value(self.id, f32::from(self.selected), window, cx);
		self.row
			.bg(gpui::rgba(0xffffff00 | (selected * 10.) as u32))
			.child(
				gpui::div()
					.absolute()
					.left(gpui::px(8.))
					.top(gpui::px(5.))
					.w(gpui::px(16.))
					.flex()
					.justify_center()
					.text_size(gpui::px(10.5))
					.text_color(gpui::rgb(ui_theme::TEXT_MUTED))
					.opacity(0.65 * (1. - selected))
					.child((self.index + 1).to_string()),
			)
			.child(
				gpui::div()
					.absolute()
					.left(gpui::px(10. + 2. * (1. - selected)))
					.top(gpui::px(8.))
					.opacity(selected)
					.child(workspace_symbols::icon_sized(Symbol::Confirm, 12.)),
			)
			.smooth()
	}
}

impl AgentSurface {
	pub(super) fn prepare_async_question_inputs(
		&mut self,
		work: &str,
		history: &AgentHistoryResult,
		cx: &mut Context<Self>,
	) {
		let AgentHistoryResult::Available {
			questions,
			questions_truncated,
			questions_recovering,
			..
		} = history
		else {
			return;
		};

		self.bind_async_question_thread(work);

		if *questions_recovering {
			return;
		}
		if !questions_truncated && !questions_recovering {
			self.async_question_inputs.retain(|(owner, id), _| {
				owner != work || questions.iter().any(|question| &question.id == id)
			});
		}

		self.async_question_choices.retain(|key, _| self.async_question_inputs.contains_key(key));

		for question in questions {
			self.async_question_choices.entry((work.into(), question.id.clone())).or_insert_with(
				|| ChoiceDraft {
					selected: question.options.first().cloned(),
					..Default::default()
				},
			);

			self.async_question_inputs.entry((work.into(), question.id.clone())).or_insert_with(
				|| {
					let input = cx.new(|cx| {
						ComposerInput::with_placeholder(40, "Your answer", "Reply to agent", cx)
					});

					if let Some(option) = question.options.first() {
						input.update(cx, |input, cx| input.set_content(option, cx));
					}

					input
				},
			);
		}

		self.restore_async_drafts(work, questions, *questions_truncated, cx);
	}

	pub(super) fn capture_async_drafts(
		&self,
		cx: &Context<Self>,
	) -> Option<Vec<decodex_protocol::DesktopQuestionDraft>> {
		let mut saved = self.restored_question_drafts.clone();

		for ((work, question), input) in &self.async_question_inputs {
			let thread = self.async_question_threads.get(work)?;
			let state = self.async_question_choices.get(&(work.clone(), question.clone()));

			saved.push(decodex_protocol::DesktopQuestionDraft {
				work_id: EntityId::new(work).ok()?,
				thread_id: WireText::new(thread).ok()?,
				question_id: WireText::new(question).ok()?,
				text: input.read(cx).content().into(),
				selected: state.and_then(|state| state.selected.clone()),
				custom: state
					.and_then(|state| state.custom.as_ref())
					.map(|input| input.read(cx).content().into()),
				collapsed: self.collapsed_async_questions.contains(work),
			});
		}

		Some(saved)
	}

	fn restore_async_drafts(
		&mut self,
		work: &str,
		questions: &[AgentAsyncQuestionDto],
		truncated: bool,
		cx: &mut Context<Self>,
	) {
		let Some(thread) = self.async_question_threads.get(work).cloned() else {
			return;
		};

		for saved in mem::take(&mut self.restored_question_drafts) {
			if saved.work_id.as_str() != work {
				self.restored_question_drafts.push(saved);

				continue;
			}
			if saved.thread_id.as_str() != thread {
				continue;
			}

			let Some(question) =
				questions.iter().find(|question| question.id == saved.question_id.as_str())
			else {
				if truncated {
					self.restored_question_drafts.push(saved);
				}

				continue;
			};
			let key = (work.into(), question.id.clone());

			if let Some(input) = self.async_question_inputs.get(&key) {
				input.update(cx, |input, cx| input.set_content(&saved.text, cx));
			}

			let custom = saved.custom.map(|text| {
				cx.new(|cx| {
					let mut input = ComposerInput::with_placeholder(
						40,
						"Write an answer",
						"Reply to agent",
						cx,
					);

					input.set_content(&text, cx);

					input
				})
			});
			let selected = saved.selected.filter(|option| question.options.contains(option));

			self.async_question_choices
				.insert(key, ChoiceDraft { selected, custom, ..Default::default() });

			if saved.collapsed {
				self.collapsed_async_questions.insert(work.into());
			}
		}
	}

	fn bind_async_question_thread(&mut self, work: &str) {
		let Some(owner) = self
			.snapshot
			.as_ref()
			.and_then(|snapshot| snapshot.work_items.iter().find(|item| item.id == work))
		else {
			return;
		};
		let thread = owner.codex_thread_id.clone();

		if self
			.async_question_threads
			.get(work)
			.is_some_and(|previous| Some(previous) != thread.as_ref())
		{
			self.async_question_inputs.retain(|(owner, _), _| owner != work);
			self.async_question_choices.retain(|(owner, _), _| owner != work);
			self.collapsed_async_questions.remove(work);
		}

		if let Some(thread) = thread {
			self.async_question_threads.insert(work.into(), thread);
		} else {
			self.async_question_threads.remove(work);
		}
	}

	fn select_async_choice(
		&mut self,
		key: &(String, String),
		option: Option<&str>,
		window: &mut Window,
		cx: &mut Context<Self>,
	) {
		if self.sending || self.uncertain {
			return;
		}
		let Some(current) = self.async_question_inputs.get(key).cloned() else {
			return;
		};
		let state = self.async_question_choices.entry(key.clone()).or_default();

		if state.selected.as_deref() != Some(current.read(cx).content()) {
			state.custom = Some(current);
		}

		let input = if let Some(option) = option {
			let input = cx
				.new(|cx| ComposerInput::with_placeholder(40, "Your answer", "Reply to agent", cx));

			input.update(cx, |input, cx| input.set_content(option, cx));

			input
		} else {
			state
				.custom
				.get_or_insert_with(|| {
					cx.new(|cx| {
						ComposerInput::with_placeholder(40, "Write an answer", "Reply to agent", cx)
					})
				})
				.clone()
		};

		state.selected = option.map(str::to_owned);

		if option.is_none() {
			window.focus(&input.focus_handle(cx), cx);
		}

		self.async_question_inputs.insert(key.clone(), input);
		cx.notify();
	}

	fn async_choice_button(
		&self,
		key: &(String, String),
		index: usize,
		option: Option<&str>,
		selected: bool,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let visible = std::rc::Rc::new(std::cell::Cell::new(false));

		if let Some(option) = option
			&& let Some(state) = self.async_question_choices.get(key)
		{
			state.visible.borrow_mut().insert(option.into(), visible.clone());
		}

		let label = option.unwrap_or("Something else…").to_owned();
		let option = option.map(str::to_owned);
		let key_option = option.clone();
		let owner = key.clone();
		let key_owner = key.clone();
		let selector = format!("async-option-{}-{index}", key.1);

		let row = gpui::div()
			.id(SharedString::from(selector.clone()))
			.debug_selector(move || selector)
			.relative()
			.min_w_0()
			.whitespace_normal()
			.role(Role::RadioButton)
			.aria_toggled(if selected { gpui::Toggled::True } else { gpui::Toggled::False })
			.tab_index(0)
			.aria_label(label.clone())
			.py(gpui::px(4.))
			.pl(gpui::px(32.))
			.pr(gpui::px(10.))
			.min_h(gpui::px(28.))
			.rounded(gpui::px(6.0))
			.hover(|style| style.bg(gpui::rgba(ui_theme::HOVER_FILL)))
			.focus_visible(|style| style.bg(gpui::rgba(ui_theme::PRESSED_FILL)))
			.cursor_pointer()
			.on_click(cx.listener(move |s, _, window, cx| {
				s.select_async_choice(&owner, option.as_deref(), window, cx)
			}))
			.on_key_down(cx.listener(move |s, event: &KeyDownEvent, window, cx| {
				if matches!(event.keystroke.key.as_str(), "enter" | "space") {
					if !event.is_held {
						if event.keystroke.key == "enter" && event.keystroke.modifiers.platform {
							s.answer_async_question(&key_owner.0, &key_owner.1, cx);
						} else {
							s.select_async_choice(&key_owner, key_option.as_deref(), window, cx);
						}
					}

					cx.stop_propagation();
				}
			}))
			.child(label)
			.child(
				gpui::canvas(
					move |bounds, window, _| {
						let mask = window
							.content_mask()
							.bounds
							.intersect(&Bounds::new(Point::default(), window.viewport_size()));

						visible.set(
							bounds.size.width > gpui::px(0.0)
								&& bounds.size.height > gpui::px(0.0)
								&& mask.intersect(&bounds) == bounds,
						);
					},
					|_, _, _, _| {},
				)
				.absolute()
				.inset_0(),
			);
		QuestionChoice {
			id: format!("async-selection-{}-{}-{index}", key.0, key.1).into(),
			selected,
			index,
			row,
		}
		.into_any_element()
	}

	fn answer_async_question(&mut self, work: &str, question: &str, cx: &mut Context<Self>) {
		if self.sending || self.uncertain {
			return;
		}

		let Some(input) = self.async_question_inputs.get(&(work.into(), question.into())) else {
			return;
		};
		let Ok(answer) = HistoryText::new(input.read(cx).content().trim().to_owned()) else {
			return;
		};

		if answer.as_str().is_empty() {
			return;
		}

		let Some((owner, AgentHistoryResult::Available { questions, .. })) = &self.history else {
			return;
		};
		let Some(source) = questions.iter().find(|item| owner == work && item.id == question)
		else {
			return;
		};
		let matching_options: Vec<_> =
			source.options.iter().filter(|option| option.trim() == answer.as_str()).collect();
		let visible =
			self.async_question_choices.get(&(work.into(), question.into())).is_some_and(|state| {
				matching_options.iter().any(|option| {
					state.visible.borrow().get(*option).is_some_and(|shown| shown.get())
				})
			});

		if self.collapsed_async_questions.contains(work)
			|| (!matching_options.is_empty() && !visible)
		{
			self.feedback = "Show the entire suggested answer before sending.".into();

			cx.notify();

			return;
		}

		if let Err(message) = decodex_protocol::agent_async_question_reply(source, answer.as_str())
		{
			self.feedback = message;

			cx.notify();

			return;
		}

		let (Ok(work_id), Ok(question_id)) = (EntityId::new(work), WireText::new(question)) else {
			return;
		};

		self.execute(AgentActionDto::AnswerQuestion { work_id, question_id, answer }, None, cx);
	}

	fn async_question_skip(
		&self,
		work: &AgentWorkItemDto,
		question: &str,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let Some(thread) = &work.codex_thread_id else {
			return gpui::div().into_any_element();
		};
		let (Ok(work_id), Ok(thread_id), Ok(question_id)) =
			(EntityId::new(&work.id), WireText::new(thread), WireText::new(question))
		else {
			return gpui::div().into_any_element();
		};
		let action = AgentActionDto::SkipQuestion { work_id, thread_id, question_id };
		let key_action = action.clone();
		let selector = format!("async-skip-{question}");

		gpui::div()
			.id(SharedString::from(selector.clone()))
			.debug_selector(move || selector)
			.role(Role::Button)
			.tab_index(0)
			.aria_label("Skip question")
			.px_3()
			.py(gpui::px(5.))
			.rounded(gpui::px(6.))
			.text_color(gpui::rgb(ui_theme::TEXT_MUTED))
			.hover(|style| {
				style.bg(gpui::rgba(ui_theme::HOVER_FILL)).text_color(gpui::rgb(ui_theme::TEXT))
			})
			.focus_visible(|style| style.bg(gpui::rgba(ui_theme::PRESSED_FILL)))
			.cursor_pointer()
			.on_click(cx.listener(move |s, _, _, cx| {
				if !s.sending && !s.uncertain {
					s.execute(action.clone(), None, cx);
				}
			}))
			.on_key_down(cx.listener(move |s, event: &KeyDownEvent, _, cx| {
				if matches!(event.keystroke.key.as_str(), "enter" | "space") && !event.is_held {
					if !s.sending && !s.uncertain {
						s.execute(key_action.clone(), None, cx);
					}

					cx.stop_propagation();
				}
			}))
			.child("Skip")
			.smooth()
			.into_any_element()
	}

	fn toggle_async_questions(&mut self, work: &str, window: &mut Window, cx: &mut Context<Self>) {
		if !self.collapsed_async_questions.remove(work) {
			self.collapsed_async_questions.insert(work.into());
			window.focus(&self.composer.focus_handle(cx), cx);
		}

		cx.notify();
	}

	fn async_question_toggle(
		&self,
		work: &str,
		count: usize,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let collapsed = self.collapsed_async_questions.contains(work);
		let label =
			if collapsed { format!("Show questions ({count})") } else { "Answer later".into() };
		let owner = work.to_owned();
		let key_owner = owner.clone();

		gpui::div()
			.id("async-question-toggle")
			.debug_selector(|| "async-question-toggle".into())
			.role(Role::Button)
			.tab_index(0)
			.aria_label(label.clone())
			.px_2()
			.py_1()
			.text_size(gpui::px(11.))
			.rounded(gpui::px(5.))
			.text_color(gpui::rgb(ui_theme::TEXT_MUTED))
			.hover(|style| style.bg(gpui::rgba(ui_theme::HOVER_FILL)))
			.focus_visible(|style| style.bg(gpui::rgba(ui_theme::PRESSED_FILL)))
			.cursor_pointer()
			.on_click(
				cx.listener(move |s, _, window, cx| s.toggle_async_questions(&owner, window, cx)),
			)
			.on_key_down(cx.listener(move |s, event: &KeyDownEvent, window, cx| {
				if matches!(event.keystroke.key.as_str(), "enter" | "space") && !event.is_held {
					s.toggle_async_questions(&key_owner, window, cx);
					cx.stop_propagation();
				}
			}))
			.child(if collapsed {
				gpui::div().child(label).into_any_element()
			} else {
				workspace_symbols::icon_sized(Symbol::Close, 12.)
			})
			.smooth()
			.into_any_element()
	}

	pub(super) fn async_question_panel(
		&self,
		work: &AgentWorkItemDto,
		cx: &mut Context<Self>,
	) -> AnyElement {
		self.render_async_questions(work, None, cx)
			.unwrap_or_else(|| gpui::div().into_any_element())
	}

	pub(super) fn async_question_item_panel(
		&self,
		work: &AgentWorkItemDto,
		item: &str,
		cx: &mut Context<Self>,
	) -> Option<AnyElement> {
		self.render_async_questions(work, Some(item), cx)
	}

	fn render_async_questions(
		&self,
		work: &AgentWorkItemDto,
		source: Option<&str>,
		cx: &mut Context<Self>,
	) -> Option<AnyElement> {
		let Some((
			id,
			AgentHistoryResult::Available {
				questions,
				questions_truncated,
				questions_recovering,
				..
			},
		)) = &self.history
		else {
			return None;
		};
		if id != &work.id || *questions_recovering {
			return None;
		}
		// The native message owns its question card. Only questions whose source
		// is outside the retained history page need a separate answer panel.
		let questions: Vec<_> = questions.iter().filter(|question| {
            let item = question_source(&question.id);
            if let Some(source) = source {
                item.as_deref() == Some(source)
            } else {
                !self.timeline.native.entries.iter().any(|entry| matches!(&entry.content,
                    decodex_protocol::AgentTimelineContent::Item { item_id, kind, truncated: false, attachments, .. }
                    if kind == "agentMessage" && attachments.is_empty() && item.as_ref() == Some(item_id)))
            }
        }).collect();
		if questions.is_empty() {
			return None;
		}
		for question in &questions {
			if let Some(state) =
				self.async_question_choices.get(&(work.id.clone(), question.id.clone()))
			{
				state.visible.borrow_mut().clear();
			}
		}
		let mut panel = gpui::div().w_full().max_w(gpui::px(600.)).flex().flex_col().gap_3();
		if self.collapsed_async_questions.contains(&work.id) {
			return Some(
				panel
					.items_start()
					.child(self.async_question_toggle(&work.id, questions.len(), cx))
					.into_any_element(),
			);
		}
		for question in questions {
			if let Some(card) = self.async_question_card(work, question, cx) {
				panel = panel.child(card);
			}
		}
		if *questions_truncated && source.is_none() {
			panel = panel.child(agent_surface::muted(
				"More questions are available after these are answered.",
			));
		}
		Some(panel.into_any_element())
	}

	fn async_question_card(
		&self,
		work: &AgentWorkItemDto,
		question: &AgentAsyncQuestionDto,
		cx: &mut Context<Self>,
	) -> Option<AnyElement> {
		let key = (work.id.clone(), question.id.clone());
		let input = self.async_question_inputs.get(&key)?;
		let custom = question.options.is_empty()
			|| self.async_question_choices.get(&key).is_none_or(|state| {
				state
					.selected
					.as_deref()
					.is_none_or(|selected| selected != input.read(cx).content())
			});
		let busy = self.sending || self.uncertain;
		let can_send = !busy && !input.read(cx).content().trim().is_empty();
		let mut card = gpui::div().flex().flex_col().gap(gpui::px(2.)).child(
			gpui::div()
				.flex()
				.items_start()
				.gap_3()
				.mb_2()
				.child(
					gpui::div()
						.flex_1()
						.min_w_0()
						.font_weight(gpui::FontWeight::MEDIUM)
						.child(question.title.clone()),
				)
				.child(self.async_question_toggle(&work.id, 1, cx)),
		);
		for (index, option) in question.options.iter().enumerate() {
			card = card.child(self.async_choice_button(
				&key,
				index,
				Some(option),
				!custom && input.read(cx).content() == option,
				cx,
			));
		}
		if !question.options.is_empty() {
			card = card.child(self.async_choice_button(
				&key,
				question.options.len(),
				None,
				custom,
				cx,
			));
		}
		let owner = work.id.clone();
		let question_id = question.id.clone();
		let enter_owner = owner.clone();
		let enter_question = question_id.clone();
		let key_owner = owner.clone();
		let key_question = question_id.clone();
		card = card
			.key_context("AsyncQuestion")
			.on_key_down(cx.listener(move |s, event: &KeyDownEvent, _, cx| {
				s.handle_async_question_key(event, &key_owner, &key_question, cx);
			}))
			.on_action(cx.listener(move |s, _: &SubmitComposer, _, cx| {
				s.answer_async_question(&enter_owner, &enter_question, cx);
				cx.stop_propagation();
			}))
			.child(ui_motion::disclosure(
				SharedString::from(format!("async-custom-{}-{}", work.id, question.id)),
				custom,
				gpui::div().pt_2().h(gpui::px(48.)).child(input.clone()),
			))
			.child(self.async_question_actions(work, question, can_send, cx));
		Some(card.into_any_element())
	}

	fn async_question_actions(
		&self,
		work: &AgentWorkItemDto,
		question: &AgentAsyncQuestionDto,
		can_send: bool,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let owner = work.id.clone();
		let question_id = question.id.clone();
		let button_owner = owner.clone();
		let button_question = question_id.clone();
		let send_selector = format!("async-send-{question_id}");
		let busy = self.sending || self.uncertain;
		gpui::div()
			.flex()
			.items_center()
			.gap_2()
			.mt_2()
			.child(
				gpui::div()
					.id(SharedString::from(format!("async-send-{question_id}")))
					.debug_selector(move || send_selector)
					.role(Role::Button)
					.tab_index(0)
					.aria_label("Send answer")
					.aria_description(if can_send {
						"Send the selected answer"
					} else if busy {
						"Waiting for delivery confirmation"
					} else {
						"Write an answer first"
					})
					.px(gpui::px(10.))
					.h(gpui::px(28.))
					.min_w(gpui::px(82.))
					.flex()
					.items_center()
					.justify_center()
					.gap(gpui::px(8.))
					.rounded(gpui::px(6.))
					.bg(gpui::rgba(0xffffff12))
					.text_color(gpui::rgb(ui_theme::TEXT))
					.font_weight(gpui::FontWeight::MEDIUM)
					.opacity(if can_send { 1. } else { 0.45 })
					.cursor_pointer()
					.focus_visible(|style| style.bg(gpui::rgba(ui_theme::PRESSED_FILL)))
					.on_click(cx.listener(move |s, _, _, cx| {
						s.answer_async_question(&owner, &question_id, cx)
					}))
					.on_key_down(cx.listener(move |s, event: &KeyDownEvent, _, cx| {
						if matches!(event.keystroke.key.as_str(), "enter" | "space") {
							if !event.is_held {
								s.answer_async_question(&button_owner, &button_question, cx);
							}
							cx.stop_propagation();
						}
					}))
					.child(if self.sending { "Sending…" } else { "Send" })
					.child(workspace_symbols::icon_sized(Symbol::Forward, 12.))
					.smooth()
					.enabled(can_send),
			)
			.child(self.async_question_skip(work, &question.id, cx))
			.into_any_element()
	}
}

impl AgentSurface {
	fn handle_async_question_key(
		&mut self,
		event: &KeyDownEvent,
		owner: &str,
		question: &str,
		cx: &mut Context<Self>,
	) {
		let modifiers = event.keystroke.modifiers;

		if event.keystroke.key == "enter"
			&& !modifiers.shift
			&& !modifiers.control
			&& !modifiers.alt
		{
			if !event.is_held {
				self.answer_async_question(owner, question, cx);
			}

			cx.stop_propagation();
		}
	}
}

fn question_source(id: &str) -> Option<String> {
	let (kind, source, _): (String, String, usize) = serde_json::from_str(id).ok()?;
	(kind == "request_user_input_async").then_some(source)
}

#[cfg(test)]
mod tests {
	use gpui::Focusable;

	#[cfg(test)]
	use crate::shell::agent_surface::async_questions::{
		AgentDispatchStateDto, AgentSnapshotDto, AgentSnapshotResult, AgentWorkStatusDto,
	};
	use crate::shell::agent_surface::async_questions::{
		AgentHistoryResult, AgentSurface, AgentWorkItemDto, Context,
	};

	#[gpui::test]
	fn async_cold_restore_waits_for_history_and_prunes_resolved_questions(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		surface.update(visual, |s, cx| {
			install_question_fixture(s, cx);

			let key = ("root".into(), "q1".into());

			s.async_question_inputs[&key]
				.update(cx, |input, cx| input.set_content("Saved answer", cx));

			s.restored_question_drafts = s.capture_async_drafts(cx).unwrap();

			s.async_question_inputs.clear();
			s.async_question_choices.clear();

			let (_, mut history) = s.history.clone().unwrap();

			if let AgentHistoryResult::Available { questions_recovering, .. } = &mut history {
				*questions_recovering = true;
			}

			s.prepare_async_question_inputs("root", &history, cx);

			assert!(s.async_question_inputs.is_empty());
			assert_eq!(s.capture_async_drafts(cx).unwrap().len(), 1);

			if let AgentHistoryResult::Available { questions_recovering, .. } = &mut history {
				*questions_recovering = false;
			}

			s.prepare_async_question_inputs("root", &history, cx);

			assert_eq!(s.async_question_inputs[&key].read(cx).content(), "Saved answer");
			assert!(s.restored_question_drafts.is_empty());

			s.restored_question_drafts = s.capture_async_drafts(cx).unwrap();

			s.async_question_inputs.clear();
			s.async_question_choices.clear();

			if let AgentHistoryResult::Available { questions, questions_truncated, .. } =
				&mut history
			{
				questions.clear();

				*questions_truncated = true;
			}

			s.prepare_async_question_inputs("root", &history, cx);

			assert_eq!(s.restored_question_drafts.len(), 1);

			if let AgentHistoryResult::Available { questions_truncated, .. } = &mut history {
				*questions_truncated = false;
			}

			s.prepare_async_question_inputs("root", &history, cx);

			assert!(s.restored_question_drafts.is_empty());
			assert!(s.async_question_inputs.is_empty());
			assert!(!s.sending);
		});
	}
	#[gpui::test]
	fn async_question_card_follows_its_native_source_across_history_pages(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
		surface.update(visual, |s, cx| {
			install_question_fixture(s, cx);
			let (_, history) = s.history.as_mut().unwrap();
			let AgentHistoryResult::Available { questions, .. } = history else {
				panic!("fixture history");
			};
			questions[0].id = decodex_protocol::agent_async_question_id("question-source", 0);
			let history = history.clone();
			s.prepare_async_question_inputs("root", &history, cx);
			let work = s.snapshot.as_ref().unwrap().work_items[0].clone();
			assert!(s.render_async_questions(&work, None, cx).is_some());
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
			assert!(s.async_question_item_panel(&work, "question-source", cx).is_some());
			assert!(s.async_question_item_panel(&work, "other-message", cx).is_none());
			assert!(
				s.render_async_questions(&work, None, cx).is_none(),
				"one answer card at its source, no duplicate footer"
			);
			s.timeline.native.entries.clear();
			assert!(
				s.render_async_questions(&work, None, cx).is_some(),
				"paging out the source must not hide a pending question"
			);
		});
	}

	#[gpui::test]
	fn async_question_drafts_survive_refresh_and_other_work(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		surface.update(visual, |s, cx| {
			let question = |index| decodex_protocol::AgentAsyncQuestionDto {
				arrived_live: false,
				id: decodex_protocol::agent_async_question_id("message", index),
				title: "Same title".into(),
				options: vec!["Suggested".into()],
			};
			let first = question(0);
			let second = question(1);
			let mut history = AgentHistoryResult::Available {
				questions: vec![first.clone(), second.clone()],
				questions_truncated: false,
				questions_recovering: false,
				misalignment: None,
				usage: None,
				entries: vec![],
				has_more: false,
				next_before: None,
				live: vec![],
			};

			s.prepare_async_question_inputs("a", &history, cx);

			let key = ("a".to_owned(), second.id.clone());

			assert_eq!(s.async_question_inputs[&key].read(cx).content(), "Suggested");

			s.async_question_inputs[&key]
				.update(cx, |input, cx| input.set_content("My answer", cx));
			s.prepare_async_question_inputs("b", &history, cx);
			s.prepare_async_question_inputs("a", &history, cx);

			assert_eq!(s.async_question_inputs[&key].read(cx).content(), "My answer");
			assert_eq!(
				s.async_question_inputs[&("b".into(), second.id.clone())].read(cx).content(),
				"Suggested"
			);

			if let AgentHistoryResult::Available { questions, .. } = &mut history {
				questions.remove(0);
			}

			s.prepare_async_question_inputs("a", &history, cx);

			assert!(!s.async_question_inputs.contains_key(&("a".into(), first.id)));
			assert_eq!(s.async_question_inputs[&key].read(cx).content(), "My answer");

			s.prepare_async_question_inputs("a", &AgentHistoryResult::Unavailable, cx);

			assert_eq!(s.async_question_inputs[&key].read(cx).content(), "My answer");

			s.async_question_inputs[&key].update(cx, |input, cx| input.clear(cx));
			s.prepare_async_question_inputs("a", &history, cx);

			assert_eq!(s.async_question_inputs[&key].read(cx).content(), "");

			if let AgentHistoryResult::Available { questions, .. } = &mut history {
				questions[0].id = "free-text".into();

				questions[0].options.clear();
			}

			s.prepare_async_question_inputs("a", &history, cx);

			assert_eq!(
				s.async_question_inputs[&("a".into(), "free-text".into())].read(cx).content(),
				""
			);
			assert!(!s.sending);
			assert!(s.submission.command.is_none());
		});
	}
	#[gpui::test]
	fn async_option_click_and_explicit_submission_preserve_unaccepted_draft(
		cx: &mut gpui::TestAppContext,
	) {
		cx.update(crate::composer_input::bind_keys);

		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		surface.update(visual, install_question_fixture);
		visual.update(|window, cx| {
			window.resize(gpui::size(gpui::px(1_180.0), gpui::px(1_200.0)));
			window.draw(cx).clear(cx);
		});
		surface.read_with(visual, |s, cx| {
			assert_eq!(
				s.async_question_inputs[&("root".into(), "q1".into())].read(cx).content(),
				"PDF"
			);
			assert!(!s.sending);
			assert!(s.submission.command.is_none());
		});

		let bounds = visual.debug_bounds("async-option-q1-1").expect("visible async option");

		visual.simulate_click(bounds.center(), gpui::Modifiers::default());
		surface.update(visual, |s, cx| {
			assert_eq!(
				s.async_question_inputs[&("root".into(), "q1".into())].read(cx).content(),
				"Markdown"
			);
			assert!(!s.sending);
			assert!(s.submission.command.is_none());
		});

		let bounds = visual.debug_bounds("async-send-q1").expect("visible explicit send");

		visual.simulate_click(bounds.center(), gpui::Modifiers::default());

		let input = surface.update(visual, |s, cx| {
			assert_eq!(s.feedback, "No service profile is configured.");

			s.feedback.clear();

			let input = s.async_question_inputs[&("root".into(), "q1".into())].clone();

			assert_eq!(input.read(cx).content(), "Markdown");

			input
		});

		let option = visual.debug_bounds("async-option-q1-1").unwrap();
		visual.simulate_click(option.center(), gpui::Modifiers::default());
		visual.simulate_keystrokes("cmd-enter");
		surface.update(visual, |s, cx| {
			assert_eq!(s.feedback, "No service profile is configured.");
			assert_eq!(input.read(cx).content(), "Markdown");
			assert!(!s.sending);
		});
		surface.update(visual, |s, cx| {
			for literal in
				["!keep this literal", "/keep this literal", "?keep this literal", "x\nsecond line"]
			{
				input.update(cx, |input, cx| input.set_content(literal, cx));
				s.answer_async_question("root", "q1", cx);

				assert_eq!(s.feedback, "No service profile is configured.");
				assert_eq!(input.read(cx).content(), literal);
			}
		});
	}
	fn install_question_fixture(s: &mut AgentSurface, cx: &mut Context<AgentSurface>) {
		s.apply_result(Ok(AgentSnapshotResult::Available(AgentSnapshotDto {
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

		s.prepare_async_question_inputs("root", &history, cx);

		s.history = Some(("root".into(), history));
	}

	#[gpui::test]
	fn collapsed_questions_preserve_both_drafts_and_never_send(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		surface.update(visual, |s, cx| {
			install_question_fixture(s, cx);

			s.composer.update(cx, |input, cx| input.set_content("Main draft", cx));
			s.async_question_inputs[&("root".into(), "q1".into())]
				.update(cx, |input, cx| input.set_content("Answer draft", cx));
		});
		visual.update(|window, cx| {
			window.resize(gpui::size(gpui::px(1_180.0), gpui::px(1_200.0)));
			window.draw(cx).clear(cx);
		});

		let bounds = visual.debug_bounds("async-question-toggle").expect("return control");

		visual.simulate_click(bounds.center(), gpui::Modifiers::default());
		visual.update(|window, cx| {
			surface.read_with(cx, |s, cx| {
				assert!(s.collapsed_async_questions.contains("root"));
				assert!(!s.collapsed_async_questions.contains("other"));
				assert!(s.composer.focus_handle(cx).is_focused(window));
				assert!(!s.sending);
				assert!(s.submission.command.is_none());
			});
			window.draw(cx).clear(cx);
		});

		assert!(visual.debug_bounds("async-send-q1").is_none());

		surface.update(visual, |s, cx| {
			let history = s.history.as_ref().expect("history").1.clone();

			s.prepare_async_question_inputs("root", &history, cx);
		});

		let bounds = visual.debug_bounds("async-question-toggle").expect("reopen control");

		visual.simulate_click(bounds.center(), gpui::Modifiers::default());
		visual.update(|window, cx| {
			window.draw(cx).clear(cx);
		});

		assert!(visual.debug_bounds("async-send-q1").is_some());

		surface.read_with(visual, |s, cx| {
			assert_eq!(s.composer.read(cx).content(), "Main draft");
			assert_eq!(
				s.async_question_inputs[&("root".into(), "q1".into())].read(cx).content(),
				"Answer draft"
			);
			assert!(!s.sending);
			assert!(s.submission.command.is_none());
		});
	}
	#[gpui::test]
	fn rejected_skip_preserves_question_and_main_drafts(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		surface.update(visual, |s, cx| {
			install_question_fixture(s, cx);

			s.composer.update(cx, |input, cx| input.set_content("Keep main draft", cx));
		});
		visual.update(|window, cx| {
			window.resize(gpui::size(gpui::px(1_180.0), gpui::px(1_200.0)));
			window.draw(cx).clear(cx);
		});

		let bounds = visual.debug_bounds("async-skip-q1").expect("explicit skip control");

		visual.simulate_click(bounds.center(), gpui::Modifiers::default());
		surface.read_with(visual, |s, cx| {
			assert_eq!(s.feedback, "No service profile is configured.");
			assert_eq!(s.composer.read(cx).content(), "Keep main draft");
			assert_eq!(
				s.async_question_inputs[&("root".into(), "q1".into())].read(cx).content(),
				"PDF"
			);
			assert!(!s.sending);
			assert!(s.submission.command.is_none());
		});
	}
	#[gpui::test]
	fn custom_answer_survives_choice_switches_with_its_editor(cx: &mut gpui::TestAppContext) {
		cx.update(crate::composer_input::bind_keys);

		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		surface.update(visual, install_question_fixture);
		visual.update(|window, cx| {
			window.resize(gpui::size(gpui::px(1_180.0), gpui::px(1_200.0)));
			window.draw(cx).clear(cx);
		});

		let key = ("root".into(), "q1".into());
		let bounds = visual.debug_bounds("async-option-q1-2").expect("custom answer option");

		visual.simulate_click(bounds.center(), gpui::Modifiers::default());

		let custom = surface.update(visual, |s, cx| {
			let input = s.async_question_inputs[&key].clone();

			assert_eq!(input.read(cx).content(), "");

			input
		});

		visual.simulate_keystrokes("m y space c u s t o m space a n s w e r");

		let bounds = visual.debug_bounds("async-option-q1-1").expect("suggested answer");

		visual.simulate_click(bounds.center(), gpui::Modifiers::default());

		let selected_input =
			surface.read_with(visual, |s, _| s.async_question_inputs[&key].clone());

		visual.simulate_keystrokes("space");
		surface.read_with(visual, |s, cx| {
			assert_ne!(
				s.async_question_inputs[&key], selected_input,
				"keyboard activates the option without submitting"
			);
			assert_eq!(s.async_question_inputs[&key].read(cx).content(), "Markdown");
			assert_eq!(custom.read(cx).content(), "my custom answer");
			assert!(!s.sending);
			assert!(s.feedback.is_empty());
		});

		let bounds = visual.debug_bounds("async-option-q1-2").expect("restore custom answer");

		visual.simulate_click(bounds.center(), gpui::Modifiers::default());
		surface.read_with(visual, |s, cx| {
			assert_eq!(
				s.async_question_inputs[&key], custom,
				"retain editor, cursor and undo state"
			);
			assert_eq!(custom.read(cx).content(), "my custom answer");
		});
	}

	#[gpui::test]
	fn held_enter_cannot_submit_an_async_answer(cx: &mut gpui::TestAppContext) {
		cx.update(crate::composer_input::bind_keys);

		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		surface.update(visual, |s, cx| {
			install_question_fixture(s, cx);
			s.async_question_choices.get_mut(&("root".into(), "q1".into())).unwrap().selected =
				None;
			s.async_question_inputs[&("root".into(), "q1".into())]
				.update(cx, |input, cx| input.set_content("My answer", cx));
		});

		visual.update(|window, cx| {
			window.resize(gpui::size(gpui::px(1_180.0), gpui::px(1_200.0)));

			let focus = surface.read_with(cx, |s, cx| {
				s.async_question_inputs[&("root".into(), "q1".into())].focus_handle(cx)
			});

			window.focus(&focus, cx);
			window.draw(cx).clear(cx);
		});

		visual.simulate_event(gpui::KeyDownEvent {
			keystroke: gpui::Keystroke::parse("enter").unwrap(),
			is_held: true,
			prefer_character_input: false,
		});
		surface.read_with(visual, |s, _| {
			assert!(s.feedback.is_empty(), "a held key must not reach command submission");
			assert!(!s.sending);
			assert!(s.submission.command.is_none());
		});
		visual.simulate_keystrokes("enter");
		surface
			.read_with(visual, |s, _| assert_eq!(s.feedback, "No service profile is configured."));
	}
	#[gpui::test]
	fn suggested_answer_requires_current_full_visibility_after_resize_and_scroll(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
		let label = format!("{} but do not deploy", "Read the full proposed change. ".repeat(20));

		surface.update(visual, |s, cx| {
			install_question_fixture(s, cx);

			let (_, history) = s.history.as_mut().unwrap();
			let AgentHistoryResult::Available { questions, .. } = history else {
				panic!("fixture history");
			};

			questions[0].options = vec![label.clone()];

			for index in 2..12 {
				questions.push(decodex_protocol::AgentAsyncQuestionDto {
					arrived_live: false,
					id: format!("q{index}"),
					title: format!("Question {index}"),
					options: vec!["One".into(), "Two".into()],
				});
			}

			let history = history.clone();

			s.prepare_async_question_inputs("root", &history, cx);
			s.async_question_inputs[&("root".into(), "q1".into())]
				.update(cx, |input, cx| input.set_content(&label, cx));
		});

		for (width, height, bottom, allowed) in [
			(1_180.0, 1_200.0, false, true),
			(1_180.0, 1_200.0, true, false),
			(380.0, 160.0, false, false),
			(380.0, 1_200.0, false, true),
			(1_180.0, 1_200.0, false, true),
		] {
			visual.simulate_resize(gpui::size(gpui::px(width), gpui::px(height)));

			visual.update(|window, cx| {
				assert_eq!(window.viewport_size(), gpui::size(gpui::px(width), gpui::px(height)));

				window.draw(cx).clear(cx);

				surface.update(cx, |s, cx| {
					let scroll =
						s.timeline.scroll.get("root").expect("rendered conversation scroll");

					if bottom {
						scroll.scroll_to_bottom();
					} else {
						scroll.set_offset(gpui::point(gpui::px(0.0), gpui::px(0.0)));
					}

					s.feedback.clear();
					cx.notify();
				});

				window.draw(cx).clear(cx);
			});

			surface.update(visual, |s, cx| {
				s.answer_async_question("root", "q1", cx);

				assert_eq!(
					s.feedback,
					if allowed {
						"No service profile is configured."
					} else {
						"Show the entire suggested answer before sending."
					},
					"height={height}, bottom={bottom}"
				);
				assert_eq!(
					s.async_question_inputs[&("root".into(), "q1".into())].read(cx).content(),
					label
				);
				assert!(!s.sending);
				assert!(s.submission.command.is_none());
			});
		}
	}
	#[gpui::test]
	fn changed_native_thread_cannot_inherit_question_drafts(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		surface.update(visual, |s, cx| {
			install_question_fixture(s, cx);

			let key = ("root".into(), "q1".into());
			let old = s.async_question_inputs[&key].clone();

			old.update(cx, |input, cx| input.set_content("Private old-thread draft", cx));

			let history = s.history.as_ref().unwrap().1.clone();

			s.snapshot.as_mut().unwrap().work_items[0].codex_thread_id = Some("new-thread".into());

			s.prepare_async_question_inputs("root", &history, cx);

			assert_ne!(s.async_question_inputs[&key], old);
			assert_eq!(s.async_question_inputs[&key].read(cx).content(), "PDF");

			let mut resolved = history.clone();
			let AgentHistoryResult::Available { questions, .. } = &mut resolved else {
				panic!("fixture");
			};

			questions.clear();
			s.prepare_async_question_inputs("root", &resolved, cx);

			assert!(!s.async_question_inputs.contains_key(&key));
			assert!(!s.async_question_choices.contains_key(&key));
		});
	}
}
