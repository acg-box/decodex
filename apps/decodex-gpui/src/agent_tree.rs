//! Agent ownership tree, separate from work dependencies in the graph.
use std::{collections::BTreeSet, f32::consts::FRAC_PI_2};

use gpui::{AnyElement, Div, FontWeight, KeyDownEvent, PathBuilder, Stateful};
use ui_theme::{
	CAPTION_SIZE, HOVER_FILL, PANEL_HEADER_HEIGHT, SELECTED_HOVER_FILL, TEXT, TEXT_MUTED,
	TREE_ROW_HEIGHT,
};

use crate::{
	shell::agent_surface::{
		AgentSnapshotDto, AgentSurface, AgentWorkItemDto, Context, FluentBuilder,
		InteractiveElement, IntoElement, ParentElement, Role, SharedString, SmoothControl,
		StatefulInteractiveElement, Styled, Window, graph, ui_theme, workspace_size::Panel,
	},
	ui_motion,
	ui_scroll::SmoothScrollArea,
};

pub(super) const DISCLOSURE: f32 = 18.;

const INSET: f32 = 8.;
const ROW_INSET: f32 = 4.;
const INDENT: f32 = 12.;

impl AgentSurface {
	pub(crate) fn toggle_agent_tree(&mut self, cx: &mut Context<Self>) {
		self.workspace.agent_tree_visible = !self.workspace.agent_tree_visible;

		cx.notify();
	}

	pub(super) fn agent_tree_width(&self, window: &Window) -> f32 {
		if !self.workspace.agent_tree_visible
			|| self.workspace.graph_expanded
			|| !self.reserve_workspace_panels()
		{
			return 0.0;
		}

		let width = f32::from(window.viewport_size().width);
		let left = if self.workspace.sidebar_visible && width > 1_000.0 {
			self.workspace.sidebar_width
		} else {
			0.0
		};

		self.workspace.agent_panel_width.min((width - left - 440.0).max(0.0))
	}

	fn agent_count(&self) -> usize {
		let mut threads = BTreeSet::new();
		let mut count = 0;
		if let Some(snapshot) = &self.snapshot {
			for work in &snapshot.work_items {
				if work.codex_thread_id.as_ref().is_none_or(|id| threads.insert(id.clone())) {
					count += 1;
				}
			}
		}
		for agent in self.native_agents.lists.values().flatten() {
			if threads.insert(agent.thread_id.clone()) {
				count += 1;
			}
		}
		count
	}

	pub(super) fn agent_tree(&self, cx: &mut Context<Self>) -> AnyElement {
		let mut list =
			gpui::div().id("agent-structure-list").flex_1().min_h_0().overflow_y_scroll();

		if let Some(snapshot) = &self.snapshot {
			for root in snapshot.work_items.iter().filter(|work| work.parent_goal_id.is_none()) {
				list = list.child(self.agent_branch(snapshot, root, 0, cx).0);
			}
		}

		gpui::div()
			.id("agent-panel-focus")
			.capture_any_mouse_down(
				cx.listener(|s, _, _, _| s.workspace.focused_panel = Some(Panel::Right)),
			)
			.size_full()
			.text_size(gpui::px(12.0))
			.min_w_0()
			.flex()
			.flex_col()
			.px(gpui::px(INSET))
			.child(
				gpui::div()
					.h(gpui::px(PANEL_HEADER_HEIGHT))
					.flex_none()
					.px(gpui::px(ROW_INSET))
					.flex()
					.items_center()
					.child(
						gpui::div()
							.text_size(gpui::px(13.0))
							.font_weight(FontWeight::SEMIBOLD)
							.child("Agents"),
					)
					.when(self.snapshot.is_some(), |header| {
						header.child(
							gpui::div()
								.ml(gpui::px(6.))
								.text_size(gpui::px(CAPTION_SIZE))
								.text_color(gpui::rgb(TEXT_MUTED))
								.child(self.agent_count().to_string()),
						)
					}),
			)
			.child(list.smooth_scroll("agent-tree-scroll"))
			.into_any_element()
	}

