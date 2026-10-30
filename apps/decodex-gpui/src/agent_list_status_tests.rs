//! Exercise the actual list controls without a service or native process.
use gpui::{AppContext as _, Context, Entity, IntoElement, Render, Window};

use super::{AgentSurface, View};
use decodex_protocol::{
	NativeAgentDto, ProjectDto,
	work::{Work, WorkRequirements, WorkState},
};

struct ListView {
	surface: Entity<AgentSurface>,
}
impl Render for ListView {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		self.surface.update(cx, |s, cx| s.render_work_browser(cx))
	}
}

fn fixture(cx: &mut Context<ListView>) -> ListView {
	let surface = cx.new(|cx| {
		let mut s = AgentSurface::new(cx);
		s.visual_workspace_fixture(cx);
		s.open_page("agent", cx);
		s.composer.update(cx, |input, cx| input.set_content("Unsent source draft", cx));
		s.workspace.browsing = true;
		s
	});
	cx.observe(&surface, |_, _, cx| cx.notify()).detach();
	ListView { surface }
}

fn project(id: &str, agents: Vec<String>) -> ProjectDto {
	ProjectDto {
		id: id.into(),
		name: id.into(),
		directory: format!("/tmp/{id}"),
		agent_ids: agents,
		native_project_id: None,
		lead_agent_id: None,
	}
}

#[gpui::test]
fn conversation_status_filter_preserves_search_project_and_source_draft(
	cx: &mut gpui::TestAppContext,
) {
	let (view, visual) = cx.add_window_view(|_, cx| fixture(cx));
	let surface = view.read_with(visual, |v, _| v.surface.clone());
	visual.update(|w, cx| {
		w.resize(gpui::size(gpui::px(1_200.), gpui::px(900.)));
		w.draw(cx).clear(cx);
	});
	let button = visual.debug_bounds("DecodexConversationListStatus-Running").unwrap();
	visual.simulate_click(button.center(), Default::default());
	visual.update(crate::ui_frame::draw_current_frame);
	assert!(visual.debug_bounds("browse-verify").is_some());
	assert!(visual.debug_bounds("browse-improve").is_some());
	assert!(visual.debug_bounds("browse-release").is_none());
	surface.update(visual, |s, cx| {
		s.snapshot.as_mut().unwrap().projects = vec![project("p", vec!["verify".into()])];
		s.workspace.workspace_filter = Some("p".into());
		s.work_search.update(cx, |input, cx| input.set_content("compatibility", cx));
		cx.notify();
	});
	visual.update(crate::ui_frame::draw_current_frame);
	assert!(visual.debug_bounds("browse-verify").is_some());
	assert!(visual.debug_bounds("browse-improve").is_none());
	surface.update(visual, |s, cx| {
		s.work_search.update(cx, |input, cx| input.set_content("no match", cx));
		cx.notify();
	});
	visual.update(crate::ui_frame::draw_current_frame);
	assert!(visual.debug_bounds("browse-verify").is_none());
	let all = visual.debug_bounds("DecodexConversationListStatus-All statuses").unwrap();
	visual.simulate_click(all.center(), Default::default());
	visual.update(crate::ui_frame::draw_current_frame);
	surface.update(visual, |s, cx| {
		assert_eq!(s.workspace.workspace_filter.as_deref(), Some("p"));
		assert_eq!(s.work_search.read(cx).content(), "no match");
		assert!(s.workspace.list_status.selected(View::Conversations).is_none());
		s.work_search.update(cx, |input, cx| input.set_content("compatibility", cx));
		cx.notify();
	});
	visual.update(crate::ui_frame::draw_current_frame);
	let row = visual.debug_bounds("browse-verify").unwrap();
	visual.simulate_click(row.center(), Default::default());
	surface.update(visual, |s, cx| {
		assert_eq!(s.selected.as_deref(), Some("verify"));
		s.open_page("agent", cx);
		assert_eq!(s.composer.read(cx).content(), "Unsent source draft");
		assert!(s.workspace.list_status.selected(View::Conversations).is_none());
	});
}

