use crate::shell::agent_surface::{
	AgentDispatchStateDto, AgentSnapshotDto, AgentSnapshotResult, AgentSurface, AgentWorkItemDto,
	AgentWorkStatusDto, Context, EntityId, IntoElement,
};
use decodex_protocol::{AgentReadStateResult, AgentUnreadPosition, AgentWorkKindDto, WireText};
use gpui::{AppContext as _, Entity, Render, TestAppContext, Window};
struct ReceiptView {
	surface: Entity<AgentSurface>,
}
impl Render for ReceiptView {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		self.surface.update(cx, |s, cx| s.read_state_panel(cx))
	}
}
fn work() -> AgentWorkItemDto {
	AgentWorkItemDto {
		id: "root".into(),
		parent_goal_id: None,
		kind: AgentWorkKindDto::Goal,
		title: "Root".into(),
		codex_thread_id: Some("thread".into()),
		active_turn_id: Some("turn".into()),
		dispatch_state: AgentDispatchStateDto::Running,
		status: AgentWorkStatusDto::Open,
		next_check_at_micros: None,
		created_at_micros: 1,
		updated_at_micros: 1,
	}
}

fn snapshot() -> AgentSnapshotDto {
	AgentSnapshotDto {
		context_references: vec![],
		connection_initializing: false,
		runtime_source: Some(EntityId::new("source").expect("source")),
		workspaces: vec![],
		work_items: vec![work()],
		dependencies: vec![],
		pending_events: vec![],
	}
}

#[gpui::test]
fn read_state_panel_hides_invalid_actions_and_discards_replaced_source(cx: &mut TestAppContext) {
	let (view, visual) = cx.add_window_view(|_, cx| {
		let surface = cx.new(AgentSurface::new);
		cx.observe(&surface, |_, _, cx| cx.notify()).detach();
		surface.update(cx, |s, _| {
			s.apply_result(Ok(AgentSnapshotResult::Available(snapshot())));
			s.selected = Some("root".into());
		});
		ReceiptView { surface }
	});
	let surface = view.read_with(visual, |v, _| v.surface.clone());
	for available in [false, true] {
		surface.update(visual, |s, cx| {
			s.read_state.target = Some(("root".into(), "thread".into()));
			s.read_state.result = Some(if available {
				AgentReadStateResult::Available {
					work_id: EntityId::new("root").unwrap(),
					thread_id: EntityId::new("thread").unwrap(),
					first_unread: Some(AgentUnreadPosition::ThreadStart),
					revision: WireText::new("r1").unwrap(),
					review_token: WireText::new("review").unwrap(),
				}
			} else {
				AgentReadStateResult::Unavailable
			});
			cx.notify();
		});
		visual.update(|w, cx| {
			w.resize(gpui::size(gpui::px(500.), gpui::px(200.)));
			w.draw(cx).clear();
		});
		assert_eq!(visual.debug_bounds("native-read-state-mark").is_some(), available);
	}
	surface.update(visual, |s, cx| {
		s.native_agents.selected = Some(("root".into(), "child".into()));
		assert!(s.read_state_target().is_none());
		s.refresh_read_state(cx);
		assert!(s.read_state.result.is_none());
		s.native_agents.selected = None;
		s.read_state.target = Some(("root".into(), "thread".into()));
		s.read_state.result = Some(AgentReadStateResult::Unavailable);
		let mut next = snapshot();
		next.runtime_source = Some(EntityId::new("replacement").unwrap());
		s.apply_result(Ok(AgentSnapshotResult::Available(next)));
		s.apply_result(Ok(AgentSnapshotResult::Available(snapshot())));
		assert!(s.read_state.result.is_none());
	});
}
