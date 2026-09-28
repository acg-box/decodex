//! Compact response statistics with an anchored inspector, independent of transcript layout.
use super::{compact_tokens, markdown, ui_theme};
use decodex_protocol::AgentTurnUsageDto;
use gpui::{prelude::*, *};

#[derive(IntoElement)]
pub(super) struct ResponseMetrics {
	pub key: String,
	pub duration_ms: Option<u64>,
	pub status: Option<String>,
	pub usage: Option<AgentTurnUsageDto>,
	pub diagnostics: Option<String>,
}

fn row(label: &'static str, value: impl Into<SharedString>) -> Div {
	div()
		.flex()
		.items_center()
		.justify_between()
		.gap(px(16.))
		.child(div().text_color(rgb(ui_theme::TEXT_MUTED)).child(label))
		.child(div().text_color(rgb(ui_theme::TEXT)).child(value.into()))
}
fn heading(label: &'static str) -> Div {
	div()
		.mt(px(5.))
		.mb(px(1.))
		.text_size(px(10.))
		.font_weight(FontWeight::MEDIUM)
		.text_color(rgb(ui_theme::TEXT_MUTED))
		.child(label)
}
fn number(label: &'static str, value: Option<u64>) -> Option<Div> {
	value.map(|value| row(label, compact_tokens(value)))
}