#[gpui::test]
fn native_status_filter_keeps_unknown_labels_and_changed_status_explicit(
	cx: &mut gpui::TestAppContext,
) {
	let (view, visual) = cx.add_window_view(|_, cx| fixture(cx));
	let surface = view.read_with(visual, |v, _| v.surface.clone());
	surface.update(visual, |s, cx| {
		s.native_agents.lists.insert(
			"agent".into(),
			vec![NativeAgentDto {
				thread_id: "child-thread".into(),
				parent_thread_id: "parent".into(),
				title: "Native review".into(),
				status: "paused_by_user".into(),
				task: String::new(),
				model: None,
				reasoning_effort: None,
			}],
		);
		cx.notify();
	});
	visual.update(|w, cx| {
		w.resize(gpui::size(gpui::px(1_200.), gpui::px(900.)));
		w.draw(cx).clear(cx);
	});
	let filter = visual.debug_bounds("DecodexConversationListStatus-paused_by_user").unwrap();
	visual.simulate_click(filter.center(), Default::default());
	visual.update(crate::ui_frame::draw_current_frame);
	assert!(visual.debug_bounds("browse-native:agent:child-thread").is_some());
	assert!(visual.debug_bounds("browse-verify").is_none());
	surface.update(visual, |s, cx| {
		s.native_agents.lists.get_mut("agent").unwrap()[0].status = "completed".into();
		cx.notify();
	});
	visual.update(crate::ui_frame::draw_current_frame);
	assert!(visual.debug_bounds("browse-native:agent:child-thread").is_none());
	assert!(visual.debug_bounds("DecodexConversationListStatus-paused_by_user").is_some());
	let all = visual.debug_bounds("DecodexConversationListStatus-All statuses").unwrap();
	visual.simulate_click(all.center(), Default::default());
	visual.update(crate::ui_frame::draw_current_frame);
	assert!(visual.debug_bounds("browse-native:agent:child-thread").is_some());
	surface.read_with(visual, |s, cx| {
		assert_eq!(s.selected.as_deref(), Some("agent"));
		assert_eq!(s.composer.read(cx).content(), "Unsent source draft");
		assert!(s.native_agents.selected.is_none());
	});
}

fn work(id: &str, project: &str, state: WorkState) -> Work {
	Work {
		id: id.into(),
		project_id: Some(project.into()),
		owner_agent_id: "agent".into(),
		title: format!("Check {id}"),
		state,
		reason: None,
		resume_condition: None,
		user_hold: false,
		assessment: None,
		requirements: WorkRequirements {
			id: "r1".into(),
			goal: "Check the result".into(),
			criteria: "Record evidence".into(),
			source: "User".into(),
		},
	}
}

#[gpui::test]
fn work_status_filter_is_independent_and_survives_view_switches(cx: &mut gpui::TestAppContext) {
	let (view, visual) = cx.add_window_view(|_, cx| fixture(cx));
	let surface = view.read_with(visual, |v, _| v.surface.clone());
	surface.update(visual, |s, cx| {
		let snapshot = s.snapshot.as_mut().unwrap();
		snapshot.projects = vec![project("p", vec!["agent".into()]), project("q", vec![])];
		snapshot.portfolio.works = vec![
			work("done", "p", WorkState::Completed),
			work("open", "p", WorkState::Open),
			work("other", "q", WorkState::Completed),
		];
		s.workspace.workspace_filter = Some("p".into());
		s.work_search.update(cx, |input, cx| input.set_content("check", cx));
		s.workspace.list_status.select(View::Conversations, Some("Running".into()));
		s.workspace.portfolio_mode = true;
		cx.notify();
	});
	visual.update(|w, cx| {
		w.resize(gpui::size(gpui::px(1_200.), gpui::px(900.)));
		w.draw(cx).clear(cx);
	});
	let completed = visual.debug_bounds("DecodexWorkListStatus-Completed").unwrap();
	visual.simulate_click(completed.center(), Default::default());
	visual.update(crate::ui_frame::draw_current_frame);
	assert!(visual.debug_bounds("work-done").is_some());
	assert!(visual.debug_bounds("work-open").is_none());
	assert!(visual.debug_bounds("work-other").is_none());
	let done = visual.debug_bounds("work-done").unwrap();
	visual.simulate_click(done.center(), Default::default());
	surface.update(visual, |s, cx| {
		assert_eq!(s.workspace.work_detail.as_deref(), Some("done"));
		s.workspace.portfolio_mode = false;
		s.open_page("agent", cx);
		s.workspace.browsing = true;
		s.workspace.portfolio_mode = true;
		assert_eq!(s.workspace.list_status.selected(View::Work), Some("Completed"));
		assert_eq!(s.workspace.list_status.selected(View::Conversations), Some("Running"));
		assert_eq!(s.workspace.workspace_filter.as_deref(), Some("p"));
		assert_eq!(s.work_search.read(cx).content(), "check");
		assert_eq!(s.composer.read(cx).content(), "Unsent source draft");
		cx.notify();
	});
	visual.update(crate::ui_frame::draw_current_frame);
	let all = visual.debug_bounds("DecodexWorkListStatus-All statuses").unwrap();
	visual.simulate_click(all.center(), Default::default());
	visual.update(crate::ui_frame::draw_current_frame);
	assert!(visual.debug_bounds("work-open").is_some());
	assert!(visual.debug_bounds("work-other").is_none());
}