	pub(super) fn tree_toggle(
		&self,
		id: String,
		name: &str,
		expanded: bool,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let click_id = id.clone();
		let key_id = id.clone();

		gpui::div()
			.id(SharedString::from(format!("agent-toggle-{id}")))
			.debug_selector({
				let id = id.clone();

				move || format!("agent-toggle-{id}")
			})
			.role(Role::Button)
			.tab_index(0)
			.aria_label(format!("{} {name}", if expanded { "Collapse" } else { "Expand" }))
			.aria_expanded(expanded)
			.size(gpui::px(DISCLOSURE))
			.flex_none()
			.flex()
			.items_center()
			.justify_center()
			.rounded(gpui::px(4.))
			.cursor_pointer()
			.hover(|s| s.text_color(gpui::rgb(TEXT)))
			.on_click(cx.listener(move |s, _, _, cx| {
				if !s.workspace.agent_tree_collapsed.remove(&click_id) {
					s.workspace.agent_tree_collapsed.insert(click_id.clone());
				}

				cx.notify();
			}))
			.on_key_down(cx.listener(move |s, event: &KeyDownEvent, _, cx| {
				if !event.is_held && ["enter", "space"].contains(&event.keystroke.key.as_str()) {
					if !s.workspace.agent_tree_collapsed.remove(&key_id) {
						s.workspace.agent_tree_collapsed.insert(key_id.clone());
					}

					cx.stop_propagation();
					cx.notify();
				}
			}))
			.child(chevron(format!("tree-chevron-{id}"), expanded))
			.smooth()
			.into_any_element()
	}

	fn agent_branch(
		&self,
		snapshot: &AgentSnapshotDto,
		work: &AgentWorkItemDto,
		depth: usize,
		cx: &mut Context<Self>,
	) -> (AnyElement, usize) {
		let has_native = work.codex_thread_id.as_ref().is_some_and(|thread| {
			self.native_agents
				.lists
				.get(&work.id)
				.is_some_and(|list| list.iter().any(|a| &a.parent_thread_id == thread))
		});
		let descendants = if depth < 24 { children(snapshot, &work.id) } else { Vec::new() };
		let expanded = !self.workspace.agent_tree_collapsed.contains(&work.id);
		let selected = self.selected.as_ref() == Some(&work.id)
			&& self.native_agents.selected.as_ref().is_none_or(|(owner, thread)| {
				owner == &work.id && work.codex_thread_id.as_ref() == Some(thread)
			});
		let name = self.work_label(work);
		let id = work.id.clone();
		let (status, _) = graph::state_in(snapshot, work);
		let toggle = self.tree_toggle(work.id.clone(), &name, expanded, cx);
		let row = tree_row(
			format!("agent-row-{}", work.id),
			depth,
			selected,
			!descendants.is_empty() || has_native,
			expanded,
		)
		.child(if descendants.is_empty() && !has_native {
			gpui::div().w(gpui::px(DISCLOSURE)).flex_none().into_any_element()
		} else {
			toggle.into_any_element()
		})
		.child(tree_identity(
			self.workspace_action(
				format!("agent-open-{id}"),
				name,
				move |s, cx| s.open_page(&id, cx),
				cx,
			),
			format!("agent-signal-{}", work.id),
			status,
		));
		let mut nested = tree_children(depth);
		let mut count = 0;

		for child in descendants {
			let (branch, rows) = self.agent_branch(snapshot, child, depth + 1, cx);

			count += rows;
			nested = nested.child(branch);
		}

		if let Some(thread) = &work.codex_thread_id {
			let (native, rows) = self.native_branches(&work.id, thread, depth + 1, cx);

			nested = nested.child(native);
			count += rows;
		}

		(
			gpui::div()
				.w_full()
				.flex_none()
				.flex()
				.flex_col()
				.child(row)
				.child(ui_motion::reveal(
					SharedString::from(format!("agent-children-{}", work.id)),
					if expanded { count as f32 * TREE_ROW_HEIGHT } else { 0.0 },
					false,
					nested,
				))
				.into_any_element(),
			1 + if expanded { count } else { 0 },
		)
	}
}

