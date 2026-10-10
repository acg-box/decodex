//! Explicit task selection and per-manager composer drafts.
use gpui::{AnyElement, AppContext as _, KeyDownEvent};

#[cfg(test)] use crate::shell::agent_surface::composer::{AgentActionDto, HistoryText};
use crate::{
	shell::agent_surface::composer::{
		AgentSurface, ComposerInput, Context, Entity, EntityId, InteractiveElement, IntoElement,
		ParentElement, Role, SharedString, SmoothControl, StatefulInteractiveElement, Styled,
		ui_theme::TEXT_MUTED,
	},
	ui_theme::HOVER_FILL,
};
use decodex_protocol::{AgentTaskReferenceDto, WireText};

impl AgentSurface {
	pub(crate) fn visual_task_references(&mut self) {
		self.workspace.graph_visible = false;

		if let Some(snapshot) = &mut self.snapshot {
			for work in &mut snapshot.work_items {
				work.codex_thread_id = Some(format!("fixture-thread-{}", work.id));
			}
		}

		self.task_references = vec![decodex_protocol::AgentTaskReferenceDto {
			work_id: EntityId::new("verify").expect("fixture"),
			thread_id: WireText::new("fixture-thread-verify").expect("fixture"),
			title: WireText::new("Verify compatibility").expect("fixture"),
		}];
		self.composer_menu = Some("tasks");
		self.composer_menu_content = Some("tasks");
	}

	pub(crate) fn new_task_reference_search(cx: &mut Context<Self>) -> Entity<ComposerInput> {
		let search = cx.new(|cx| {
			ComposerInput::with_placeholder(45, "Search tasks", "Search task references", cx)
		});

		cx.observe(&search, |_, _, cx| cx.notify()).detach();

		search
	}

	pub(crate) fn clear_sent_task_references(
		&mut self,
		sent: &[AgentTaskReferenceDto],
		same_owner: bool,
		owner: Option<&str>,
	) {
		if same_owner {
			self.task_references.retain(|reference| !sent.contains(reference));
		} else if let Some(draft) = owner.and_then(|id| self.draft_profiles.tasks.get_mut(id)) {
			draft.retain(|reference| !sent.contains(reference));
		}
	}

	fn select_task_reference(&mut self, reference: AgentTaskReferenceDto, cx: &mut Context<Self>) {
		if self.sending || self.uncertain {
			return;
		}
		if self
			.task_references
			.iter()
			.any(|r| r.work_id == reference.work_id && r.thread_id == reference.thread_id)
		{
			return;
		}
		if self.task_references.len() >= 16 {
			self.feedback = "Select at most 16 tasks.".into();
		} else {
			self.task_references.push(reference);

			self.composer_menu = None;
		}

		cx.notify();
	}

