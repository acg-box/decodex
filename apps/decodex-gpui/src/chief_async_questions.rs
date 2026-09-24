//! Nonblocking model questions are explicit user messages, not approval callbacks.
use super::*;

#[derive(Default)]
pub(super) struct ChoiceDraft {
	selected: Option<String>,
	custom: Option<Entity<ComposerInput>>,
	visible:
		std::cell::RefCell<std::collections::BTreeMap<String, std::rc::Rc<std::cell::Cell<bool>>>>,
}

impl ChiefSurface {
	pub(super) fn prepare_async_question_inputs(
		&mut self,
		work: &str,
		history: &ChiefHistoryResult,
		cx: &mut Context<Self>,
	) {
		let ChiefHistoryResult::Available {
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
						ComposerInput::with_placeholder(40, "Your answer", "Answer to Chief", cx)
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
		questions: &[decodex_protocol::ChiefAsyncQuestionDto],
		truncated: bool,
		cx: &mut Context<Self>,
	) {
		let Some(thread) = self.async_question_threads.get(work).cloned() else {
			return;
		};
		for saved in std::mem::take(&mut self.restored_question_drafts) {
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
						"Answer to Chief",
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
		let Some(current) = self.async_question_inputs.get(key).cloned() else {
			return;
		};
		let state = self.async_question_choices.entry(key.clone()).or_default();
		if state.selected.as_deref() != Some(current.read(cx).content()) {
			state.custom = Some(current);
		}
		let input = if let Some(option) = option {
			let input = cx.new(|cx| {
				ComposerInput::with_placeholder(40, "Your answer", "Answer to Chief", cx)
			});
			input.update(cx, |input, cx| input.set_content(option, cx));
			input
		} else {
			state
				.custom
				.get_or_insert_with(|| {
					cx.new(|cx| {
						ComposerInput::with_placeholder(
							40,
							"Write an answer",
							"Answer to Chief",
							cx,
						)
					})
				})
				.clone()
		};
		state.selected = option.map(str::to_owned);
		if option.is_none() {
			use gpui::Focusable;
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
	) -> gpui::AnyElement {
		let visible = std::rc::Rc::new(std::cell::Cell::new(false));
		if let Some(option) = option
			&& let Some(state) = self.async_question_choices.get(key)
		{
			state.visible.borrow_mut().insert(option.into(), visible.clone());
		}
		let label = option.unwrap_or("Other (write an answer)").to_owned();
		let option = option.map(str::to_owned);
		let key_option = option.clone();
		let owner = key.clone();
		let key_owner = key.clone();
		let selector = format!("async-option-{}-{index}", key.1);
		div()
			.id(SharedString::from(selector.clone()))
			.debug_selector(move || selector)
			.relative()
			.min_w_0()
			.whitespace_normal()
			.role(Role::Button)
			.tab_index(0)
			.aria_label(label.clone())
			.p_2()
			.rounded(px(6.0))
			.bg(rgba(if selected { 0xffffff18 } else { 0xffffff06 }))
			.cursor_pointer()
			.on_click(cx.listener(move |s, _, window, cx| {
				s.select_async_choice(&owner, option.as_deref(), window, cx)
			}))
			.on_key_down(cx.listener(move |s, event: &gpui::KeyDownEvent, window, cx| {
				if matches!(event.keystroke.key.as_str(), "enter" | "space") {
					if !event.is_held {
						s.select_async_choice(&key_owner, key_option.as_deref(), window, cx);
					}
					cx.stop_propagation();
				}
			}))
			.child(label)
			.child(
				gpui::canvas(
					move |bounds, window, _| {
						let mask = window.content_mask().bounds.intersect(&gpui::Bounds::new(
							gpui::Point::default(),
							window.viewport_size(),
						));
						visible.set(
							bounds.size.width > px(0.0)
								&& bounds.size.height > px(0.0)
								&& mask.intersect(&bounds) == bounds,
						);
					},
					|_, _, _, _| {},
				)
				.absolute()
				.inset_0(),
			)
			.into_any_element()
	}

	fn answer_async_question(&mut self, work: &str, question: &str, cx: &mut Context<Self>) {
		if self.sending || self.uncertain {
			return;
		}
		let Some(input) = self.async_question_inputs.get(&(work.into(), question.into())) else {
			return;
		};
		let Ok(answer) =
			decodex_protocol::HistoryText::new(input.read(cx).content().trim().to_owned())
		else {
			return;
		};
		if answer.as_str().is_empty() {
			return;
		}
		let Some((owner, ChiefHistoryResult::Available { questions, .. })) = &self.history else {
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
		if let Err(message) = decodex_protocol::chief_async_question_reply(source, answer.as_str())
		{
			self.feedback = message;
			cx.notify();
			return;
		}
		let (Ok(work_id), Ok(question_id)) =
			(EntityId::new(work), decodex_protocol::WireText::new(question))
		else {
			return;
		};
		self.execute(ChiefActionDto::AnswerQuestion { work_id, question_id, answer }, None, cx);
	}

	fn async_question_skip(
		&self,
		work: &ChiefWorkItemDto,
		question: &str,
		cx: &mut Context<Self>,
	) -> gpui::AnyElement {
		let Some(thread) = &work.codex_thread_id else {
			return div().into_any_element();
		};
		let (Ok(work_id), Ok(thread_id), Ok(question_id)) = (
			EntityId::new(&work.id),
			decodex_protocol::WireText::new(thread),
			decodex_protocol::WireText::new(question),
		) else {
			return div().into_any_element();
		};
		let action = ChiefActionDto::SkipQuestion { work_id, thread_id, question_id };
		let key_action = action.clone();
		let selector = format!("async-skip-{question}");
		div()
			.id(SharedString::from(selector.clone()))
			.debug_selector(move || selector)
			.role(Role::Button)
			.tab_index(0)
			.aria_label("Skip question")
			.p_2()
			.cursor_pointer()
			.on_click(cx.listener(move |s, _, _, cx| {
				if !s.sending && !s.uncertain {
					s.execute(action.clone(), None, cx);
				}
			}))
			.on_key_down(cx.listener(move |s, event: &gpui::KeyDownEvent, _, cx| {
				if matches!(event.keystroke.key.as_str(), "enter" | "space") && !event.is_held {
					if !s.sending && !s.uncertain {
						s.execute(key_action.clone(), None, cx);
					}
					cx.stop_propagation();
				}
			}))
			.child("Skip question")
			.into_any_element()
	}

	fn toggle_async_questions(&mut self, work: &str, window: &mut Window, cx: &mut Context<Self>) {
		use gpui::Focusable;
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
	) -> gpui::AnyElement {
		let collapsed = self.collapsed_async_questions.contains(work);
		let label = if collapsed {
			format!("Show questions ({count})")
		} else {
			"Return to message".into()
		};
		let owner = work.to_owned();
		let key_owner = owner.clone();
		div()
			.id("async-question-toggle")
			.debug_selector(|| "async-question-toggle".into())
			.role(Role::Button)
			.tab_index(0)
			.aria_label(label.clone())
			.p_2()
			.cursor_pointer()
			.on_click(
				cx.listener(move |s, _, window, cx| s.toggle_async_questions(&owner, window, cx)),
			)
			.on_key_down(cx.listener(move |s, event: &gpui::KeyDownEvent, window, cx| {
				if matches!(event.keystroke.key.as_str(), "enter" | "space") && !event.is_held {
					s.toggle_async_questions(&key_owner, window, cx);
					cx.stop_propagation();
				}
			}))
			.child(label)
			.into_any_element()
	}

	pub(super) fn async_question_panel(
		&self,
		work: &ChiefWorkItemDto,
		cx: &mut Context<Self>,
	) -> gpui::AnyElement {
		for (key, state) in &self.async_question_choices {
			if key.0 == work.id {
				state.visible.borrow_mut().clear();
			}
		}
		let Some((
			id,
			ChiefHistoryResult::Available {
				questions,
				questions_truncated,
				questions_recovering,
				..
			},
		)) = &self.history
		else {
			return div().into_any_element();
		};
		if id != &work.id {
			return div().into_any_element();
		}
		let mut panel = div().flex().flex_col().gap_3();
		if *questions_recovering || questions.is_empty() {
			return div().into_any_element();
		}
		panel = panel.child(self.async_question_toggle(&work.id, questions.len(), cx));
		if self.collapsed_async_questions.contains(&work.id) {
			return panel.into_any_element();
		}
		for question in questions {
			let key = (work.id.clone(), question.id.clone());
			let Some(input) = self.async_question_inputs.get(&key) else {
				continue;
			};
			let mut card = div()
				.p_3()
				.rounded(px(8.0))
				.border_1()
				.border_color(rgba(0xffffff18))
				.flex()
				.flex_col()
				.gap_2()
				.child(question.title.clone());
			for (index, option) in question.options.iter().enumerate() {
				card = card.child(self.async_choice_button(
					&key,
					index,
					Some(option),
					input.read(cx).content() == option,
					cx,
				));
			}
			if !question.options.is_empty() {
				card = card.child(
					self.async_choice_button(
						&key,
						question.options.len(),
						None,
						self.async_question_choices
							.get(&key)
							.is_some_and(|state| state.selected.is_none()),
						cx,
					),
				);
			}
			let owner = work.id.clone();
			let question_id = question.id.clone();
			let enter_owner = owner.clone();
			let enter_question = question_id.clone();
			let send_selector = format!("async-send-{question_id}");
			let key_owner = owner.clone();
			let key_question = question_id.clone();
			card =
				card.key_context("AsyncQuestion")
					.on_key_down(cx.listener(move |s, event: &gpui::KeyDownEvent, _, cx| {
						let modifiers = event.keystroke.modifiers;
						if event.keystroke.key == "enter"
							&& !modifiers.shift && !modifiers.control
							&& !modifiers.alt
						{
							if !event.is_held {
								s.answer_async_question(&key_owner, &key_question, cx);
							}
							cx.stop_propagation();
						}
					}))
					.on_action(cx.listener(move |s, _: &SubmitComposer, _, cx| {
						s.answer_async_question(&enter_owner, &enter_question, cx);
						cx.stop_propagation();
					}))
					.child(div().h(px(40.0)).child(input.clone()))
					.child(
						div()
							.id(SharedString::from(format!("async-send-{question_id}")))
							.debug_selector(move || send_selector)
							.role(Role::Button)
							.tab_index(0)
							.aria_label("Send answer")
							.p_2()
							.cursor_pointer()
							.on_click(cx.listener(move |s, _, _, cx| {
								s.answer_async_question(&owner, &question_id, cx)
							}))
							.child("Send answer"),
					);
			panel = panel.child(card.child(self.async_question_skip(work, &question.id, cx)));
		}
		if *questions_truncated {
			panel = panel.child(muted("More questions are available after these are answered."));
		}
		panel.into_any_element()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	#[gpui::test]
	fn async_cold_restore_waits_for_history_and_prunes_resolved_questions(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		surface.update(visual, |s, cx| {
			install_question_fixture(s, cx);
			let key = ("root".into(), "q1".into());
			s.async_question_inputs[&key]
				.update(cx, |input, cx| input.set_content("Saved answer", cx));
			s.restored_question_drafts = s.capture_async_drafts(cx).unwrap();
			s.async_question_inputs.clear();
			s.async_question_choices.clear();
			let (_, mut history) = s.history.clone().unwrap();
			if let ChiefHistoryResult::Available { questions_recovering, .. } = &mut history {
				*questions_recovering = true;
			}
			s.prepare_async_question_inputs("root", &history, cx);
			assert!(s.async_question_inputs.is_empty());
			assert_eq!(s.capture_async_drafts(cx).unwrap().len(), 1);
			if let ChiefHistoryResult::Available { questions_recovering, .. } = &mut history {
				*questions_recovering = false;
			}
			s.prepare_async_question_inputs("root", &history, cx);
			assert_eq!(s.async_question_inputs[&key].read(cx).content(), "Saved answer");
			assert!(s.restored_question_drafts.is_empty());
			s.restored_question_drafts = s.capture_async_drafts(cx).unwrap();
			s.async_question_inputs.clear();
			s.async_question_choices.clear();
			if let ChiefHistoryResult::Available { questions, questions_truncated, .. } =
				&mut history
			{
				questions.clear();
				*questions_truncated = true;
			}
			s.prepare_async_question_inputs("root", &history, cx);
			assert_eq!(s.restored_question_drafts.len(), 1);
			if let ChiefHistoryResult::Available { questions_truncated, .. } = &mut history {
				*questions_truncated = false;
			}
			s.prepare_async_question_inputs("root", &history, cx);
			assert!(s.restored_question_drafts.is_empty());
			assert!(s.async_question_inputs.is_empty());
			assert!(!s.sending);
		});
	}
	#[gpui::test]
	fn async_question_drafts_survive_refresh_and_other_work(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		surface.update(visual, |s, cx| {
			let question = |index| decodex_protocol::ChiefAsyncQuestionDto {
				arrived_live: false,
				id: decodex_protocol::chief_async_question_id("message", index),
				title: "Same title".into(),
				options: vec!["Suggested".into()],
			};
			let first = question(0);
			let second = question(1);
			let mut history = ChiefHistoryResult::Available {
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
			if let ChiefHistoryResult::Available { questions, .. } = &mut history {
				questions.remove(0);
			}
			s.prepare_async_question_inputs("a", &history, cx);
			assert!(!s.async_question_inputs.contains_key(&("a".into(), first.id)));
			assert_eq!(s.async_question_inputs[&key].read(cx).content(), "My answer");
			s.prepare_async_question_inputs("a", &ChiefHistoryResult::Unavailable, cx);
			assert_eq!(s.async_question_inputs[&key].read(cx).content(), "My answer");
			s.async_question_inputs[&key].update(cx, |input, cx| input.clear(cx));
			s.prepare_async_question_inputs("a", &history, cx);
			assert_eq!(s.async_question_inputs[&key].read(cx).content(), "");
			if let ChiefHistoryResult::Available { questions, .. } = &mut history {
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
		use gpui::Focusable;
		cx.update(crate::composer_input::bind_keys);
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		surface.update(visual, install_question_fixture);
		visual.update(|window, cx| {
			window.resize(gpui::size(px(1180.0), px(1200.0)));
			window.draw(cx).clear();
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
		visual.update(|window, cx| {
			window.focus(&input.focus_handle(cx), cx);
			window.draw(cx).clear();
		});
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
	fn install_question_fixture(s: &mut ChiefSurface, cx: &mut Context<ChiefSurface>) {
		s.apply_result(Ok(ChiefSnapshotResult::Available(ChiefSnapshotDto {
			runtime_source: None,
			workspaces: vec![],
			dependencies: vec![],
			pending_events: vec![],
			work_items: vec![ChiefWorkItemDto {
				id: "root".into(),
				parent_goal_id: None,
				kind: decodex_protocol::ChiefWorkKindDto::Goal,
				title: "Chief".into(),
				codex_thread_id: Some("thread".into()),
				active_turn_id: Some("turn".into()),
				dispatch_state: ChiefDispatchStateDto::Running,
				status: ChiefWorkStatusDto::Open,
				next_check_at_micros: None,
				created_at_micros: 1,
				updated_at_micros: 1,
			}],
		})));
		let history = ChiefHistoryResult::Available {
			questions: vec![decodex_protocol::ChiefAsyncQuestionDto {
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
		use gpui::Focusable;
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		surface.update(visual, |s, cx| {
			install_question_fixture(s, cx);
			s.composer.update(cx, |input, cx| input.set_content("Main draft", cx));
			s.async_question_inputs[&("root".into(), "q1".into())]
				.update(cx, |input, cx| input.set_content("Answer draft", cx));
		});
		visual.update(|window, cx| {
			window.resize(gpui::size(px(1180.0), px(1200.0)));
			window.draw(cx).clear();
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
			window.draw(cx).clear();
		});
		assert!(visual.debug_bounds("async-send-q1").is_none());
		surface.update(visual, |s, cx| {
			let history = s.history.as_ref().expect("history").1.clone();
			s.prepare_async_question_inputs("root", &history, cx);
		});
		let bounds = visual.debug_bounds("async-question-toggle").expect("reopen control");
		visual.simulate_click(bounds.center(), gpui::Modifiers::default());
		visual.update(|window, cx| {
			window.draw(cx).clear();
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
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		surface.update(visual, |s, cx| {
			install_question_fixture(s, cx);
			s.composer.update(cx, |input, cx| input.set_content("Keep main draft", cx));
		});
		visual.update(|window, cx| {
			window.resize(gpui::size(px(1180.0), px(1200.0)));
			window.draw(cx).clear();
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
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		surface.update(visual, install_question_fixture);
		visual.update(|window, cx| {
			window.resize(gpui::size(px(1180.0), px(1200.0)));
			window.draw(cx).clear();
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
		use gpui::Focusable;
		cx.update(crate::composer_input::bind_keys);
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		surface.update(visual, install_question_fixture);
		visual.update(|window, cx| {
			window.resize(gpui::size(px(1180.0), px(1200.0)));
			let focus = surface.read_with(cx, |s, cx| {
				s.async_question_inputs[&("root".into(), "q1".into())].focus_handle(cx)
			});
			window.focus(&focus, cx);
			window.draw(cx).clear();
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
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		let label = format!("{} but do not deploy", "Read the full proposed change. ".repeat(20));
		surface.update(visual, |s, cx| {
			install_question_fixture(s, cx);
			let (_, history) = s.history.as_mut().unwrap();
			let ChiefHistoryResult::Available { questions, .. } = history else {
				panic!("fixture history");
			};
			questions[0].options = vec![label.clone()];
			for index in 2..12 {
				questions.push(decodex_protocol::ChiefAsyncQuestionDto {
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
			(1180.0, 1200.0, false, true),
			(1180.0, 1200.0, true, false),
			(380.0, 160.0, false, false),
			(380.0, 1200.0, false, true),
			(1180.0, 1200.0, false, true),
		] {
			visual.simulate_resize(gpui::size(px(width), px(height)));
			visual.update(|window, cx| {
				assert_eq!(window.viewport_size(), gpui::size(px(width), px(height)));
				window.draw(cx).clear();
				surface.update(cx, |s, cx| {
					let scroll =
						s.transcript_scroll.get("root").expect("rendered conversation scroll");
					if bottom {
						scroll.scroll_to_bottom();
					} else {
						scroll.set_offset(gpui::point(px(0.0), px(0.0)));
					}
					s.feedback.clear();
					cx.notify();
				});
				window.draw(cx).clear();
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
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
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
			let ChiefHistoryResult::Available { questions, .. } = &mut resolved else {
				panic!("fixture");
			};
			questions.clear();
			s.prepare_async_question_inputs("root", &resolved, cx);
			assert!(!s.async_question_inputs.contains_key(&key));
			assert!(!s.async_question_choices.contains_key(&key));
		});
	}
}
