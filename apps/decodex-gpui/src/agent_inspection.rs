//! Compact conversation inspection, independent of transcript layout.
use gpui::{AnyElement, Div, MouseDownEvent};
use reqwest::Url;
use serde_json::Value;

use crate::{
	shell::agent_surface::{
		self, AgentSnapshotDto, AgentSurface, AgentWorkItemDto, Context, FluentBuilder, FontWeight,
		InteractiveElement, IntoElement, ParentElement, SharedString, StatefulInteractiveElement,
		Styled, graph, markdown, ui_theme::TEXT,
	},
	ui_loading,
	ui_theme::HOVER_FILL,
};
use decodex_protocol::AgentResourcesResult;

impl AgentSurface {
	pub(super) fn inspection_card(
		&self,
		snapshot: &AgentSnapshotDto,
		work: &AgentWorkItemDto,
		cx: &mut Context<Self>,
	) -> impl IntoElement {
		gpui::div()
			.id("work-inspection-scroll")
			.debug_selector(|| "work-inspection-scroll".into())
			.occlude()
			.max_h(gpui::px(380.))
			.overflow_y_scroll()
			.rounded(gpui::px(18.))
			.bg(gpui::rgb(0x29292e))
			.p(gpui::px(16.))
			.text_size(gpui::px(12.))
			.line_height(gpui::px(18.))
			.text_color(gpui::rgb(TEXT))
			.flex()
			.flex_col()
			.gap(gpui::px(14.))
			.on_mouse_down_out(cx.listener(|s, event: &MouseDownEvent, _, cx| {
				if s.menu_trigger_bounds
					.get("inspect-work")
					.is_some_and(|bounds| bounds.contains(&event.position))
				{
					return;
				}

				s.workspace.details_visible = false;

				cx.notify();
			}))
			.child(
				gpui::div()
					.flex()
					.items_center()
					.justify_between()
					.child(gpui::div().font_weight(FontWeight::MEDIUM).child(self.work_label(work)))
					.child(agent_surface::muted(graph::state_in(snapshot, work).0)),
			)
			.child(self.recap_panel(&work.id, cx))
			.child(self.inspection_resources(&work.id, cx))
			.child(self.native_receipts_panel(work, true, cx))
			.when_some(work.parent_goal_id.as_ref(), |panel, parent| {
				panel.child(self.relation("Reports to", snapshot, parent, cx))
			})
			.child(self.inspection_relations(snapshot, work, cx))
			.when_some(work.next_check_at_micros, |panel, due| {
				panel.child(agent_surface::muted(format!(
					"Next check · {}",
					agent_surface::next_check_text(due)
				)))
			})
			.when_some(work.codex_thread_id.as_ref(), |panel, thread| {
				panel.child(
					gpui::div()
						.flex()
						.items_center()
						.justify_between()
						.child(agent_surface::muted("Conversation reference"))
						.child(markdown::copy_button(
							&format!("work-reference-{}", work.id),
							"Copy task reference",
							format!("Work: {}\nThread: {}", work.id, thread),
						)),
				)
			})
	}

	fn inspection_relations(
		&self,
		snapshot: &AgentSnapshotDto,
		work: &AgentWorkItemDto,
		cx: &mut Context<Self>,
	) -> Div {
		let mut panel = gpui::div().flex().flex_col().gap(gpui::px(4.));

		for (label, id) in snapshot
			.dependencies
			.iter()
			.filter_map(|edge| {
				if edge.work_item_id == work.id {
					Some(("Requires", edge.depends_on_id.as_str()))
				} else if edge.depends_on_id == work.id {
					Some(("Required by", edge.work_item_id.as_str()))
				} else {
					None
				}
			})
			.chain(
				snapshot
					.work_items
					.iter()
					.filter(|child| child.parent_goal_id.as_ref() == Some(&work.id))
					.map(|child| ("Coordinates", child.id.as_str())),
			) {
			let selector = format!("inspection-{label}-{id}");

			panel = panel.child(
				gpui::div()
					.debug_selector(move || selector.clone())
					.child(self.relation(label, snapshot, id, cx)),
			);
		}

		panel
	}

