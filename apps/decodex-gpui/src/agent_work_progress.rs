//! A compact, scoped view of the same native work shown in Graph.
use std::collections::BTreeSet;

use gpui::{AnyElement, FontWeight, KeyDownEvent, Role, prelude::*};

use crate::{
	shell::{
		agent_surface::{AgentSurface, Context},
		workspace_symbols::{self, Symbol},
	},
	ui_live_activity::ICON_SIZE,
	ui_motion::{self, OverflowLabel},
	ui_theme::{self, CONTROL_GROUP_HEIGHT, CONVERSATION_INSET, TEXT_MUTED},
};

impl AgentSurface {
	fn progress_rows(&self) -> Vec<super::Row> {
		let Some(current) = self.conversation_work() else { return Vec::new() };
		let Some(snapshot) = &self.snapshot else { return Vec::new() };
		let mut scope = BTreeSet::new();
		let mut pending = vec![current.id];
		while let Some(id) = pending.pop() {
			if scope.insert(id.clone()) {
				pending.extend(
					snapshot
						.work_items
						.iter()
						.filter(|work| work.parent_goal_id.as_deref() == Some(&id))
						.map(|work| work.id.clone()),
				);
			}
		}
		let mut rows: Vec<_> = self
			.factory_rows()
			.into_iter()
			.filter(|row| {
				if !scope.contains(&row.work) || row.group == 3 {
					return false;
				}
				row.native
					|| row.group <= 1
					|| self.snapshot.as_ref().is_some_and(|snapshot| {
						snapshot.work_items.iter().any(|work| {
							work.id == row.work
								&& work.kind == decodex_protocol::AgentWorkKindDto::Task
						})
					})
			})
			.collect();
		rows.sort_by(|a, b| a.group.cmp(&b.group).then(a.key.cmp(&b.key)));
		rows
	}

	fn work_progress_details(
		&self,
		rows: Vec<super::Row>,
		window: &gpui::Window,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let mut details = gpui::div()
			.id("work-progress-rows")
			.debug_selector(|| "work-progress-rows".into())
			.max_h(gpui::px(
				(f32::from(window.viewport_size().height) * 0.25)
					.clamp(2. * CONTROL_GROUP_HEIGHT, 6. * CONTROL_GROUP_HEIGHT),
			))
			.overflow_y_scroll()
			.px(gpui::px(8.));
		for row in rows {
			let id = row.key.clone();
			let title = self.graph_purpose(&row);
			let title = if title.is_empty() { row.title.clone() } else { title };
			let action = self.work_activity_summary(&row).1;
			let label = if action.is_empty() { row.status.clone() } else { action };
			let target = row.clone();
			details = details.child(
				gpui::div()
					.id(format!("work-progress-{id}"))
					.debug_selector(move || format!("work-progress-{id}"))
					.h(gpui::px(CONTROL_GROUP_HEIGHT))
					.w_full()
					.min_w_0()
					.flex()
					.items_center()
					.gap(gpui::px(8.))
					.role(Role::Button)
					.tab_index(0)
					.aria_label(format!("Open {title}: {label}"))
					.rounded(gpui::px(5.))
					.px(gpui::px(4.))
					.hover(|s| s.bg(ui_theme::rgb(ui_theme::HOVER_FILL)))
					.child(
						gpui::div()
							.size(gpui::px(5.))
							.flex_none()
							.rounded_full()
							.bg(ui_theme::rgb(row.color)),
					)
					.child(gpui::div().flex_1().min_w_0().child(OverflowLabel {
						id: format!("work-progress-title-{}", row.key).into(),
						text: title.into(),
					}))
					.child(
						gpui::div()
							.max_w(gpui::px(180.))
							.min_w_0()
							.text_color(ui_theme::rgb(TEXT_MUTED))
							.child(OverflowLabel {
								id: format!("work-progress-state-{}", row.key).into(),
								text: label.into(),
							}),
					)
					.on_click(cx.listener(move |s, _, _, cx| s.open_graph_agent(&target, cx)))
					.on_key_down(cx.listener(move |s, event: &KeyDownEvent, _, cx| {
						if ["enter", "space"].contains(&event.keystroke.key.as_str()) {
							s.open_graph_agent(&row, cx);
							cx.stop_propagation();
						}
					})),
			)
		}
		details.into_any_element()
	}