/// Keep a row's name and state together; indentation alone expresses ancestry.
pub(super) fn tree_identity(name: AnyElement, id: String, status: &str) -> Div {
	gpui::div()
		.min_w_0()
		.max_w_full()
		.flex()
		.items_center()
		.gap(gpui::px(4.))
		.child(ui_motion::AgentSignal { id: SharedString::from(id).into(), state: status.into() })
		.child(gpui::div().min_w_0().flex_shrink(1.).child(name))
}

pub(super) fn tree_row(
	id: String,
	depth: usize,
	selected: bool,
	has_children: bool,
	expanded: bool,
) -> Stateful<Div> {
	gpui::div()
		.id(SharedString::from(id.clone()))
		.debug_selector(move || id.clone())
		.relative()
		.h(gpui::px(TREE_ROW_HEIGHT))
		.flex_none()
		.min_w_0()
		.pl(gpui::px(ROW_INSET + depth as f32 * INDENT))
		.pr(gpui::px(ROW_INSET))
		.flex()
		.items_center()
		.gap(gpui::px(4.))
		.rounded(gpui::px(5.))
		.when(selected, |row| row.bg(gpui::rgba(0xffffff0b)))
		.hover(move |row| {
			row.bg(gpui::rgba(if selected { SELECTED_HOVER_FILL } else { HOVER_FILL }))
		})
		.when(depth > 0, |row| {
			row.child(
				gpui::div()
					.absolute()
					.left(gpui::px(ROW_INSET + (depth - 1) as f32 * INDENT + DISCLOSURE / 2.))
					.top(gpui::px(TREE_ROW_HEIGHT / 2.))
					.w(gpui::px(if has_children {
						INDENT - 5.
					} else {
						INDENT + DISCLOSURE / 2. + 12.
					}))
					.h(gpui::px(1.))
					.bg(gpui::rgba(0xffffff30)),
			)
		})
		.when(has_children && expanded, |row| {
			row.child(
				gpui::div()
					.absolute()
					.left(gpui::px(ROW_INSET + depth as f32 * INDENT + DISCLOSURE / 2.))
					.top(gpui::px(TREE_ROW_HEIGHT / 2. + 6.))
					.bottom_0()
					.w(gpui::px(1.))
					.bg(gpui::rgba(0xffffff30)),
			)
		})
}

pub(super) fn tree_children(depth: usize) -> Div {
	gpui::div().relative().w_full().flex().flex_col().child(
		gpui::div()
			.absolute()
			.left(gpui::px(ROW_INSET + depth as f32 * INDENT + DISCLOSURE / 2.))
			.top_0()
			.bottom(gpui::px(TREE_ROW_HEIGHT / 2.))
			.w(gpui::px(1.))
			.bg(gpui::rgba(0xffffff30)),
	)
}

fn chevron(id: String, expanded: bool) -> impl IntoElement {
	gpui::canvas(
		|_, _, _| (),
		move |bounds, _, window, cx| {
			let angle = ui_motion::value(
				SharedString::from(id.clone()),
				if expanded { FRAC_PI_2 } else { 0. },
				window,
				cx,
			);
			let center = bounds.center();
			let point = |x: f32, y: f32| {
				center
					+ gpui::point(
						gpui::px(x * angle.cos() - y * angle.sin()),
						gpui::px(x * angle.sin() + y * angle.cos()),
					)
			};
			let mut path = PathBuilder::stroke(gpui::px(1.2));

			path.move_to(point(-1.5, -3.));
			path.line_to(point(1.5, 0.));
			path.line_to(point(-1.5, 3.));

			if let Ok(path) = path.build() {
				window.paint_path(path, gpui::rgb(TEXT_MUTED));
			}
		},
	)
	.size(gpui::px(12.))
}

fn children<'a>(snapshot: &'a AgentSnapshotDto, parent: &str) -> Vec<&'a AgentWorkItemDto> {
	snapshot
		.work_items
		.iter()
		.filter(|work| work.parent_goal_id.as_deref() == Some(parent))
		.collect()
}