impl RenderOnce for ResponseMetrics {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let state = window.use_keyed_state(
			SharedString::from(format!("response-details-{}", self.key)),
			cx,
			|_, _| (false, Bounds::<Pixels>::default()),
		);
		let open = state.read(cx).0;
		let anchor = state.read(cx).1.origin;
		let has_details = self.usage.is_some() || self.diagnostics.is_some();
		let progress = crate::ui_motion::value(
			SharedString::from(format!("response-details-motion-{}", self.key)),
			if open { 1. } else { 0. },
			window,
			cx,
		);
		let duration = self.duration_ms.map(|ms| {
			if ms >= 60_000 {
				format!("{}m {}s", ms / 60_000, ms % 60_000 / 1000)
			} else {
				format!("{:.1}s", ms as f64 / 1000.)
			}
		});
		let label = duration
			.as_ref()
			.map(|time| format!("Worked for {time}"))
			.or(self.status)
			.unwrap_or_default();
		let mut panel = div()
			.id(SharedString::from(format!("response-detail-panel-{}", self.key)))
			.debug_selector(|| "native-turn-usage".into())
			.occlude()
			.cursor_default()
			.on_click(|_, _, cx| cx.stop_propagation())
			.w(px(280.))
			.whitespace_normal()
			.p(px(12.))
			.rounded(px(14.))
			.bg(rgb(0x29292d))
			.text_size(px(11.))
			.line_height(px(15.))
			.flex()
			.flex_col()
			.gap(px(2.))
			.shadow(vec![BoxShadow {
				inset: false,
				color: rgba(0x00000024).into(),
				offset: point(px(0.), px(4.)),
				blur_radius: px(12.),
				spread_radius: px(-3.),
			}]);
		let mut header = div().flex().items_center().justify_between().child(
			div()
				.text_size(px(12.))
				.font_weight(FontWeight::MEDIUM)
				.text_color(rgb(ui_theme::TEXT))
				.child("Response details"),
		);
		if let Some(raw) = &self.diagnostics {
			header = header.child(markdown::copy_button(
				&format!("copy-diagnostics-{}", self.key),
				"Copy diagnostic details",
				raw.clone(),
			));
		}
		panel = panel.child(header).child(heading("This turn"));
		if let Some(duration) = duration {
			panel = panel.child(row("Duration", duration));
		}
		if let Some(usage) = &self.usage {
			panel = panel
				.child(row("Input", compact_tokens(usage.input_tokens)))
				.child(row("Output", compact_tokens(usage.output_tokens)));
			if let Some(details) = &usage.details {
				panel = panel.children(number("Model responses", details.responses));
				if details.last_input.is_some() || details.last_output.is_some() {
					panel = panel
						.child(heading("Last model response"))
						.children(number("Input", details.last_input))
						.children(number("Cached input", details.cached_input))
						.children(number("Output", details.last_output))
						.children(number("Reasoning", details.reasoning_output));
				}
				if details.thread_total.is_some() || details.context_capacity.is_some() {
					panel = panel
						.child(heading("Conversation"))
						.children(number("Total tokens", details.thread_total))
						.children(number("Context capacity", details.context_capacity));
				}
				if details.cached_input.is_some() || details.reasoning_output.is_some() {
					panel = panel.child(
						div()
							.mt(px(8.))
							.text_size(px(10.))
							.line_height(px(14.))
							.text_color(rgb(ui_theme::TEXT_MUTED))
							.child("Input includes cache; output includes reasoning."),
					);
				}
			}
		} else {
			panel = panel.child(
				div().text_color(rgb(ui_theme::TEXT_MUTED)).child("Token breakdown unavailable."),
			);
		}
		let dismiss = state.clone();
		panel = panel.on_mouse_down_out(move |event, _, cx| {
			if !dismiss.read(cx).1.contains(&event.position) {
				dismiss.update(cx, |state, cx| {
					state.0 = false;
					cx.notify();
				});
			}
		});
		let toggle = state.clone();
		let keyboard = state.clone();
		let measure = state.clone();
		let detail_button = div()
			.id(SharedString::from(format!("details-{}", self.key)))
			.debug_selector(|| "turn-details-toggle".into())
			.relative()
			.occlude()
			.size(px(24.))
			.flex_none()
			.role(Role::Button)
			.aria_label("Show response details")
			.aria_expanded(open)
			.tab_index(0)
			.cursor_pointer()
			.rounded(px(6.))
			.flex()
			.items_center()
			.justify_center()
			.hover(|d| d.bg(rgba(ui_theme::HOVER_FILL)))
			.child(
				div()
					.size(px(12.))
					.border_1()
					.border_color(rgb(ui_theme::TEXT_MUTED))
					.rounded_full()
					.flex()
					.flex_col()
					.items_center()
					.justify_center()
					.gap(px(1.3))
					.child(div().size(px(1.4)).rounded_full().bg(rgb(ui_theme::TEXT_MUTED)))
					.child(div().w(px(1.2)).h(px(4.)).rounded_full().bg(rgb(ui_theme::TEXT_MUTED))),
			)
			.child(
				canvas(
					move |bounds, _, cx| {
						measure.update(cx, |state, _| state.1 = bounds);
					},
					|_, _, _, _| {},
				)
				.absolute()
				.inset_0(),
			)
			.on_click(move |_, _, cx| {
				toggle.update(cx, |state, cx| {
					state.0 = !state.0;
					cx.notify();
				});
			})
			.on_key_down(move |event, _, cx| {
				if ["enter", "space"].contains(&event.keystroke.key.as_str()) {
					keyboard.update(cx, |state, cx| {
						state.0 = !state.0;
						cx.notify();
					});
					cx.stop_propagation();
				}
			})
			.when(progress > 0.001, |d| {
				d.child(
					deferred(
						anchored()
							.anchor(Anchor::BottomLeft)
							.position(anchor)
							.offset(point(px(0.), px(-6.)))
							.snap_to_window_with_margin(px(8.))
							.child(panel.opacity(progress)),
					)
					.with_priority(4),
				)
			});
		div()
			.flex_none()
			.whitespace_nowrap()
			.h(px(24.))
			.flex()
			.items_center()
			.gap(px(8.))
			.text_size(px(11.))
			.text_color(rgb(ui_theme::TEXT_MUTED))
			.when(!label.is_empty(), |d| d.child(label))
			.when_some(self.usage, |d, u| {
				d.child(format!(
					"In {} · Out {}",
					compact_tokens(u.input_tokens),
					compact_tokens(u.output_tokens)
				))
			})
			.when(has_details, |d| d.child(detail_button))
	}
}
