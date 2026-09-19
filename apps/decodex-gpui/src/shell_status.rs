//! Quiet global status with explicit, read-only recovery controls.
use super::{
	AnyElement, ConnectionPresentation, Context, Destination, FluentBuilder, InteractiveElement,
	IntoElement, ParentElement, Role, Shell, StatefulInteractiveElement, Styled, WB_TEXT,
	WB_TEXT_MUTED, div, px, rgb, rgba, ui_theme,
};
use crate::ui_motion::{SmoothControl, popover};

impl Shell {
	pub(super) fn render_status_center(
		&self,
		connection: &ConnectionPresentation,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let notice = self.chief.read(cx).status_notice();
		let recent_event = self.chief.read(cx).recent_service_event();
		let (title, detail, retry, color) = if let Some((title, detail, retry)) = notice {
			(title, detail, retry, ui_theme::AMBER)
		} else if connection.label != "Online" {
			(connection.label, connection.detail.to_string(), false, connection.color)
		} else {
			("Status", "Connected to the local service.".into(), false, WB_TEXT_MUTED)
		};
		let panel = div()
			.occlude()
			.w(px(304.0))
			.p_3()
			.flex()
			.flex_col()
			.gap_2()
			.child(div().text_size(px(12.0)).text_color(rgb(WB_TEXT)).child(title))
			.child(
				div()
					.text_size(px(11.0))
					.line_height(px(17.0))
					.text_color(rgb(WB_TEXT_MUTED))
					.child(detail),
			)
			.when_some(recent_event, |panel, detail| {
				panel.child(
					div()
						.text_size(px(11.0))
						.text_color(rgb(WB_TEXT_MUTED))
						.child("Last saved service event")
						.child(div().mt_1().child(detail)),
				)
			})
			.child(
				div()
					.flex()
					.items_center()
					.gap_2()
					.when(retry, |row| {
						row.child(self.status_action("status-retry", "Refresh", true, cx))
					})
					.child(self.status_action("status-diagnostics", "Diagnostics", false, cx)),
			);
		div()
			.id("status-center")
			.w(px(304.0))
			.absolute()
			.bottom(px(12.0))
			.right(px(12.0))
			.flex()
			.flex_col()
			.items_end()
			.gap_2()
			.on_mouse_down_out(cx.listener(|s, _, _, cx| {
				if s.status_open {
					s.status_open = false;
					cx.notify();
				}
			}))
			.on_key_down(cx.listener(|s, event: &gpui::KeyDownEvent, _, cx| {
				if event.keystroke.key == "escape" {
					s.status_open = false;
					cx.notify();
				}
			}))
			.child(popover("status-panel-motion", "status", self.status_open, panel))
			.child(
				div()
					.id("status-toggle")
					.role(Role::Button)
					.tab_index(0)
					.aria_label(format!("Status: {title}"))
					.aria_expanded(self.status_open)
					.h(px(26.0))
					.px_2()
					.rounded(px(7.0))
					.bg(rgba(ui_theme::TOPBAR_MATERIAL))
					.flex()
					.items_center()
					.gap_2()
					.text_size(px(11.0))
					.text_color(rgb(WB_TEXT_MUTED))
					.cursor_pointer()
					.hover(|s| s.bg(rgba(0xffffff0c)))
					.on_click(cx.listener(|s, _, _, cx| {
						s.status_open = !s.status_open;
						cx.notify();
					}))
					.child(div().size(px(5.0)).rounded_full().bg(rgb(color)))
					.child(title)
					.smooth(),
			)
			.into_any_element()
	}

	fn status_action(
		&self,
		id: &'static str,
		label: &'static str,
		retry: bool,
		cx: &mut Context<Self>,
	) -> AnyElement {
		div()
			.id(id)
			.role(Role::Button)
			.tab_index(0)
			.aria_label(label)
			.h(px(26.0))
			.px_2()
			.rounded(px(5.0))
			.flex()
			.items_center()
			.text_size(px(11.0))
			.text_color(rgb(WB_TEXT))
			.bg(rgba(0xffffff08))
			.hover(|s| s.bg(rgba(0xffffff14)))
			.cursor_pointer()
			.on_click(cx.listener(move |s, _, _, cx| {
				if retry {
					s.chief.update(cx, |chief, cx| chief.refresh(cx));
				} else {
					s.status_open = false;
					s.select_destination(Destination::Health, cx);
				}
				cx.notify();
			}))
			.child(label)
			.smooth()
			.into_any_element()
	}
}
