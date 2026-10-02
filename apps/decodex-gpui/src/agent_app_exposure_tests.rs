//! Rendered edits preserve inherited omissions until an explicit save.
#[cfg(test)] use gpui::AppContext as _;
use gpui::TestAppContext;

#[cfg(test)]
use crate::shell::agent_surface::app_exposure::{
	self, AgentDispatchStateDto, AgentSnapshotResult, AgentWorkItemDto, AgentWorkStatusDto, Entity,
	Render, Window,
};
use crate::shell::agent_surface::app_exposure::{
	AgentAppExposureResult, AgentSnapshotDto, AgentSurface, AgentToolExposureSurface, Context,
	EntityId, IntoElement, WireText,
};
use decodex_protocol::{AgentPendingEventDto, AgentWorkKindDto};

struct ExposureView {
	surface: Entity<AgentSurface>,
}
impl Render for ExposureView {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		self.surface.update(cx, |s, cx| s.integrations_panel("root", cx))
	}
}

#[gpui::test]
fn exposure_edits_preserve_inheritance_and_reset_on_source_change(cx: &mut TestAppContext) {
	let (view, visual) = cx.add_window_view(|_, cx| {
		let surface = cx.new(AgentSurface::new);

		cx.observe(&surface, |_, _, cx| cx.notify()).detach();

		ExposureView { surface }
	});
	let surface = view.read_with(visual, |v, _| v.surface.clone());

	surface.update(visual, |s, _| {
		s.apply_result(Ok(AgentSnapshotResult::Available(AgentSnapshotDto {
			runtime_source: Some(EntityId::new("native-source").unwrap()),
			workspaces: vec![],
			dependencies: vec![],
			work_items: vec![AgentWorkItemDto {
				id: "root".into(),
				parent_goal_id: None,
				kind: AgentWorkKindDto::Goal,
				title: "Agent".into(),
				codex_thread_id: Some("thread".into()),
				active_turn_id: None,
				dispatch_state: AgentDispatchStateDto::Idle,
				status: AgentWorkStatusDto::Open,
				next_check_at_micros: None,
				created_at_micros: 1,
				updated_at_micros: 1,
			}],
			pending_events: vec![AgentPendingEventDto {
				id: 7,
				source_event_id: "approval".into(),
				work_item_id: "root".into(),
				event_kind: "server_request_pending".into(),
				created_at_micros: 1,
				delivery_claimed: false,
			}],
		})));

		s.integrations = Some(("root".into(), None));
		s.app_exposure.owner = Some(("root".into(), "calendar".into()));
		s.app_exposure.state = Some(AgentAppExposureResult::Available {
			work_id: EntityId::new("root").unwrap(),
			connector_id: WireText::new("calendar").unwrap(),
			review_token: WireText::new("a".repeat(64)).unwrap(),
			effective: Some(vec!["direct".into(), "deferred".into()]),
			preference: None,
			can_update: true,
			last_outcome: None,
		});
	});

	visual.update(|w, cx| {
		w.resize(gpui::size(app_exposure::px(1_180.), app_exposure::px(2_600.)));
		w.draw(cx).clear();
	});

	let button = visual.debug_bounds("app-exposure-surface-0").expect("direct toggle");

	visual.simulate_click(button.center(), Default::default());
	surface.read_with(visual, |s, _| {
		assert_eq!(s.app_exposure.draft, Some(vec![AgentToolExposureSurface::Deferred]));
		assert!(s.app_exposure.task.is_none(), "editing does not send a write");
	});
	visual.update(|w, cx| {
		w.draw(cx).clear();
	});

	assert!(visual.debug_bounds("app-exposure-save").is_some());

	let clear = visual.debug_bounds("app-exposure-clear").unwrap();

	visual.simulate_click(clear.center(), Default::default());
	surface.read_with(visual, |s, _| assert_eq!(s.app_exposure.draft, Some(vec![])));
	visual.update(|w, cx| {
		w.draw(cx).clear();
	});

	let inherit = visual.debug_bounds("app-exposure-inherit").unwrap();

	visual.simulate_click(inherit.center(), Default::default());
	surface.read_with(visual, |s, _| assert_eq!(s.app_exposure.draft, None));
	visual.update(|w, cx| {
		w.draw(cx).clear();
	});

	assert!(visual.debug_bounds("app-exposure-save").is_none());

	surface.update(visual, |s, _| {
		if let Some(AgentAppExposureResult::Available { effective, .. }) = &mut s.app_exposure.state
		{
			*effective = Some(vec!["future-surface".into()]);
		}
	});

	visual.update(|w, cx| {
		w.draw(cx).clear();
	});

	assert!(visual.debug_bounds("app-exposure-surface-0").is_none());
	assert!(visual.debug_bounds("app-exposure-clear").is_none());

	surface.update(visual, |s, _| {
		let epoch = s.app_exposure.epoch;
		let mut snapshot = s.snapshot.clone().unwrap();

		snapshot.runtime_source = Some(EntityId::new("replacement-source").unwrap());

		s.apply_result(Ok(AgentSnapshotResult::Available(snapshot)));

		assert!(s.app_exposure.epoch != epoch);
		assert!(s.app_exposure.state.is_none());
	});
}
