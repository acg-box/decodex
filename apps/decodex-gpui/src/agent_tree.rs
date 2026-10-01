//! Agent ownership tree, separate from work dependencies in the graph.
use std::f32::consts::FRAC_PI_2;

use gpui::{AnyElement, Div, FontWeight, KeyDownEvent, PathBuilder, Stateful};
use ui_theme::{
	CAPTION_SIZE, HOVER_FILL, PANEL_HEADER_HEIGHT, SELECTED_HOVER_FILL, TEXT, TEXT_MUTED,
	TREE_ROW_HEIGHT,
};
use workspace_size::Panel;

use crate::{shell::agent_surface::*, ui_motion, ui_scroll::SmoothScrollArea};

pub(super) const DISCLOSURE: f32 = 18.;

const INSET: f32 = 8.;
const ROW_INSET: f32 = 4.;
const INDENT: f32 = 12.;

impl AgentSurface {
	pub(crate) fn toggle_agent_tree(&mut self, cx: &mut Context<Self>) {
		self.agent_tree_visible = !self.agent_tree_visible;

		cx.notify();
	}

	pub(super) fn agent_tree_width(&self, window: &Window) -> f32 {
		if !self.agent_tree_visible || self.graph_expanded || !self.reserve_workspace_panels() {
			return 0.0;
		}

		let width = f32::from(window.viewport_size().width);
		let left = if self.sidebar_visible && width > 1_000.0 { self.sidebar_width } else { 0.0 };

		self.agent_panel_width.min((width - left - 440.0).max(0.0))
	}

