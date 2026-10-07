//! A global handoff strip and one bounded, in-place preview.
use super::{
	AgentSurface, Context, InteractiveElement, IntoElement, ParentElement, Role, SharedString,
	StatefulInteractiveElement, Styled,
	handoffs::Handoff,
	ui_theme::{AMBER, CANVAS, TEXT_MUTED},
};
use gpui::{AnyElement, KeyDownEvent};

impl AgentSurface {
	pub(super) fn handoff_items(&self) -> Vec<Handoff> {
		self.snapshot.as_ref().map_or_else(Vec::new, |snapshot| self.handoffs.items(snapshot))
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
		if self.handoffs.relations {
			self.open_page(id, cx);
			self.workspace.dock_record = None;
			self.handoffs.relations = false;
		} else {
			self.toggle_dock_record(id, cx);
		}
		cx.notify();
	}

	pub(super) fn handoff_bar(&self, cx: &mut Context<Self>) -> AnyElement {
		let items = self.handoff_items();
		let running = self.snapshot.as_ref().map_or(0, |snapshot| {
			snapshot
				.work_items
				.iter()
				.filter(|w| {
					matches!(
						w.dispatch_state,
						super::AgentDispatchStateDto::Running
							| super::AgentDispatchStateDto::Dispatching
					)
				})
				.count()
		});
		let mut strip = gpui::div()
			.id("handoff-strip")
			.debug_selector(|| "handoff-strip".into())
			.flex()
			.items_center()
			.gap_2()
			.flex_1()
			.min_w_0()
			.overflow_x_scroll();
		if items.is_empty() {
			strip = strip.child(gpui::div().text_color(gpui::rgb(TEXT_MUTED)).child(
				if self.snapshot.as_ref().is_none_or(|s| s.connection_initializing) {
					"Connecting to your work…"
				} else {
					"Nothing needs your attention"
				},
			));
		}
		for item in items {
			strip = strip.child(self.handoff_chip(&item, cx));
		}
		gpui::div()
			.h_full()
			.w_full()
			.flex()
			.items_center()
			.gap_2()
			.px_2()
			.text_size(gpui::px(12.))
			.child(gpui::div().flex_none().text_color(gpui::rgb(TEXT_MUTED)).child("For you"))
			.child(strip)
			.child(
				gpui::div().flex_none().text_color(gpui::rgb(TEXT_MUTED)).child(if running > 0 {
					format!("{running} running")
				} else {
					String::new()
				}),
			)
			.into_any_element()
	}

	fn handoff_chip(&self, item: &Handoff, cx: &mut Context<Self>) -> AnyElement {
		let Some(work) =
			self.snapshot.as_ref().and_then(|s| s.work_items.iter().find(|w| w.id == item.work))
		else {
			return gpui::div().into_any_element();
		};
		let id = item.work.clone();
		let keyboard = id.clone();
		let title = self.work_label(work);
		let selector = format!("handoff-{id}");
		gpui::div()
			.id(SharedString::from(selector.clone()))
			.debug_selector(move || selector.clone())
			.role(Role::Button)
			.tab_index(0)
			.aria_label(format!("{title}: {}. Preview handoff.", item.label))
			.aria_expanded(self.workspace.dock_record.as_ref() == Some(&id))
			.flex_none()
			.w(gpui::px(184.))
			.px_2()
			.py_1()
			.rounded(gpui::px(8.))
			.cursor_pointer()
			.bg(gpui::rgba(if self.workspace.dock_record.as_ref() == Some(&id) {
				0xffffff18
			} else {
				0xffffff08
			}))
			.hover(|s| s.bg(gpui::rgba(0xffffff20)))
			.on_click(cx.listener(move |s, _, _, cx| {
				s.handoffs.relations = false;
				s.toggle_dock_record(&id, cx);
			}))
			.on_key_down(cx.listener(move |s, e: &KeyDownEvent, _, cx| {
				if e.keystroke.key == "enter" || e.keystroke.key == "space" {
					s.handoffs.relations = false;
					s.toggle_dock_record(&keyboard, cx);
					cx.stop_propagation();
				}
			}))
			.child(gpui::div().whitespace_nowrap().text_ellipsis().child(title))
			.child(
				gpui::div()
					.text_color(gpui::rgb(if item.result { TEXT_MUTED } else { AMBER }))
					.child(item.label),
			)
			.into_any_element()
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
		if !self.workspace_graph_full_layout().edges.is_empty() {
			header = header.child(self.workspace_action(
				"handoff-relations".into(),
				if self.handoffs.relations { "Back" } else { "Dependencies" }.into(),
				|s, cx| {
					s.handoffs.relations = !s.handoffs.relations;
					s.workspace.dock_compact = false;
					s.workspace.dock_completed = true;
					s.workspace.graph_pan = (0., 0.);
					s.workspace.graph_panel_height = s.workspace.graph_panel_height.max(400.);
					cx.notify();
				},
				cx,
			));
		}
		header = header.child(self.workspace_action(
			"handoff-close".into(),
			"Close".into(),
			|s, cx| {
				s.workspace.dock_record = None;
				s.handoffs.relations = false;
				cx.notify();
			},
			cx,
		));
		header
	}

	pub(super) fn handoff_popup(
		&self,
		width: f32,
		height: f32,
		cx: &mut Context<Self>,
	) -> Option<AnyElement> {
		let item = self
			.handoff_items()
			.into_iter()
			.find(|h| Some(&h.work) == self.workspace.dock_record.as_ref())?;
		let snapshot = self.snapshot.as_ref()?;
		let work = snapshot.work_items.iter().find(|w| w.id == item.work)?;
		let header = self.handoff_header(work, cx);
		let mut body = gpui::div()
			.id("handoff-preview")
			.occlude()
			.debug_selector(|| "handoff-preview".into())
			.w(gpui::px((width - 16.).clamp(240., 640.)))
			.max_h(gpui::px(
				self.workspace.graph_panel_height.clamp(160., (height - 160.).max(160.)),
			))
			.p_3()
			.rounded(gpui::px(12.))
			.bg(gpui::rgb(CANVAS))
			.border_1()
			.border_color(gpui::rgba(0xffffff20))
			.shadow_lg()
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
					s.handoffs.relations = false;
					cx.stop_propagation();
					cx.notify();
				}
			}))
			.child(header);
		if self.handoffs.relations {
			body = body
				.child(gpui::div().h(gpui::px(280.)).child(self.workspace_dependency_graph(cx)));
		} else {
			body = body.child(item.reason);
			if item.result {
				body = body.child(self.overview_evidence(work, cx));
			} else {
				if let Some(text) = self
					.handoff_request_excerpt(work)
					.or_else(|| self.handoff_decision_excerpt(work))
				{
					body = body.child(text);
				}
				let id = item.work.clone();
				body = body.child(
					gpui::div().flex().child(
						self.workspace_action(
							"handoff-open-request".into(),
							if item.label == "Check execution" {
								"Open conversation"
							} else {
								"Open task to respond"
							}
							.into(),
							move |s, cx| {
								s.open_page(&id, cx);
								s.sync_request(cx);
								s.workspace.dock_record = None;
								s.handoffs.relations = false;
								cx.notify();
							},
							cx,
						),
					),
				);
			}
			if item.result {
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