	pub(super) fn task_reference_options(&self, cx: &mut Context<Self>) -> AnyElement {
		let query = self.task_reference_search.read(cx).content().trim().to_lowercase();
		let mut list = gpui::div()
			.id("task-reference-results")
			.max_h(gpui::px(280.))
			.overflow_y_scroll()
			.flex()
			.flex_col()
			.gap_1();
		let mut count = 0;

		if let Some(snapshot) = &self.snapshot {
			for work in snapshot
				.work_items
				.iter()
				.filter(|w| {
					w.codex_thread_id.is_some()
						&& (query.is_empty()
							|| w.title.to_lowercase().contains(&query)
							|| w.id.to_lowercase().contains(&query))
				})
				.take(50)
			{
				let (Ok(work_id), Ok(thread_id), Ok(title)) = (
					EntityId::new(work.id.clone()),
					WireText::new(work.codex_thread_id.clone().expect("filtered bound task")),
					WireText::new(work.title.chars().take(160).collect::<String>()),
				) else {
					continue;
				};
				let reference = AgentTaskReferenceDto { work_id, thread_id, title };
				let clicked = reference.clone();

				list = list.child(
					gpui::div()
						.id(SharedString::from(format!("reference-task-{}", work.id)))
						.debug_selector({
							let id = format!("reference-task-{}", work.id);

							move || id.clone()
						})
						.role(Role::Button)
						.tab_index(0)
						.aria_label(format!("Reference {}", work.title))
						.px_2()
						.py_1()
						.rounded(gpui::px(6.))
						.cursor_pointer()
						.hover(|d| d.bg(gpui::rgba(HOVER_FILL)))
						.child(
							gpui::div()
								.text_size(gpui::px(12.))
								.overflow_hidden()
								.text_ellipsis()
								.child(work.title.clone()),
						)
						.child(
							gpui::div()
								.text_size(gpui::px(10.))
								.text_color(gpui::rgb(TEXT_MUTED))
								.child(work.id.clone()),
						)
						.on_click(cx.listener(move |s, _, _, cx| {
							s.select_task_reference(clicked.clone(), cx)
						}))
						.on_key_down(cx.listener(move |s, e: &KeyDownEvent, _, cx| {
							if ["enter", "space"].contains(&e.keystroke.key.as_str()) {
								s.select_task_reference(reference.clone(), cx);
								cx.stop_propagation();
							}
						}))
						.smooth(),
				);
				count += 1;
			}
		}

		if count == 0 {
			list = list.child(
				gpui::div()
					.text_size(gpui::px(11.))
					.child("No matching tasks with a conversation."),
			);
		}

		gpui::div()
			.flex()
			.flex_col()
			.gap_2()
			.child(
				gpui::div().h(gpui::px(36.)).flex_none().child(self.task_reference_search.clone()),
			)
			.child(
				gpui::div()
					.text_size(gpui::px(10.))
					.text_color(gpui::rgb(TEXT_MUTED))
					.child("Sending grants read access to the selected task history."),
			)
			.child(list)
			.into_any_element()
	}

	pub(super) fn task_reference_row(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
		if self.task_references.is_empty() {
			return None;
		}

		let mut row = gpui::div().flex().flex_wrap().gap_1().px_1();

		for reference in &self.task_references {
			let remove = reference.clone();
			let clicked = remove.clone();

			row = row.child(
				gpui::div()
					.id(SharedString::from(format!(
						"selected-task-{}-{}",
						reference.work_id.as_str(),
						reference.thread_id.as_str()
					)))
					.debug_selector({
						let id = format!(
							"selected-task-{}-{}",
							reference.work_id.as_str(),
							reference.thread_id.as_str()
						);

						move || id.clone()
					})
					.role(Role::Button)
					.tab_index(0)
					.aria_label(format!("Remove task reference {}", reference.title.as_str()))
					.max_w(gpui::px(220.))
					.px_2()
					.py_1()
					.rounded(gpui::px(6.))
					.bg(gpui::rgba(0xffffff0a))
					.text_size(gpui::px(11.))
					.cursor_pointer()
					.child(
						gpui::div()
							.overflow_hidden()
							.text_ellipsis()
							.child(format!("@{} ×", reference.title.as_str())),
					)
					.on_click(cx.listener(move |s, _, _, cx| {
						s.task_references.retain(|r| r != &clicked);
						cx.notify();
					}))
					.on_key_down(cx.listener(move |s, e: &KeyDownEvent, _, cx| {
						if ["enter", "space", "backspace"].contains(&e.keystroke.key.as_str()) {
							s.task_references.retain(|r| r != &remove);
							cx.notify();
							cx.stop_propagation();
						}
					}))
					.smooth(),
			);
		}

		Some(row.into_any_element())
	}
}

#[cfg(test)]
mod tests {
	use std::{slice, thread};

	use gpui::AppContext as _;
	#[cfg(test)] use gpui::Focusable as _;

	#[cfg(test)]
	use crate::shell::agent_surface::composer::task_references::{AgentActionDto, HistoryText};
	use crate::shell::agent_surface::composer::task_references::{
		AgentSurface, AgentTaskReferenceDto, EntityId, WireText,
	};

	fn reference(id: &str) -> AgentTaskReferenceDto {
		AgentTaskReferenceDto {
			work_id: EntityId::new(id).unwrap(),
			thread_id: WireText::new(format!("thread-{id}")).unwrap(),
			title: WireText::new(id).unwrap(),
		}
	}

