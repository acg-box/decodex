//! Compact response statistics with hover details, independent of transcript layout.
use super::{compact_tokens, ui_theme};
use decodex_protocol::AgentTurnUsageDto;
use gpui::{prelude::*, *};

#[derive(IntoElement)]
pub(super) struct ResponseMetrics {
	pub key: String,
	pub duration_ms: Option<u64>,
	pub status: Option<String>,
	pub usage: Option<AgentTurnUsageDto>,
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

fn duration_label(ms: u64) -> String {
	if ms >= 3_600_000 {
		format!("{}h {}m", ms / 3_600_000, ms % 3_600_000 / 60_000)
	} else if ms >= 60_000 {
		format!("{}m {}s", ms / 60_000, ms % 60_000 / 1000)
	} else {
		format!("{:.1}s", ms as f64 / 1000.)
	}
}

fn duration_icon() -> impl IntoElement {
	canvas(
		|_, _, _| (),
		|bounds, _, window, _| {
			let mut path = PathBuilder::stroke(px(1.));
			for step in 0..=32 {
				let angle = step as f32 * std::f32::consts::TAU / 32.;
				let p = bounds.origin
					+ point(px(5.5 + 4.25 * angle.cos()), px(5.5 + 4.25 * angle.sin()));
				if step == 0 {
					path.move_to(p);
				} else {
					path.line_to(p);
				}
			}
			path.move_to(bounds.origin + point(px(5.5), px(2.5)));
			path.line_to(bounds.origin + point(px(5.5), px(5.5)));
			path.line_to(bounds.origin + point(px(7.5), px(6.5)));
			if let Ok(path) = path.build() {
				window.paint_path(path, rgb(ui_theme::TEXT_MUTED));
			}
		},
	)
	.size(px(11.))
	.flex_none()
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
		let has_details = self.usage.is_some();
		let progress = crate::ui_motion::popover_progress(
			SharedString::from(format!("response-details-motion-{}", self.key)),
			"response-details",
			open,
			window,
			cx,
		);

		let duration = self.duration_ms.map(duration_label);
		let label = duration.clone().or(self.status).unwrap_or_default();
		let has_duration = duration.is_some();
		let panel = if progress > 0.001 {
			let mut panel = div()
				.id(SharedString::from(format!("response-detail-panel-{}", self.key)))
				.debug_selector(|| "native-turn-usage".into())
				.occlude()
				.cursor_default()
				.on_click(|_, _, cx| cx.stop_propagation())
				.w(px(220.))
				.whitespace_normal()
				.p(px(10.))
				.rounded(px(12.))
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
			panel = panel.child(heading("This turn"));
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
				}
			}
			panel.into_any_element()
		} else {
			div().into_any_element()
		};
		let hover_state = state.clone();
		let measure = state.clone();
		div()
			.id(SharedString::from(format!("response-metrics-{}", self.key)))
			.debug_selector(|| "turn-metrics-hover".into())
			.relative()
			.cursor_default()
			.flex_none()
			.whitespace_nowrap()
			.h(px(24.))
			.flex()
			.items_center()
			.gap(px(6.))
			.text_size(px(ui_theme::CAPTION_SIZE))
			.text_color(rgb(ui_theme::TEXT_MUTED))
			.when(!label.is_empty(), |d| {
				d.child(
					div()
						.flex()
						.items_center()
						.gap(px(3.))
						.when(has_duration, |d| d.child(duration_icon()))
						.child(label),
				)
			})
			.when_some(self.usage, |d, u| {
				d.child(
					div()
						.flex()
						.items_center()
						.gap(px(6.))
						.child(format!("↑ {}", compact_tokens(u.input_tokens)))
						.child(format!("↓ {}", compact_tokens(u.output_tokens))),
				)
			})
			.when(has_details, |d| {
				d.hover(|d| d.text_color(rgb(ui_theme::TEXT)))
					.on_hover(move |hovered, _, cx| {
						hover_state.update(cx, |state, cx| {
							state.0 = *hovered;
							cx.notify();
						});
					})
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
					.when(progress > 0.001, |d| {
						d.child(
							deferred(
								anchored()
									.anchor(Anchor::BottomLeft)
									.position(anchor)
									.offset(point(px(0.), px(-6. + (1. - progress) * 4.)))
									.snap_to_window_with_margin(px(8.))
									.child(panel),
							)
							.with_priority(4),
						)
					})
			})
	}
}