#[cfg(test)]
mod tests {
	use std::thread;

	use gpui::AppContext as _;

	use crate::shell::agent_surface::agent_tree::{self, AgentSurface};

	#[gpui::test]
	fn native_tree_disclosure_uses_its_own_hit_target(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_400.), gpui::px(1_200.)));

		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

			s.workspace.agent_tree_visible = true;
			s.snapshot
				.as_mut()
				.unwrap()
				.work_items
				.iter_mut()
				.find(|w| w.id == "agent")
				.unwrap()
				.codex_thread_id = Some("root-native".into());

			s.native_agents.lists.insert(
				"agent".into(),
				vec![
					decodex_protocol::NativeAgentDto {
						thread_id: "native-child".into(),
						parent_thread_id: "root-native".into(),
						title: "Research".into(),
						status: "idle".into(),
					},
					decodex_protocol::NativeAgentDto {
						thread_id: "native-grandchild".into(),
						parent_thread_id: "native-child".into(),
						title: "Sources".into(),
						status: "idle".into(),
					},
				],
			);
			let mut legacy = s.snapshot.as_ref().unwrap().work_items.last().unwrap().clone();
			legacy.kind = decodex_protocol::AgentWorkKindDto::Task;
			legacy.parent_goal_id = Some("decodex".into());
			legacy.id = "decodex-gpui-conversation-review".into();
			legacy.title = legacy.id.clone();
			assert_eq!(s.work_label(&legacy), "GPUI conversation review");
			legacy.title = "Conversation review".into();
			assert_eq!(s.work_label(&legacy), "Conversation review");
			let count = s.agent_count();
			let duplicate = s.native_agents.lists["agent"].clone();
			s.native_agents.lists.insert("release".into(), duplicate);
			assert_eq!(
				s.agent_count(),
				count,
				"ancestor lists must not count the same agent twice"
			);
			cx.notify();
		});

		visual.update(|w, cx| {
			w.draw(cx).clear();
		});

		// Hit-test the settled sidebar, after its entrance animation.
		thread::sleep(std::time::Duration::from_millis(240));

		visual.update(|w, cx| {
			w.draw(cx).clear();
		});

		let root = visual.debug_bounds("agent-row-agent").unwrap();
		let managed = visual.debug_bounds("agent-row-release").unwrap();
		let native = visual.debug_bounds("native-agent-row-native-child").unwrap();
		let arrow = visual.debug_bounds("agent-toggle-agent").unwrap();
		let child_arrow = visual.debug_bounds("agent-toggle-native:agent:native-child").unwrap();

		assert!(
			visual.debug_bounds("agent-open-agent").unwrap().size.width < gpui::px(100.),
			"short names must not stretch into a separate status column"
		);
		assert_eq!(root.left(), native.left());
		assert_eq!(managed.right(), native.right());
		assert_eq!(arrow.center().y, root.center().y);
		assert_eq!(child_arrow.center().y, native.center().y);

		visual.simulate_click(child_arrow.center(), Default::default());
		surface.update(visual, |s, cx| {
			assert!(s.workspace.agent_tree_collapsed.contains("native:agent:native-child"));
			assert_eq!(s.native_branches("agent", "root-native", 1, cx).1, 1);
		});
	}

	#[gpui::test]
	fn structure_uses_parentage_and_preserves_history_when_opening_workers(
		cx: &mut gpui::TestAppContext,
	) {
		let surface = cx.new(AgentSurface::new);

		surface.update(cx, |s, cx| {
			s.visual_workspace_fixture(cx);

			let snapshot = s.snapshot.as_ref().unwrap();

			assert_eq!(agent_tree::children(snapshot, "agent").len(), 1);
			assert_eq!(agent_tree::children(snapshot, "release").len(), 6);

			s.workspace.agent_tree_collapsed.insert("release".into());
			s.open_page("verify", cx);

			assert_eq!(s.selected.as_deref(), Some("verify"));
			assert!(s.timeline.cache.contains_key("agent"));
			assert!(s.workspace.agent_tree_collapsed.contains("release"));
		});
	}
}
