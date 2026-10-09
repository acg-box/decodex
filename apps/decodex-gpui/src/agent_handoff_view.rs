//! A global Dock that expands into one bounded work canvas.
use super::{
	AgentSurface, Context, InteractiveElement, IntoElement, ParentElement,
	StatefulInteractiveElement, Styled, handoffs::Handoff, ui_theme::TEXT_MUTED,
};
use gpui::{AnyElement, KeyDownEvent, prelude::FluentBuilder};

impl AgentSurface {
	pub(super) fn handoff_items(&self) -> Vec<Handoff> {
		self.snapshot.as_ref().map_or_else(Vec::new, |snapshot| self.handoffs.items(snapshot))
	}

	pub(super) fn dock_items(&self) -> Vec<Handoff> {
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
		if !self.work_board.graph {
			self.work_board.focus_work(id);
		}
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
			.child(if self.work_board.graph {
				self.dock_status(cx)
			} else {
				gpui::div()
					.flex_1()
					.text_size(gpui::px(12.))
					.child(self.factory_summary())
					.into_any_element()
			})
			.child(self.workspace_action(
				"dock-toggle".into(),
				if compact { "Expand Dock" } else { "Collapse Dock" }.into(),
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
		if self.work_board.graph {
			header = header.child(self.workspace_action(
				"factory-overview".into(),
				"All agents".into(),
				|s, cx| {
					s.work_board.graph = false;
					cx.notify();
				},
				cx,
			));
		}
		if self.work_board.graph && self.workspace.dock_record.is_some() {
			header = header.child(self.workspace_action(
				"graph-home".into(),
				"All work".into(),
				|s, cx| {
					s.workspace.dock_record = s.root_id();
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
			.when(self.workspace.panel_drag.is_none(), |panel| panel.occlude())
			.capture_any_mouse_down(cx.listener(|s, _, _, _| {
				s.workspace.focused_panel = Some(super::workspace_size::Panel::Bottom)
			}))
			.child(header);
		if !compact && !self.work_board.graph {
			panel = panel.child(gpui::div().flex_1().min_h_0().child(self.render_work_board(cx)));
		} else if !compact {
			panel = panel
				.child(
					gpui::div()
						.px_3()
						.text_size(gpui::px(11.))
						.text_color(gpui::rgb(TEXT_MUTED))
						.child(self.graph_context()),
				)
				.child(gpui::div().flex_1().min_h_0().child(self.workspace_dependency_graph(cx)));
		}
		panel.into_any_element()
	}

	pub(super) fn expanded_graph_node(
		&self,
		node: &super::graph::Node,
		work: &super::AgentWorkItemDto,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let zoom = self.workspace.graph_display_zoom;
		let (width, height) = self.graph_node_size(&node.id);
		let key = format!("graph-node-{}", node.id);
		gpui::div()
			.id(super::SharedString::from(key.clone()))
			.debug_selector(move || key.clone())
			.role(super::Role::Group)
			.aria_label(format!("{}: task details", self.work_label(work)))
			.tab_index(0)
			.absolute()
			.left(gpui::px(
				node.x * zoom + self.workspace.graph_pan.0 + self.workspace.graph_inset.0,
			))
			.top(gpui::px(
				node.y * zoom + self.workspace.graph_pan.1 + self.workspace.graph_inset.1,
			))
			.w(gpui::px(width * zoom))
			.h(gpui::px(height * zoom))
			.overflow_hidden()
			.rounded(gpui::px(9.))
			.border_1()
			.border_color(gpui::rgba(0xffffff24))
			.bg(gpui::rgba(0x242427f8))
			.children(self.handoff_details(cx))
			.into_any_element()
	}

	fn dock_status(&self, cx: &mut Context<Self>) -> AnyElement {
		let items = self.dock_items();
		let mut row = gpui::div()
			.flex_1()
			.min_w_0()
			.flex()
			.items_center()
			.gap_2()
			.text_size(gpui::px(12.))
			.child(
				gpui::div().overflow_hidden().whitespace_nowrap().text_ellipsis().child(
					self.snapshot
						.as_ref()
						.and_then(|snapshot| {
							let scope =
								self.workspace.dock_record.clone().or_else(|| self.dock_scope());
							snapshot.work_items.iter().find(|work| Some(&work.id) == scope.as_ref())
						})
						.map(|work| self.work_label(work))
						.unwrap_or_else(|| "Work".into()),
				),
			);

		if let Some(item) = items.iter().find(|item| item.attention && !item.result) {
			let id = item.work.clone();
			let label = "Respond";
			row = row.child(
				gpui::div().flex_none().rounded(gpui::px(5.)).bg(gpui::rgba(0x6b9fff20)).child(
					self.workspace_action(
						"dock-next-action".into(),
						format!(
							"{label}: {}",
							self.snapshot
								.as_ref()
								.and_then(|s| s.work_items.iter().find(|w| w.id == id))
								.map(|w| self.work_label(w))
								.unwrap_or_default()
						),
						move |s, cx| {
							let scope = s.snapshot.as_ref().and_then(|snapshot| {
								let work = snapshot.work_items.iter().find(|w| w.id == id)?;
								if snapshot
									.work_items
									.iter()
									.any(|w| w.parent_goal_id.as_deref() == Some(&id))
								{
									Some(id.clone())
								} else {
									work.parent_goal_id.clone()
								}
							});
							s.workspace.dock_record = scope;
							s.workspace.dock_compact = false;
							s.activate_graph_node(&id, cx);
						},
						cx,
					),
				),
			);
		}
		row.into_any_element()
	}

	fn handoff_header(&self, work: &super::AgentWorkItemDto, cx: &mut Context<Self>) -> gpui::Div {
		let mut header = gpui::div().flex().items_center().gap_2().child(
			gpui::div()
				.flex_1()
				.min_w_0()
				.overflow_hidden()
				.whitespace_nowrap()
				.text_ellipsis()
				.child(self.work_label(work)),
		);

		let id = work.id.clone();
		header = header.child(self.workspace_action(
			"dock-open-source".into(),
			"Open conversation".into(),
			move |s, cx| {
				s.workspace.graph_expanded = false;
				s.open_page(&id, cx);
			},
			cx,
		));

		header = header.child(self.workspace_action(
			"handoff-close".into(),
			"×".into(),
			|s, cx| {
				s.handoffs.focus = None;
				cx.notify();
			},
			cx,
		));
		if self.workspace.dock_record.clone().or_else(|| self.dock_scope()).as_deref()
			!= Some(&work.id)
			&& self.snapshot.as_ref().is_some_and(|snapshot| {
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

	pub(super) fn handoff_details(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
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

		let mut header = if self.work_board.graph {
			self.handoff_header(work, cx)
		} else {
			gpui::div()
				.flex()
				.flex_col()
				.gap_2()
				.child(self.work_label(work))
				.child(gpui::div().text_color(gpui::rgb(state.color)).child(state.label))
		};
		if result && attention {
			let item = item.clone().expect("unread result");
			header = header.child(gpui::div().flex().child(self.workspace_action(
				"handoff-viewed".into(),
				"Mark viewed".into(),
				move |s, cx| {
					if let Some(snapshot) = &s.snapshot {
						s.handoffs.acknowledge(&item.work, &item.key, snapshot);
					}

					s.handoffs.focus = None;
					s.work_board.clear_focus();
					cx.notify();
				},
				cx,
			)));
		}
		let mut body = gpui::div()
			.id("handoff-preview")
			.occlude()
			.debug_selector(|| "handoff-preview".into())
			.w_full()
			.h_full()
			.min_h_0()
			.p_2()
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
					s.handoffs.focus = None;
					s.work_board.clear_focus();
					cx.stop_propagation();
					cx.notify();
				}
			}))
			.child(header);
		{
			let question =
				self.handoff_request_excerpt(work).or_else(|| self.handoff_decision_excerpt(work));
			if state.group != 3
				&& state.group != 1
				&& state.label != "Waiting on work"
				&& question.is_none()
			{
				body = body.child(state.reason);
			}
			body = body.child(self.dock_dependencies(work, cx));
			if result || !attention {
				body = body.child(
					self.execution_details(work, cx)
						.unwrap_or_else(|| self.overview_evidence(work, cx)),
				);
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
		}
		Some(body.into_any_element())
	}
}
