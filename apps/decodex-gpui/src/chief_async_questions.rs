//! Nonblocking model questions are explicit user messages, not approval callbacks.
use super::*;

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
		if !questions_truncated && !questions_recovering {
			self.async_question_inputs.retain(|(owner, id), _| {
				owner != work || questions.iter().any(|question| &question.id == id)
			});
		}
		for question in questions {
			self.async_question_inputs.entry((work.into(), question.id.clone())).or_insert_with(
				|| {
					cx.new(|cx| {
						ComposerInput::with_placeholder(40, "Your answer", "Answer to Chief", cx)
					})
				},
			);
		}
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

	pub(super) fn async_question_panel(
		&self,
		work: &ChiefWorkItemDto,
		cx: &mut Context<Self>,
	) -> gpui::AnyElement {
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
		if *questions_recovering {
			return panel
				.child(muted("Questions are being restored from conversation history."))
				.into_any_element();
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
				let input = input.clone();
				let answer = option.clone();
				let selected = input.read(cx).content() == option;
				let selector = format!("async-option-{}-{index}", question.id);
				card = card.child(
					div()
						.id(SharedString::from(format!("async-option-{}-{index}", question.id)))
						.debug_selector(move || selector)
						.role(Role::Button)
						.tab_index(0)
						.aria_label(option.clone())
						.p_2()
						.rounded(px(6.0))
						.bg(rgba(if selected { 0xffffff18 } else { 0xffffff06 }))
						.cursor_pointer()
						.on_click(cx.listener(move |_, _, _, cx| {
							input.update(cx, |input, cx| input.set_content(&answer, cx));
							cx.notify();
						}))
						.child(option.clone()),
				);
			}
			let owner = work.id.clone();
			let question_id = question.id.clone();
			let enter_owner = owner.clone();
			let enter_question = question_id.clone();
			let send_selector = format!("async-send-{question_id}");
			card = card
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
			panel = panel.child(card);
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
	fn async_question_drafts_survive_refresh_and_other_work(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		surface.update(visual, |s, cx| {
			let question = |index| decodex_protocol::ChiefAsyncQuestionDto {
				id: decodex_protocol::chief_async_question_id("message", index),
				title: "Same title".into(),
				options: vec!["Suggested".into()],
			};
			let first = question(0);
			let second = question(1);
			let mut history = ChiefHistoryResult::Available {
				questions: vec![first.clone(), second.clone()],
				questions_truncated: false,
				questions_recovering: false, misalignment: None,
				usage: None,
				entries: vec![],
				has_more: false,
				next_before: None,
				live: vec![],
			};
			s.prepare_async_question_inputs("a", &history, cx);
			let key = ("a".to_owned(), second.id.clone());
			assert_eq!(s.async_question_inputs[&key].read(cx).content(), "");
			s.async_question_inputs[&key]
				.update(cx, |input, cx| input.set_content("My answer", cx));
			s.prepare_async_question_inputs("b", &history, cx);
			s.prepare_async_question_inputs("a", &history, cx);
			assert_eq!(s.async_question_inputs[&key].read(cx).content(), "My answer");
			assert_eq!(
				s.async_question_inputs[&("b".into(), second.id.clone())].read(cx).content(),
				""
			);
			if let ChiefHistoryResult::Available { questions, .. } = &mut history {
				questions.remove(0);
			}
			s.prepare_async_question_inputs("a", &history, cx);
			assert!(!s.async_question_inputs.contains_key(&("a".into(), first.id)));
			assert_eq!(s.async_question_inputs[&key].read(cx).content(), "My answer");
			s.prepare_async_question_inputs("a", &ChiefHistoryResult::Unavailable, cx);
			assert_eq!(s.async_question_inputs[&key].read(cx).content(), "My answer");
		});
	}
	#[gpui::test]
	fn async_option_click_and_explicit_submission_preserve_unaccepted_draft(
		cx: &mut gpui::TestAppContext,
	) {
		use gpui::Focusable;
		cx.update(crate::composer_input::bind_keys);
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		surface.update(visual, |s, cx| {
			s.apply_result(Ok(ChiefSnapshotResult::Available(ChiefSnapshotDto {
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
					id: "q1".into(),
					title: "Choose a format".into(),
					options: vec!["PDF".into(), "Markdown".into()],
				}],
				questions_truncated: false,
				questions_recovering: false, misalignment: None,
				usage: None,
				entries: vec![],
				has_more: false,
				next_before: None,
				live: vec![],
			};
			s.prepare_async_question_inputs("root", &history, cx);
			s.history = Some(("root".into(), history));
		});
		visual.update(|window, cx| {
			window.resize(gpui::size(px(1180.0), px(1200.0)));
			window.draw(cx).clear();
		});
		let bounds = visual.debug_bounds("async-option-q1-1").expect("visible async option");
		visual.simulate_click(bounds.center(), gpui::Modifiers::default());
		surface.update(visual, |s, cx| {
			assert_eq!(
				s.async_question_inputs[&("root".into(), "q1".into())].read(cx).content(),
				"Markdown"
			);
			assert!(!s.sending);
			assert!(s.command_task.is_none());
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
	}
}