	fn inspection_resources(&self, work: &str, cx: &mut Context<Self>) -> AnyElement {
		let mut rows = gpui::div().flex().flex_col().gap(gpui::px(4.));

		match self.resources.as_ref().filter(|(owner, _)| owner == work).map(|(_, result)| result) {
			Some(Some(AgentResourcesResult::Available { resources })) =>
				for resource in resources {
					let payload =
						serde_json::from_str::<Value>(&resource.payload_json).unwrap_or_default();
					let title = payload["title"]
						.as_str()
						.or_else(|| payload["name"].as_str())
						.unwrap_or(&resource.identity_key)
						.to_owned();
					let link = payload["url"].as_str().and_then(|url| Url::parse(url).ok()).filter(
						|url| {
							matches!(url.scheme(), "http" | "https")
								&& url.username().is_empty()
								&& url.password().is_none()
						},
					);
					let mut row = gpui::div()
						.id(SharedString::from(format!("inspection-resource-{}", resource.id)))
						.px(gpui::px(8.))
						.py(gpui::px(7.))
						.rounded(gpui::px(8.))
						.child(title);

					if let Some(link) = link {
						row = row
							.cursor_pointer()
							.hover(|row| row.bg(gpui::rgba(HOVER_FILL)))
							.on_click(cx.listener(move |_, _, _, cx| cx.open_url(link.as_str())));
					}

					rows = rows.child(row);
				},
			Some(None) => rows = rows.child(ui_loading::loading("Loading records")),
			Some(Some(
				AgentResourcesResult::Unavailable | AgentResourcesResult::CapacityExceeded,
			)) => rows = rows.child(agent_surface::muted("Records are unavailable.")),
			_ => {},
		}

		rows.into_any_element()
	}
}

#[cfg(test)]
mod tests {
	use std::thread;

	use crate::shell::agent_surface::inspection::AgentSurface;

	#[gpui::test]
	fn inspection_retains_dependencies_outside_the_current_graph_scope(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.snapshot.as_mut().unwrap().dependencies.push(decodex_protocol::AgentDependencyDto {
				work_item_id: "improve".into(),
				depends_on_id: "agent".into(),
			});

			s.selected = Some("improve".into());
			s.workspace.details_visible = true;
		});

		visual.update(|window, cx| window.draw(cx).clear());

		for selector in [
			"inspection-Requires-agent",
			"inspection-Requires-trace",
			"inspection-Required by-impact",
		] {
			assert!(visual.debug_bounds(selector).is_some(), "{selector}");
		}

		surface.update(visual, |s, cx| {
			s.selected = Some("release".into());

			cx.notify();
		});

		visual.update(|window, cx| window.draw(cx).clear());

		assert!(visual.debug_bounds("inspection-Coordinates-improve").is_some());
	}

	#[gpui::test]
	fn inspection_does_not_resize_or_scroll_the_transcript(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		surface.update(visual, |s, cx| s.visual_workspace_fixture(cx));

		for _ in 0..4 {
			visual.update(|window, cx| window.draw(cx).clear());
		}

		thread::sleep(std::time::Duration::from_millis(250));

		visual.update(|window, cx| window.draw(cx).clear());

		let before = surface.read_with(visual, |s, _| {
			(s.timeline.scroll["agent"].bounds(), s.timeline.scroll["agent"].offset())
		});

		surface.update(visual, |s, cx| {
			s.workspace.details_visible = true;

			cx.notify();
		});

		visual.update(|window, cx| window.draw(cx).clear());
		surface.read_with(visual, |s, _| {
			assert_eq!(s.timeline.scroll["agent"].bounds(), before.0);
			assert_eq!(s.timeline.scroll["agent"].offset(), before.1);
		});
	}
}
