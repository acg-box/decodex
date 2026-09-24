//! Compact conversation inspection, independent of transcript layout.
use super::{
	ChiefSnapshotDto, ChiefSurface, ChiefWorkItemDto, Context, FluentBuilder, FontWeight,
	InteractiveElement, IntoElement, ParentElement, SharedString, StatefulInteractiveElement,
	Styled, div, graph, markdown, muted, next_check_text, px, rgb, rgba, ui_theme,
};

impl ChiefSurface {
	pub(super) fn inspection_card(
		&self,
		snapshot: &ChiefSnapshotDto,
		work: &ChiefWorkItemDto,
		cx: &mut Context<Self>,
	) -> impl IntoElement {
		div()
			.id("work-inspection-scroll")
			.debug_selector(|| "work-inspection-scroll".into())
			.occlude()
			.max_h(px(380.))
			.overflow_y_scroll()
			.rounded(px(18.))
			.bg(rgb(0x29292e))
			.p(px(16.))
			.text_size(px(12.))
			.line_height(px(18.))
			.text_color(rgb(ui_theme::TEXT))
			.flex()
			.flex_col()
			.gap(px(14.))
			.on_mouse_down_out(cx.listener(|s, event: &gpui::MouseDownEvent, _, cx| {
				if s.menu_trigger_bounds
					.get("inspect-work")
					.is_some_and(|bounds| bounds.contains(&event.position))
				{
					return;
				}
				s.details_visible = false;
				cx.notify();
			}))
			.child(
				div()
					.flex()
					.items_center()
					.justify_between()
					.child(div().font_weight(FontWeight::MEDIUM).child(self.work_label(work)))
					.child(muted(graph::state_in(snapshot, work).0)),
			)
			.child(self.inspection_resources(&work.id, cx))
			.when_some(work.parent_goal_id.as_ref(), |panel, parent| {
				panel.child(self.relation("Reports to", snapshot, parent, cx))
			})
			.when_some(work.next_check_at_micros, |panel, due| {
				panel.child(muted(format!("Next check · {}", next_check_text(due))))
			})
			.when_some(work.codex_thread_id.as_ref(), |panel, thread| {
				panel.child(
					div()
						.flex()
						.items_center()
						.justify_between()
						.child(muted("Conversation reference"))
						.child(markdown::copy_button(
							&format!("work-reference-{}", work.id),
							"Copy task reference",
							format!("Work: {}\nThread: {}", work.id, thread),
						)),
				)
			})
	}

	fn inspection_resources(&self, work: &str, cx: &mut Context<Self>) -> gpui::AnyElement {
		use decodex_protocol::ChiefResourcesResult;
		let mut rows = div().flex().flex_col().gap(px(4.));
		match self.resources.as_ref().filter(|(owner, _)| owner == work).map(|(_, result)| result) {
			Some(Some(ChiefResourcesResult::Available { resources })) =>
				for resource in resources {
					let payload = serde_json::from_str::<serde_json::Value>(&resource.payload_json)
						.unwrap_or_default();
					let title = payload["title"]
						.as_str()
						.or_else(|| payload["name"].as_str())
						.unwrap_or(&resource.identity_key)
						.to_owned();
					let link = payload["url"]
						.as_str()
						.and_then(|url| reqwest::Url::parse(url).ok())
						.filter(|url| {
							matches!(url.scheme(), "http" | "https")
								&& url.username().is_empty()
								&& url.password().is_none()
						});
					let mut row = div()
						.id(SharedString::from(format!("inspection-resource-{}", resource.id)))
						.px(px(8.))
						.py(px(7.))
						.rounded(px(8.))
						.child(title);
					if let Some(link) = link {
						row = row
							.cursor_pointer()
							.hover(|row| row.bg(rgba(0xffffff0c)))
							.on_click(cx.listener(move |_, _, _, cx| cx.open_url(link.as_str())));
					}
					rows = rows.child(row);
				},
			Some(None) => rows = rows.child(muted("Loading records…")),
			Some(Some(
				ChiefResourcesResult::Unavailable | ChiefResourcesResult::CapacityExceeded,
			)) => rows = rows.child(muted("Records are unavailable.")),
			_ => {},
		}
		rows.into_any_element()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	#[gpui::test]
	fn inspection_does_not_resize_or_scroll_the_transcript(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		surface.update(visual, |s, cx| s.visual_workspace_fixture(cx));
		for _ in 0..4 {
			visual.update(|window, cx| window.draw(cx).clear());
		}
		std::thread::sleep(std::time::Duration::from_millis(250));
		visual.update(|window, cx| window.draw(cx).clear());
		let before = surface.read_with(visual, |s, _| {
			(s.transcript_scroll["chief"].bounds(), s.transcript_scroll["chief"].offset())
		});
		surface.update(visual, |s, cx| {
			s.details_visible = true;
			cx.notify();
		});
		visual.update(|window, cx| window.draw(cx).clear());
		surface.read_with(visual, |s, _| {
			assert_eq!(s.transcript_scroll["chief"].bounds(), before.0);
			assert_eq!(s.transcript_scroll["chief"].offset(), before.1);
		});
	}
}
