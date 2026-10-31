use super::*;
use gpui::{Entity, Render, TestAppContext, Window};

struct View(Entity<AgentSurface>);
impl Render for View {
	fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		self.0.update(cx, |surface, cx| surface.render_workspace(window, cx))
	}
}

#[gpui::test]
fn side_chat_tabs_share_one_header_and_keep_each_draft(cx: &mut TestAppContext) {
	let (view, visual) = cx.add_window_view(|_, cx| {
		let surface = cx.new(AgentSurface::new);
		cx.observe(&surface, |_, _, cx| cx.notify()).detach();
		surface.update(cx, |s, cx| {
			s.visual_side_chat_tabs("agent", cx);
			s.composer.update(cx, |input, cx| input.set_content("Source draft", cx));
		});
		View(surface)
	});
	visual.simulate_resize(gpui::size(gpui::px(1_200.), gpui::px(760.)));
	visual.update(crate::ui_frame::draw_current_frame);
	std::thread::sleep(std::time::Duration::from_millis(240));
	visual.update(crate::ui_frame::draw_frame);
	let source = visual.debug_bounds("conversation-tab-agent").unwrap();
	let first = visual.debug_bounds("conversation-tab-side-one").unwrap();
	let second = visual.debug_bounds("conversation-tab-side-two").unwrap();
	let add = visual.debug_bounds("new-side-chat").unwrap();
	assert!(add.left() >= second.right() && add.left() - second.right() < gpui::px(24.));
	assert!(visual.debug_bounds("page-side-one").is_none());
	assert_eq!(source.center().y, first.center().y);
	assert_eq!(first.center().y, second.center().y);
	assert!(source.right() <= first.left() && first.right() <= second.left());
	let header = visual.debug_bounds("conversation-header").unwrap();
	assert!(header.contains(&source.center()) && header.contains(&second.center()));
	let surface = view.read_with(visual, |view, _| view.0.clone());
	for (id, selector, expected, draft) in [
		("side-one", "conversation-tab-side-one", "", "Architecture draft"),
		("side-two", "conversation-tab-side-two", "", "Tradeoff draft"),
		("agent", "conversation-tab-agent", "Source draft", "Source draft"),
		("side-one", "conversation-tab-side-one", "Architecture draft", "Architecture draft"),
		("side-two", "conversation-tab-side-two", "Tradeoff draft", "Tradeoff draft"),
	] {
		let bounds = visual.debug_bounds(selector).unwrap();
		visual.simulate_click(bounds.center(), Default::default());
		surface.update(visual, |s, cx| {
			assert_eq!(s.selected.as_deref(), Some(id));
			assert_eq!(s.composer.read(cx).content(), expected);
			s.composer.update(cx, |input, cx| input.set_content(draft, cx));
			assert!(s.submission.waiting.is_none());
		});
		visual.update(crate::ui_frame::draw_current_frame);
	}
}

#[gpui::test]
fn closing_a_side_chat_returns_to_its_source_and_keeps_the_native_record(cx: &mut TestAppContext) {
	let surface = cx.new(AgentSurface::new);
	surface.update(cx, |s, cx| {
		s.visual_side_chat_tabs("release", cx);
		s.composer.update(cx, |input, cx| input.set_content("Project draft", cx));
		s.open_page("side-one", cx);
		let native = s.conversation_record("side-one").unwrap().codex_thread_id.clone();
		s.composer.update(cx, |input, cx| input.set_content("Keep this side draft", cx));
		s.close_page("side-one", cx);
		assert_eq!(s.selected.as_deref(), Some("release"));
		assert_eq!(s.composer.read(cx).content(), "Project draft");
		assert_eq!(s.conversation_record("side-one").unwrap().codex_thread_id, native);
		assert!(!s.work_is_archived("side-one"));
		s.open_page("side-one", cx);
		assert_eq!(s.composer.read(cx).content(), "Keep this side draft");
		assert!(!s.workspace.closing_pages.contains("side-one"));
	});
}

