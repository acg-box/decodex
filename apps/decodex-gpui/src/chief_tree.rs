//! Agent ownership tree, separate from work dependencies in the graph.
use super::*;
use gpui::AnyElement;

fn children<'a>(snapshot: &'a ChiefSnapshotDto, parent: &str) -> Vec<&'a ChiefWorkItemDto> {
	snapshot
		.work_items
		.iter()
		.filter(|work| work.parent_goal_id.as_deref() == Some(parent))
		.collect()
}

impl ChiefSurface {
	pub(crate) fn toggle_agent_tree(&mut self, cx: &mut Context<Self>) {
		self.agent_tree_visible = !self.agent_tree_visible;
		cx.notify();
	}

	pub(super) fn agent_tree_width(&self, window: &Window) -> f32 {
		if !self.agent_tree_visible || self.graph_expanded || !self.has_work() {
			return 0.0;
		}
		let width = f32::from(window.viewport_size().width);
		let left = if self.sidebar_visible && width > 1000.0 { self.sidebar_width } else { 0.0 };
		(width - left - 440.0).clamp(0.0, 264.0)
	}

	pub(super) fn agent_tree(&self, cx: &mut Context<Self>) -> AnyElement {
		let mut list = div().id("agent-structure-list").flex_1().min_h_0().overflow_y_scroll();
		if let Some(snapshot) = &self.snapshot {
			for root in snapshot.work_items.iter().filter(|work| work.parent_goal_id.is_none()) {
				list = list.child(self.agent_branch(snapshot, root, 0, cx).0);
			}
		}
		div()
			.size_full()
			.text_size(px(12.0))
			.min_w_0()
			.flex()
			.flex_col()
			.pl(px(6.))
			.child(
				div()
					.h(px(ui_theme::PANEL_HEADER_HEIGHT))
					.flex_none()
					.bg(rgba(ui_theme::PANEL_HEADER_TINT))
					.px_2()
					.flex()
					.items_center()
					.child(
						div()
							.text_size(px(13.0))
							.font_weight(gpui::FontWeight::SEMIBOLD)
							.child("Agents"),
					),
			)
			.child(list)
			.into_any_element()
	}

	fn agent_branch(
		&self,
		snapshot: &ChiefSnapshotDto,
		work: &ChiefWorkItemDto,
		depth: usize,
		cx: &mut Context<Self>,
	) -> (AnyElement, usize) {
		let descendants = if depth < 24 { children(snapshot, &work.id) } else { Vec::new() };
		let expanded = !self.agent_tree_collapsed.contains(&work.id);
		let selected = self.selected.as_ref() == Some(&work.id);
		let toggle_id = work.id.clone();
		let keyboard_id = toggle_id.clone();
		let name = self.work_label(work);
		let id = work.id.clone();
		let (status, color) = graph::state_in(snapshot, work);
		let toggle = div()
			.id(SharedString::from(format!("agent-toggle-{}", work.id)))
			.role(Role::Button)
			.tab_index(0)
			.aria_label(format!("{} {name}", if expanded { "Collapse" } else { "Expand" }))
			.aria_expanded(expanded)
			.size(px(24.0))
			.flex_none()
			.flex()
			.items_center()
			.justify_center()
			.cursor_pointer()
			.text_color(rgb(ui_theme::TEXT_MUTED))
			.on_click(cx.listener(move |s, _, _, cx| {
				if !s.agent_tree_collapsed.remove(&toggle_id) {
					s.agent_tree_collapsed.insert(toggle_id.clone());
				}
				cx.notify();
			}))
			.on_key_down(cx.listener(move |s, event: &gpui::KeyDownEvent, _, cx| {
				if ["enter", "space"].contains(&event.keystroke.key.as_str()) {
					if !s.agent_tree_collapsed.remove(&keyboard_id) {
						s.agent_tree_collapsed.insert(keyboard_id.clone());
					}
					cx.stop_propagation();
					cx.notify();
				}
			}))
			.child(if expanded { "⌄" } else { "›" })
			.smooth();
		let row = div()
			.h(px(ui_theme::TREE_ROW_HEIGHT))
			.flex_none()
			.pl(px(8.0 + depth as f32 * 14.0))
			.pr_2()
			.flex()
			.items_center()
			.when(selected, |row| row.bg(rgba(0xffffff08)))
			.child(if descendants.is_empty() {
				div().w(px(24.0)).flex_none().into_any_element()
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
					.text_size(px(ui_theme::CAPTION_SIZE))
					.flex_none()
					.text_color(rgb(color))
					.child(status),
			);
		let mut nested = div().w_full().flex().flex_col();
		let mut count = 0;
		for child in descendants {
			let (branch, rows) = self.agent_branch(snapshot, child, depth + 1, cx);
			count += rows;
			nested = nested.child(branch);
		}
		(
			div()
				.w_full()
				.flex_none()
				.flex()
				.flex_col()
				.child(row)
				.child(crate::ui_motion::reveal(
					SharedString::from(format!("agent-children-{}", work.id)),
					if expanded { count as f32 * ui_theme::TREE_ROW_HEIGHT } else { 0.0 },
					false,
					nested,
				))
				.into_any_element(),
			1 + if expanded { count } else { 0 },
		)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	#[gpui::test]
	fn structure_uses_parentage_and_preserves_history_when_opening_workers(
		cx: &mut gpui::TestAppContext,
	) {
		let surface = cx.new(ChiefSurface::new);
		surface.update(cx, |s, cx| {
			s.visual_workspace_fixture(cx);
			let snapshot = s.snapshot.as_ref().unwrap();
			assert_eq!(children(snapshot, "chief").len(), 1);
			assert_eq!(children(snapshot, "release").len(), 6);
			s.agent_tree_collapsed.insert("release".into());
			s.open_page("verify", cx);
			assert_eq!(s.selected.as_deref(), Some("verify"));
			assert!(s.history_cache.contains_key("chief"));
			assert!(s.agent_tree_collapsed.contains("release"));
		});
	}
}
