//! Question entry points retain their source and use ordinary Side chat creation.
use gpui::{Context, Entity, IntoElement, MouseButton, Render, TestAppContext, Window};

use super::*;
use crate::shell::agent_surface::{drafts, markdown};
use decodex_protocol::PromptForkBoundary;

struct Preview {
	surface: Entity<AgentSurface>,
	_service: tempfile::TempDir,
}
impl Render for Preview {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		self.surface.update(cx, |s, cx| {
			gpui::div()
				.size_full()
				.relative()
				.child(
					gpui::div()
						.group("history-response-preview")
						.child(markdown::render_with_selection_action(
							"Uses **cursor pagination**.",
							"quick-ask-text",
							s.side_chat_selection_action(cx),
						))
						.children(s.side_chat_response_action(
							"Uses **cursor pagination**.",
							"preview",
							cx,
						)),
				)
				.child(s.composer.clone())
		})
	}
}

fn preview(cx: &mut TestAppContext) -> (Entity<Preview>, &mut gpui::VisualTestContext) {
	cx.add_window_view(|_, cx| {
		let (service, profile, _) = drafts::tests::profiles();
		let owner = cx.new(AgentSurface::new);
		cx.observe(&owner, |_, _, cx| cx.notify()).detach();
		owner.update(cx, |s, cx| {
			s.bind_drafts(Some(&profile), cx);
			s.profile = Some(profile);
			s.visual_workspace_fixture(cx);
			let work = s.selected.clone().unwrap();
			let item = s
				.snapshot
				.as_mut()
				.unwrap()
				.work_items
				.iter_mut()
				.find(|item| item.id == work)
				.unwrap();
			item.parent_goal_id = None;
			item.codex_thread_id = Some("parent".into());
			item.active_turn_id = None;
			s.composer_manager = Some(work);
			s.composer.update(cx, |input, cx| input.set_content("Keep my draft.", cx));
			s.remember_draft_document(cx);
		});
		Preview { surface: owner, _service: service }
	})
}

fn draw(visual: &mut gpui::VisualTestContext) {
	visual.run_until_parked();
	visual.update(|window, cx| {
		window.resize(gpui::size(gpui::px(700.), gpui::px(600.)));
		window.draw(cx).clear(cx);
	});
}

#[gpui::test]
fn response_question_requests_a_side_chat_without_sending_or_replacing_the_main_draft(
	cx: &mut TestAppContext,
) {
	let (view, visual) = preview(cx);
	draw(visual);
	let button = visual.debug_bounds("ask-response-preview").expect("response question action");
	visual.simulate_mouse_move(button.center(), MouseButton::Left, Default::default());
	visual.simulate_click(button.center(), Default::default());
	let owner = view.read_with(visual, |view, _| view.surface.clone());
	owner.update(visual, |s, cx| {
		assert_eq!(
			s.prompt_edit.side_chat_reference.as_deref(),
			Some("Uses **cursor pagination**.")
		);
		assert_eq!(s.prompt_edit.branch_choice, Some(PromptForkBoundary::AttachedAfterTurn));
		assert!(s.prompt_edit.fork_requested);
		assert!(!s.workspace.details_visible);
		assert!(!s.sending);
		assert_eq!(s.composer.read(cx).content(), "Keep my draft.");
	});
}

#[gpui::test]
fn selected_text_starts_the_same_side_chat_flow_with_the_displayed_selection(
	cx: &mut TestAppContext,
) {
	let (view, visual) = preview(cx);
	draw(visual);
	let text = visual.debug_bounds("quick-ask-text-0").expect("selectable response");
	let start = gpui::point(text.left() + gpui::px(1.), text.top() + gpui::px(8.));
	let end = gpui::point(text.right() - gpui::px(1.), text.top() + gpui::px(8.));
	visual.simulate_mouse_down(start, MouseButton::Left, Default::default());
	visual.simulate_mouse_move(end, MouseButton::Left, Default::default());
	visual.simulate_mouse_up(end, MouseButton::Left, Default::default());
	draw(visual);
	let ask = visual.debug_bounds("ask-selected-text").expect("selection question action");
	visual.simulate_click(ask.center(), Default::default());
	let owner = view.read_with(visual, |view, _| view.surface.clone());
	owner.update(visual, |s, cx| {
		let reference = s.prompt_edit.side_chat_reference.as_deref().unwrap();
		assert!(reference.contains("cursor pagination"));
		assert!(!reference.contains("**"));
		assert!(s.prompt_edit.fork_requested);
		assert_eq!(s.composer.read(cx).content(), "Keep my draft.");
	});
}

#[gpui::test]
fn a_retained_question_action_does_not_open_a_side_chat_for_a_replaced_source(
	cx: &mut TestAppContext,
) {
	let (view, visual) = preview(cx);
	let owner = view.read_with(visual, |view, _| view.surface.clone());
	let action = owner.update(visual, |s, cx| s.side_chat_selection_action(cx).unwrap());
	owner.update(visual, |s, _| {
		let work = s.selected.clone().unwrap();
		s.snapshot
			.as_mut()
			.unwrap()
			.work_items
			.iter_mut()
			.find(|item| item.id == work)
			.unwrap()
			.codex_thread_id = Some("replacement".into());
	});
	visual.update(|window, cx| action("Old passage".into(), window, cx));
	assert!(owner.read_with(visual, |s, _| s.prompt_edit.side_chat_reference.is_none()));
}

#[test]
fn a_large_reference_is_marked_and_leaves_room_for_the_question() {
	assert_eq!(reference_draft("First line\nSecond line"), "> First line\n> Second line\n\n");
	let text = reference_draft(&"中文\n".repeat(5000));
	assert!(text.ends_with("… (excerpt)\n\n"));
	assert!(text.len() < crate::composer_input::MAX_COMPOSER_BYTES);
}