#[gpui::test]
fn side_chat_tabs_require_current_confirmed_native_attachments(cx: &mut TestAppContext) {
	let surface = cx.new(AgentSurface::new);
	surface.update(cx, |s, cx| {
		s.visual_side_chat_tabs("agent", cx);
		let source_title = s.conversation_record("agent").unwrap().title.clone();
		for (id, suffix) in [("side-one", "attached branch"), ("side-two", "Side chat")] {
			s.snapshot
				.as_mut()
				.unwrap()
				.work_items
				.iter_mut()
				.find(|work| work.id == id)
				.unwrap()
				.title = format!("{source_title} ({suffix})");
		}
		let tabs = s.conversation_tabs();
		assert_eq!(tabs.len(), 3);
		assert!(
			tabs.iter().filter(|tab| tab.closable).all(|tab| tab.label.starts_with("Side chat "))
		);
		s.snapshot
			.as_mut()
			.unwrap()
			.work_items
			.iter_mut()
			.find(|work| work.id == "side-one")
			.unwrap()
			.title = "My research".into();
		assert_eq!(
			s.conversation_tabs().iter().find(|tab| tab.id == "side-one").unwrap().label,
			"My research"
		);
		let Some(AttachedBranchesResult::Available { branches, .. }) =
			&mut s.workspace.attached.result
		else {
			panic!("fixture attachments");
		};
		branches[0].phase = PromptForkPhase::Uncertain;
		assert_eq!(s.conversation_tabs().len(), 2);
		s.snapshot
			.as_mut()
			.unwrap()
			.work_items
			.iter_mut()
			.find(|work| work.id == "side-one")
			.unwrap()
			.codex_thread_id = Some("replacement".into());
		assert_eq!(s.conversation_tabs().len(), 1);
		let mut replacement = s.snapshot.clone().unwrap();
		replacement.runtime_source =
			Some(decodex_protocol::EntityId::new("replacement-runtime").unwrap());
		s.invalidate_attached_branches(&replacement);
		assert!(s.workspace.attached.result.is_none());
	});
}

#[gpui::test]
fn side_chat_lookup_uses_a_fresh_receipt_before_grouping_a_local_child(cx: &mut TestAppContext) {
	let surface = cx.new(AgentSurface::new);
	surface.update(cx, |s, cx| {
		s.visual_side_chat_tabs("agent", cx);
		let snapshot = s.snapshot.as_mut().unwrap();
		let mut unrelated =
			snapshot.work_items.iter().find(|work| work.id == "side-one").unwrap().clone();
		unrelated.id = "independent-agent".into();
		unrelated.codex_thread_id = Some("independent-thread".into());
		assert_eq!(s.workspace.attached.lookup_root(&unrelated, snapshot), "agent");
		s.workspace.attached.lookup_for =
			Some((unrelated.id.clone(), unrelated.codex_thread_id.clone()));
		assert_eq!(s.workspace.attached.lookup_root(&unrelated, snapshot), "independent-agent");
		let child = snapshot.work_items.iter_mut().find(|work| work.id == "side-one").unwrap();
		child.codex_thread_id = Some("replacement-thread".into());
		let child = child.clone();
		s.workspace.attached.lookup_for = Some((child.id.clone(), Some("old-thread".into())));
		assert_eq!(s.workspace.attached.lookup_root(&child, snapshot), "agent");
	});
}

#[gpui::test]
fn narrow_tab_bar_reveals_the_selected_side_chat_and_keeps_add_visible(cx: &mut TestAppContext) {
	let (view, visual) = cx.add_window_view(|_, cx| {
		let surface = cx.new(AgentSurface::new);
		cx.observe(&surface, |_, _, cx| cx.notify()).detach();
		surface.update(cx, |s, cx| s.visual_side_chat_tabs("agent", cx));
		View(surface)
	});
	visual.simulate_resize(gpui::size(gpui::px(650.), gpui::px(700.)));
	visual.update(crate::ui_frame::draw_frame);
	std::thread::sleep(std::time::Duration::from_millis(240));
	visual.update(crate::ui_frame::draw_frame);
	let surface = view.read_with(visual, |view, _| view.0.clone());
	surface.update(visual, |s, cx| s.open_page("side-two", cx));
	visual.update(crate::ui_frame::draw_frame);
	visual.update(crate::ui_frame::draw_frame);
	let selected = visual.debug_bounds("conversation-tab-side-two").unwrap();
	let add = visual.debug_bounds("new-side-chat").unwrap();
	let header = visual.debug_bounds("conversation-header").unwrap();
	assert!(header.right() <= gpui::px(650.), "header extends beyond the window: {header:?}");
	let strip = surface.read_with(visual, |s, _| s.workspace.attached.scroll.bounds());
	assert!(strip.contains(&selected.center()), "active tab must scroll into view");
	assert!(header.contains(&add.center()));
	assert!(strip.right() <= add.left(), "the add action stays outside tab scrolling");
}
