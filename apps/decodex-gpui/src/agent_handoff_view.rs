//! A global Dock that expands into one bounded work canvas.
use super::{
	AgentSurface, Context, InteractiveElement, IntoElement, ParentElement,
	StatefulInteractiveElement, Styled, handoffs::Handoff, ui_theme::TEXT_MUTED,
};
use gpui::{AnyElement, KeyDownEvent};

impl AgentSurface {
	pub(super) fn handoff_items(&self) -> Vec<Handoff> {
		self.snapshot.as_ref().map_or_else(Vec::new, |snapshot| self.handoffs.items(snapshot))
	}

	fn dock_items(&self) -> Vec<Handoff> {
		self.snapshot.as_ref().map_or_else(Vec::new, |snapshot| self.handoffs.dock_items(snapshot))
	}

	pub(super) fn acknowledge_open_handoff(&mut self, work: &str) {
		if let Some((id, key)) = self.handoffs.read_on_open.clone().filter(|(id, _)| id == work)
			&& let Some(snapshot) = &self.snapshot
		{
			self.handoffs.acknowledge(&id, &key, snapshot);
			self.handoffs.read_on_open = None;
		}
	}

	pub(super) fn activate_graph_node(&mut self, id: &str, cx: &mut Context<Self>) {
		self.handoffs.focus = Some(id.into());
		self.workspace.graph_panel_height = self.workspace.graph_panel_height.max(460.);
		self.load_dock_evidence(id, cx);
		cx.notify();
	}

	pub(super) fn dock_open(&self) -> bool {
		self.handoffs.focus.is_some()
	}

	pub(super) fn handoff_canvas(&self, cx: &mut Context<Self>) -> AnyElement {
		let compact = self.workspace.dock_compact && !self.workspace.graph_expanded;
		let mut header = gpui::div()
			.h(gpui::px(38.))
			.flex_none()
			.flex()
			.items_center()
			.px_2()
			.child(gpui::div().flex_1().text_size(gpui::px(12.)).child("Work graph"))
			.child(self.workspace_action(
				"dock-toggle".into(),
				if compact { "Show graph" } else { "Collapse graph" }.into(),
				|s, cx| {
					s.workspace.dock_compact = !s.workspace.dock_compact;
					s.workspace.graph_expanded = false;
					s.workspace.graph_panel_height = s.workspace.graph_panel_height.max(320.);
					cx.notify();
				},
				cx,
			))
			.child(
				self.workspace_action(
					"graph-expand".into(),
					if self.workspace.graph_expanded {
						"Restore conversation"
					} else {
						"Expand work overview"
					}
					.into(),
					|s, cx| {
						s.workspace.graph_expanded = !s.workspace.graph_expanded;
						s.workspace.dock_compact = false;
						cx.notify();
					},
					cx,
				),
			);
		if self.workspace.dock_record.is_some() {
			header = header.child(self.workspace_action(
				"graph-home".into(),
				"All work".into(),
				|s, cx| {
					s.workspace.dock_record = None;
					s.handoffs.focus = None;
					s.workspace.graph_pan = (0., 0.);
					s.workspace.graph_zoom = 1.;
					cx.notify();
				},
				cx,
			));
		}
		header = header.child(self.workspace_action(
			"graph-close".into(),
			"Close Dock".into(),
			|s, cx| {
				s.workspace.graph_visible = false;
				s.workspace.graph_expanded = false;
				cx.notify();
			},
			cx,
		));

		let mut panel = gpui::div()
			.id("work-dock")
			.debug_selector(|| "work-dock".into())
			.size_full()
			.flex()
			.flex_col()
			.overflow_hidden()
			.border_t_1()
			.border_color(gpui::rgba(0xffffff18))
			.capture_any_mouse_down(cx.listener(|s, _, _, _| {
				s.workspace.focused_panel = Some(super::workspace_size::Panel::Bottom)
			}))
			.child(header);
		if !compact {
			panel = panel
				.child(
					gpui::div()
						.px_3()
						.text_size(gpui::px(11.))
						.text_color(gpui::rgb(TEXT_MUTED))
						.child(self.graph_context()),
				)
				.child(gpui::div().flex_1().min_h_0().child(self.workspace_dependency_graph(cx)));
			if let Some(details) = self.handoff_details(cx) {
				panel = panel.child(
					gpui::div()
						.h(gpui::px(180.))
						.flex_none()
						.border_t_1()
						.border_color(gpui::rgba(0xffffff12))
						.child(details),
				);
			}
		}
		panel.into_any_element()
	}