	pub(super) fn agent_tree(&self, cx: &mut Context<Self>) -> AnyElement {
		let mut list = div().id("agent-structure-list").flex_1().min_h_0().overflow_y_scroll();

		if let Some(snapshot) = &self.snapshot {
			for root in snapshot.work_items.iter().filter(|work| work.parent_goal_id.is_none()) {
				list = list.child(self.agent_branch(snapshot, root, 0, cx).0);
			}
		}

		div()
			.id("agent-panel-focus")
			.capture_any_mouse_down(cx.listener(|s, _, _, _| s.focused_panel = Some(Panel::Right)))
			.size_full()
			.text_size(px(12.0))
			.min_w_0()
			.flex()
			.flex_col()
			.px(px(INSET))
			.child(
				div()
					.h(px(PANEL_HEADER_HEIGHT))
					.flex_none()
					.px(px(ROW_INSET))
					.flex()
					.items_center()
					.child(
						div().text_size(px(13.0)).font_weight(FontWeight::SEMIBOLD).child("Agents"),
					),
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

		div()
			.id(SharedString::from(format!("agent-toggle-{id}")))
			.debug_selector({
				let id = id.clone();

				move || format!("agent-toggle-{id}")
			})
			.role(Role::Button)
			.tab_index(0)
			.aria_label(format!("{} {name}", if expanded { "Collapse" } else { "Expand" }))
			.aria_expanded(expanded)
			.size(px(DISCLOSURE))
			.flex_none()
			.flex()
			.items_center()
			.justify_center()
			.rounded(px(4.))
			.cursor_pointer()
			.hover(|s| s.text_color(rgb(TEXT)))
			.on_click(cx.listener(move |s, _, _, cx| {
				if !s.agent_tree_collapsed.remove(&click_id) {
					s.agent_tree_collapsed.insert(click_id.clone());
				}

				cx.notify();
			}))
			.on_key_down(cx.listener(move |s, event: &KeyDownEvent, _, cx| {
				if !event.is_held && ["enter", "space"].contains(&event.keystroke.key.as_str()) {
					if !s.agent_tree_collapsed.remove(&key_id) {
						s.agent_tree_collapsed.insert(key_id.clone());
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
		let expanded = !self.agent_tree_collapsed.contains(&work.id);
		let selected =
			self.native_agents.selected.is_none() && self.selected.as_ref() == Some(&work.id);
		let name = self.work_label(work);
		let id = work.id.clone();
		let (status, color) = graph::state_in(snapshot, work);
		let toggle = self.tree_toggle(work.id.clone(), &name, expanded, cx);
		let row = tree_row(format!("agent-row-{}", work.id), depth, selected)
			.child(if descendants.is_empty() && !has_native {
				div().w(px(DISCLOSURE)).flex_none().into_any_element()
			} else {
				toggle.into_any_element()
			})
			.child(div().flex_1().min_w_0().child(self.workspace_action(
				format!("agent-open-{id}"),
				name,
				move |s, cx| s.open_page(&id, cx),
				cx,
			)))
			.child(
				div()
					.text_size(px(CAPTION_SIZE))
					.flex_none()
					.text_color(rgb(color))
					.child(format!("L{depth} · {status}")),
			);
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
			div()
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

pub(super) fn tree_row(id: String, depth: usize, selected: bool) -> Stateful<Div> {
	div()
		.id(SharedString::from(id.clone()))
		.debug_selector(move || id.clone())
		.relative()
		.h(px(TREE_ROW_HEIGHT))
		.flex_none()
		.min_w_0()
		.pl(px(ROW_INSET + depth as f32 * INDENT))
		.pr(px(ROW_INSET))
		.flex()
		.items_center()
		.gap(px(4.))
		.rounded(px(5.))
		.when(selected, |row| row.bg(rgba(0xffffff0b)))
		.hover(move |row| row.bg(rgba(if selected { SELECTED_HOVER_FILL } else { HOVER_FILL })))
		.when(depth > 0, |row| {
			row.child(
				div()
					.absolute()
					.left(px(ROW_INSET + (depth - 1) as f32 * INDENT + DISCLOSURE / 2.))
					.top(px(TREE_ROW_HEIGHT / 2.))
					.w(px(5.))
					.h(px(1.))
					.bg(rgba(0xffffff18)),
			)
		})
}

pub(super) fn tree_children(depth: usize) -> Div {
	div().relative().w_full().flex().flex_col().child(
		div()
			.absolute()
			.left(px(ROW_INSET + depth as f32 * INDENT + DISCLOSURE / 2.))
			.top_0()
			.bottom(px(TREE_ROW_HEIGHT / 2.))
			.w(px(1.))
			.bg(rgba(0xffffff18)),
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
						px(x * angle.cos() - y * angle.sin()),
						px(x * angle.sin() + y * angle.cos()),
					)
			};
			let mut path = PathBuilder::stroke(px(1.2));

			path.move_to(point(-1.5, -3.));
			path.line_to(point(1.5, 0.));
			path.line_to(point(-1.5, 3.));

			if let Ok(path) = path.build() {
				window.paint_path(path, rgb(TEXT_MUTED));
			}
		},
	)
	.size(px(12.))
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
	use crate::shell::agent_surface::agent_tree::*;

	use std::thread;

	#[gpui::test]
	fn native_tree_disclosure_uses_its_own_hit_target(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(gpui::size(px(1_400.), px(1_200.)));

		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

			s.agent_tree_visible = true;
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

		assert_eq!(root.left(), native.left());
		assert_eq!(managed.right(), native.right());
		assert_eq!(arrow.center().y, root.center().y);
		assert_eq!(child_arrow.center().y, native.center().y);

		visual.simulate_click(child_arrow.center(), Default::default());
		surface.update(visual, |s, cx| {
			assert!(s.agent_tree_collapsed.contains("native:agent:native-child"));
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

			assert_eq!(children(snapshot, "agent").len(), 1);
			assert_eq!(children(snapshot, "release").len(), 6);

			s.agent_tree_collapsed.insert("release".into());
			s.open_page("verify", cx);

			assert_eq!(s.selected.as_deref(), Some("verify"));
			assert!(s.history_cache.contains_key("agent"));
			assert!(s.agent_tree_collapsed.contains("release"));
		});
	}
}