	#[gpui::test]
	fn reference_drafts_deduplicate_and_clear_only_confirmed_sent_selection(
		cx: &mut gpui::TestAppContext,
	) {
		let surface = cx.new(AgentSurface::new);

		surface.update(cx, |s, cx| {
			let first = reference("first");
			let later = reference("later");

			s.select_task_reference(first.clone(), cx);
			s.select_task_reference(first.clone(), cx);

			assert_eq!(s.task_references.len(), 1);

			s.select_task_reference(later.clone(), cx);
			s.apply_command_result(Err("disconnected".into()), Some("draft"), cx);

			assert_eq!(s.task_references.len(), 2);

			s.clear_sent_task_references(slice::from_ref(&first), true, None);

			assert_eq!(s.task_references, vec![later.clone()]);

			s.draft_profiles.tasks.insert("other".into(), vec![first.clone(), later.clone()]);
			s.clear_sent_task_references(&[first], false, Some("other"));

			assert_eq!(s.draft_profiles.tasks["other"], vec![later.clone()]);
			assert_eq!(s.task_references, vec![later]);
		});
	}

	#[gpui::test]
	fn configured_send_carries_exact_reference_and_disables_empty_live_action(
		cx: &mut gpui::TestAppContext,
	) {
		let surface = cx.new(AgentSurface::new);

		surface.update(cx, |s, cx| {
			s.visual_workspace_fixture(cx);

			let selected = reference("target");

			s.select_task_reference(selected.clone(), cx);

			let action = s.configured_send(
				EntityId::new("agent").unwrap(),
				HistoryText::new("Read it").unwrap(),
				vec![],
			);

			match action {
				AgentActionDto::SendConfigured { task_references, .. }
				| AgentActionDto::Steer { task_references, .. } => assert_eq!(task_references, vec![selected]),
				_ => panic!("expected message action"),
			}

			assert!(!s.stop_button(cx));
		});
	}

	#[gpui::test]
	fn real_picker_search_select_remove_and_manager_drafts(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_400.), gpui::px(1_000.)));

		let search = surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.visual_task_references();
			s.task_references.clear();

			s.task_reference_search.clone()
		});

		visual.update(|window, cx| {
			window.focus(&search.focus_handle(cx), cx);
			window.draw(cx).clear(cx);
		});
		visual.simulate_keystrokes("v e r i f y");

		thread::sleep(std::time::Duration::from_millis(220));

		visual.update(|window, cx| {
			window.draw(cx).clear(cx);
		});

		assert_eq!(search.read_with(visual, |input, _| input.content().to_owned()), "verify");
		assert!(visual.debug_bounds("reference-task-flow").is_none());

		let bounds = visual.debug_bounds("reference-task-verify").expect("matching task rendered");

		visual.simulate_click(bounds.center(), gpui::Modifiers::default());

		surface.update(visual, |s, cx| {
			assert_eq!(s.task_references.len(), 1);
			assert_eq!(s.task_references[0].work_id.as_str(), "verify");
			assert!(s.composer_menu.is_none());

			let mut manager = s.snapshot.as_ref().unwrap().work_items[0].clone();

			manager.id = "other-manager".into();
			manager.kind = decodex_protocol::AgentWorkKindDto::Manager;
			manager.parent_goal_id = Some("agent".into());

			s.snapshot.as_mut().unwrap().work_items.push(manager);
			s.open_page("other-manager", cx);

			assert!(s.task_references.is_empty());

			s.open_page("agent", cx);

			assert_eq!(s.task_references.len(), 1);
		});

		visual.update(|window, cx| {
			window.draw(cx).clear(cx);
		});

		let chip = visual
			.debug_bounds("selected-task-verify-fixture-thread-verify")
			.expect("selected chip rendered");

		visual.simulate_click(chip.center(), gpui::Modifiers::default());
		surface.update(visual, |s, _| assert!(s.task_references.is_empty()));
	}
}