	fn handoff_source(&self, work: &super::AgentWorkItemDto) -> String {
		let Some(snapshot) = &self.snapshot else { return "Your work".into() };
		snapshot
			.workspaces
			.iter()
			.find(|w| w.work_ids.contains(&work.id))
			.map(|w| w.name.clone())
			.or_else(|| {
				work.parent_goal_id
					.as_ref()
					.and_then(|id| snapshot.work_items.iter().find(|w| &w.id == id))
					.map(|w| self.work_label(w))
			})
			.unwrap_or_else(|| "Your work".into())
	}

	fn handoff_header(&self, work: &super::AgentWorkItemDto, cx: &mut Context<Self>) -> gpui::Div {
		let source = self.handoff_source(work);
		let mut header = gpui::div().flex().items_center().gap_2().child(
			gpui::div().flex_1().min_w_0().child(self.work_label(work)).child(
				gpui::div()
					.text_size(gpui::px(11.))
					.text_color(gpui::rgb(TEXT_MUTED))
					.child(source),
			),
		);

		header = header.child(self.workspace_action(
			"handoff-close".into(),
			"Close details".into(),
			|s, cx| {
				s.handoffs.focus = None;
				cx.notify();
			},
			cx,
		));
		if self.snapshot.as_ref().is_some_and(|snapshot| {
			snapshot.work_items.iter().any(|w| w.parent_goal_id.as_deref() == Some(&work.id))
		}) {
			let id = work.id.clone();
			header = header.child(self.workspace_action(
				"dock-subtasks".into(),
				"Show subtasks".into(),
				move |s, cx| {
					s.toggle_dock_record(&id, cx);
					s.handoffs.focus = None;
				},
				cx,
			));
		}

		header
	}

	fn handoff_details(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
		if !self.dock_open() {
			return None;
		}
		let snapshot = self.snapshot.as_ref()?;
		let id = self.handoffs.focus.as_ref().or(self.workspace.dock_record.as_ref())?;
		let work = snapshot.work_items.iter().find(|w| &w.id == id).or_else(|| {
			snapshot.work_items.iter().find(|w| Some(&w.id) == self.workspace.dock_record.as_ref())
		})?;
		let state = super::dock::progress_state(snapshot, work);
		let item = self.dock_items().into_iter().find(|h| &h.work == id);
		let attention = item.as_ref().is_some_and(|h| h.attention);
		let result = item.as_ref().is_some_and(|h| h.result);

		let header = self.handoff_header(work, cx);
		let mut body = gpui::div()
			.id("handoff-preview")
			.occlude()
			.debug_selector(|| "handoff-preview".into())
			.w_full()
			.h_full()
			.min_h_0()
			.p_3()
			.overflow_y_scroll()
			.text_size(gpui::px(12.))
			.flex()
			.flex_col()
			.gap_2()
			.capture_any_mouse_down(cx.listener(|s, _, _, _| {
				s.workspace.focused_panel = Some(super::workspace_size::Panel::Bottom);
			}))
			.on_key_down(cx.listener(|s, e: &KeyDownEvent, _, cx| {
				if e.keystroke.key == "escape" {
					s.workspace.dock_record = None;
					s.handoffs.focus = None;
					cx.stop_propagation();
					cx.notify();
				}
			}))
			.child(header);
		{
			let question =
				self.handoff_request_excerpt(work).or_else(|| self.handoff_decision_excerpt(work));
			if state.group != 3 && question.is_none() {
				body = body.child(state.reason);
			}
			if result || !attention {
				body = body.child(self.overview_evidence(work, cx));
			} else {
				if let Some(text) = question {
					body = body.child(text);
				}
				let id = work.id.clone();
				body = body.child(
					gpui::div().flex().child(
						self.workspace_action(
							"handoff-open-request".into(),
							if state.label == "Status unavailable" {
								"Open conversation"
							} else {
								"Open task to respond"
							}
							.into(),
							move |s, cx| {
								s.open_page(&id, cx);
								s.sync_request(cx);
								s.workspace.dock_record = None;
								s.handoffs.focus = None;
								cx.notify();
							},
							cx,
						),
					),
				);
			}
			if result && attention {
				let item = item.expect("unread result");
				body = body.child(gpui::div().flex().child(self.workspace_action(
					"handoff-viewed".into(),
					"Mark viewed".into(),
					move |s, cx| {
						if let Some(snapshot) = &s.snapshot {
							s.handoffs.acknowledge(&item.work, &item.key, snapshot);
						}
						s.workspace.dock_record = None;
						cx.notify();
					},
					cx,
				)));
			}
		}
		Some(body.into_any_element())
	}
}