	pub(in super::super) fn work_progress_bar(
		&self,
		window: &gpui::Window,
		cx: &mut Context<Self>,
	) -> Option<AnyElement> {
		if self.workspace.graph_visible || self.is_new_conversation() {
			return None;
		}
		let rows = self.progress_rows();
		if rows.is_empty() {
			return None;
		}
		let expanded = self.workspace.work_progress_expanded;
		let count = rows.len();
		let details = self.work_progress_details(rows, window, cx);
		let toggle = |s: &mut Self, cx: &mut Context<Self>| {
			ui_motion::begin_interaction();
			s.workspace.work_progress_expanded = !s.workspace.work_progress_expanded;
			cx.notify();
		};
		Some(
			gpui::div()
				.debug_selector(|| "work-progress".into())
				.flex_none()
				.mx(gpui::px(CONVERSATION_INSET))
				.mb(gpui::px(8.))
				.rounded(gpui::px(8.))
				.bg(ui_theme::annotation_surface())
				.text_size(gpui::px(11.))
				.line_height(gpui::px(16.))
				.child(
					gpui::div()
						.id("work-progress-toggle")
						.debug_selector(|| "work-progress-toggle".into())
						.h(gpui::px(CONTROL_GROUP_HEIGHT))
						.px(gpui::px(8.))
						.flex()
						.items_center()
						.gap(gpui::px(6.))
						.role(Role::Button)
						.tab_index(0)
						.aria_expanded(expanded)
						.aria_label(format!("Work in progress ({count})"))
						.child(
							gpui::div().text_color(ui_theme::rgb(ui_theme::ACCENT)).child(
								workspace_symbols::icon_sized(Symbol::WorkActivity, ICON_SIZE),
							),
						)
						.child(
							gpui::div()
								.flex_1()
								.font_weight(FontWeight::MEDIUM)
								.child("Work in progress"),
						)
						.child(
							gpui::div()
								.text_color(ui_theme::rgb(TEXT_MUTED))
								.child(format!("({count})")),
						)
						.child(workspace_symbols::disclosure_chevron(
							"work-progress-chevron",
							expanded,
						))
						.on_click(cx.listener(move |s, _, _, cx| toggle(s, cx)))
						.on_key_down(cx.listener(move |s, event: &KeyDownEvent, _, cx| {
							if ["enter", "space"].contains(&event.keystroke.key.as_str()) {
								toggle(s, cx);
								cx.stop_propagation();
							}
						})),
				)
				.child(ui_motion::disclosure("work-progress-details", expanded, details))
				.into_any_element(),
		)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[gpui::test]
	fn progress_uses_scoped_work_and_preserves_the_draft(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.workspace.graph_visible = false;
			s.composer.update(cx, |input, cx| input.set_content("Keep this draft", cx));
			let mut foreign = s.snapshot.as_ref().unwrap().work_items[0].clone();
			foreign.id = "foreign-work".into();
			foreign.parent_goal_id = None;
			s.snapshot.as_mut().unwrap().work_items.push(foreign);
			let rows = s.progress_rows();
			assert!(!rows.is_empty());
			assert!(rows.iter().all(|row| row.work != "foreign-work"));
			assert!(rows.iter().any(|row| row.work == "verify"));
			assert!(rows.iter().any(|row| row.work == "improve"));
			assert!(rows.iter().all(|row| row.work != "flow" && row.work != "trace"));
		});
		visual.update(crate::ui_frame::draw_current_frame);
		let toggle = visual.debug_bounds("work-progress-toggle").unwrap();
		visual.simulate_click(toggle.center(), Default::default());
		visual.update(crate::ui_frame::draw_current_frame);
		surface.read_with(visual, |s, cx| {
			assert!(s.workspace.work_progress_expanded);
			assert_eq!(s.composer.read(cx).content(), "Keep this draft");
		});
		surface.update(visual, |s, cx| {
			s.workspace.graph_visible = true;
			cx.notify();
		});
		visual.update(crate::ui_frame::draw_current_frame);
		assert!(visual.debug_bounds("work-progress-toggle").is_none());
	}
}
