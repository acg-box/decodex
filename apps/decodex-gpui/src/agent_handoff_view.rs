//! A global Dock that expands into one bounded work canvas.
use super::{
	AgentSurface, Context, InteractiveElement, IntoElement, ParentElement, Role, SharedString,
	StatefulInteractiveElement, Styled,
	handoffs::Handoff,
	ui_theme::{AMBER, BLUE, GREEN, TEXT_MUTED},
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
		self.load_dock_evidence(id, cx);
		cx.notify();
	}

	pub(super) fn dock_open(&self) -> bool {
		self.dock_items().iter().any(|h| Some(&h.work) == self.workspace.dock_record.as_ref())
	}

	pub(super) fn handoff_canvas(
		&self,
		width: f32,
		height: f32,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let mut panel = gpui::div()
			.id("work-dock")
			.debug_selector(|| "work-dock".into())
			.size_full()
			.flex()
			.flex_col()
			.overflow_hidden()
			.border_t_1()
			.border_color(gpui::rgba(0xffffff18))
			.child(gpui::div().h(gpui::px(72.)).flex_none().child(self.handoff_bar(cx)));
		if let Some(details) = self.handoff_details(cx) {
			let graph = !self.workspace_graph_full_layout().edges.is_empty();
			let mut content = gpui::div()
				.id("dock-content")
				.overflow_y_scroll()
				.flex_1()
				.min_h_0()
				.flex()
				.border_t_1()
				.border_color(gpui::rgba(0xffffff12));
			if graph {
				let graph_view = gpui::div()
					.min_h_0()
					.flex()
					.flex_col()
					.child(
						gpui::div()
							.px_3()
							.py_1()
							.text_size(gpui::px(11.))
							.text_color(gpui::rgb(TEXT_MUTED))
							.child("Dependencies · arrows point to the work they unblock"),
					)
					.child(self.workspace_dependency_graph(cx));
				if width >= 860. {
					content = content.child(graph_view.flex_1().min_w_0()).child(
						gpui::div()
							.w(gpui::px(300.))
							.h_full()
							.border_l_1()
							.border_color(gpui::rgba(0xffffff12))
							.child(details),
					);
				} else {
					content = content
						.flex_col()
						.child(
							graph_view.h(gpui::px(((height - 72.) * 0.55).max(100.))).flex_none(),
						)
						.child(
							gpui::div()
								.flex_1()
								.min_h(gpui::px(160.))
								.border_t_1()
								.border_color(gpui::rgba(0xffffff12))
								.child(details),
						);
				}
			} else {
				content = content.child(details);
			}
			panel = panel.child(content);
		}
		panel.into_any_element()
	}

	pub(super) fn handoff_bar(&self, cx: &mut Context<Self>) -> AnyElement {
		let items = self.dock_items();
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
			strip = if self.snapshot.as_ref().is_none_or(|s| s.connection_initializing) {
				strip.child(gpui::div().text_color(gpui::rgb(TEXT_MUTED)).child("Connecting…"))
			} else {
				strip.child(self.workspace_action(
					"dock-start-work".into(),
					"Start work  +".into(),
					|s, cx| s.new_work_conversation(cx),
					cx,
				))
			};
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
			.child(gpui::div().flex_none().text_color(gpui::rgb(TEXT_MUTED)).child("Work canvas"))
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
		let id = work.id.clone();
		let keyboard = id.clone();
		let title = self.work_label(work);
		let selector = format!("handoff-{id}");
		let color = if item.attention {
			AMBER
		} else if item.result {
			GREEN
		} else {
			BLUE
		};
		let symbol = if item.attention {
			"●"
		} else if item.result {
			"✓"
		} else {
			"◌"
		};
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
			.border_1()
			.border_color(gpui::rgba(0xffffff18))
			.bg(gpui::rgba(if self.workspace.dock_record.as_ref() == Some(&id) {
				0xffffff18
			} else {
				0xffffff08
			}))
			.hover(|s| s.bg(gpui::rgba(0xffffff20)))
			.on_click(cx.listener(move |s, _, _, cx| {
				s.handoffs.focus = None;
				s.toggle_dock_record(&id, cx);
			}))
			.on_key_down(cx.listener(move |s, e: &KeyDownEvent, _, cx| {
				if e.keystroke.key == "enter" || e.keystroke.key == "space" {
					s.handoffs.focus = None;
					s.toggle_dock_record(&keyboard, cx);
					cx.stop_propagation();
				}
			}))
			.child(
				gpui::div()
					.flex()
					.items_center()
					.gap_2()
					.child(gpui::div().text_color(gpui::rgb(color)).child(symbol))
					.child(
						gpui::div()
							.flex_1()
							.min_w_0()
							.whitespace_nowrap()
							.text_ellipsis()
							.child(title),
					)
					.child(if self.workspace.dock_record.as_deref() == Some(item.work.as_str()) {
						"▴"
					} else {
						"▾"
					}),
			)
			.child(gpui::div().text_color(gpui::rgb(color)).child(item.label))
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

		header = header.child(self.workspace_action(
			"handoff-close".into(),
			"Collapse".into(),
			|s, cx| {
				s.workspace.dock_record = None;
				s.handoffs.focus = None;
				cx.notify();
			},
			cx,
		));
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
