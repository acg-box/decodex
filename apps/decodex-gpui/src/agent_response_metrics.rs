//! Compact response statistics with hover details, independent of transcript layout.
use std::f32::consts::TAU;

use gpui::{
	self, Anchor, AnyElement, App, Bounds, BoxShadow, Div, FontWeight, IntoElement, PathBuilder,
	Pixels, RenderOnce, SharedString, Window,
	prelude::{
		FluentBuilder as _, InteractiveElement as _, ParentElement as _,
		StatefulInteractiveElement as _, Styled as _,
	},
};

use crate::shell::agent_surface::{
	self,
	ui_theme::{CAPTION_SIZE, TEXT, TEXT_MUTED},
};
use decodex_protocol::AgentTurnUsageDto;

#[derive(IntoElement)]
pub(super) struct ResponseMetrics {
	pub key: String,
	pub duration_ms: Option<u64>,
	pub status: Option<String>,
	pub usage: Option<AgentTurnUsageDto>,
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
		let duration = self.duration_ms.map(duration_label);
		let label = duration.clone().or(self.status).unwrap_or_default();
		let has_duration = duration.is_some();
		let panel = if open {
			details_panel(&self.key, duration, self.usage.as_ref())
		} else {
			gpui::div().into_any_element()
		};
		let hover_state = state.clone();
		let measure = state.clone();

		gpui::div()
			.id(SharedString::from(format!("response-metrics-{}", self.key)))
			.debug_selector(|| "turn-metrics-hover".into())
			.relative()
			.cursor_default()
			.flex_none()
			.whitespace_nowrap()
			.h(gpui::px(24.))
			.flex()
			.items_center()
			.gap(gpui::px(6.))
			.text_size(gpui::px(CAPTION_SIZE))
			.text_color(gpui::rgb(TEXT_MUTED))
			.when(!label.is_empty(), |d| {
				d.child(
					gpui::div()
						.flex()
						.items_center()
						.gap(gpui::px(3.))
						.when(has_duration, |d| d.child(duration_icon()))
						.child(label),
				)
			})
			.when_some(self.usage, |d, u| {
				d.child(
					gpui::div()
						.flex()
						.items_center()
						.gap(gpui::px(6.))
						.child(format!("↑ {}", agent_surface::compact_tokens(u.input_tokens)))
						.child(format!("↓ {}", agent_surface::compact_tokens(u.output_tokens))),
				)
			})
			.when(has_details, |d| {
				d.hover(|d| d.text_color(gpui::rgb(TEXT)))
					.on_hover(move |hovered, _, cx| {
						hover_state.update(cx, |state, cx| {
							state.0 = *hovered;

							cx.notify();
						});
					})
					.child(
						gpui::canvas(
							move |bounds, _, cx| {
								measure.update(cx, |state, _| state.1 = bounds);
							},
							|_, _, _, _| {},
						)
						.absolute()
						.inset_0(),
					)
					.when(open, |d| {
						d.child(
							gpui::deferred(
								gpui::anchored()
									.anchor(Anchor::BottomLeft)
									.position(anchor)
									.offset(gpui::point(gpui::px(0.), gpui::px(-6.)))
									.snap_to_window_with_margin(gpui::px(8.))
									.child(panel),
							)
							.with_priority(4),
						)
					})
			})
	}
}

fn row(label: &'static str, value: impl Into<SharedString>) -> Div {
	gpui::div()
		.flex()
		.items_center()
		.justify_between()
		.gap(gpui::px(16.))
		.child(gpui::div().text_color(gpui::rgb(TEXT_MUTED)).child(label))
		.child(gpui::div().text_color(gpui::rgb(TEXT)).child(value.into()))
}

fn usage_row(label: &'static str, input: Option<u64>, output: Option<u64>) -> Div {
	gpui::div()
		.flex()
		.items_center()
		.h(gpui::px(22.))
		.child(gpui::div().flex_1().text_color(gpui::rgb(TEXT_MUTED)).child(label))
		.children([input, output].into_iter().map(|value| {
			gpui::div().w(gpui::px(58.)).text_right().child(
				value
					.map(crate::shell::agent_surface::compact_tokens)
					.unwrap_or_else(|| "—".into()),
			)
		}))
}

fn number(label: &'static str, value: Option<u64>) -> Option<Div> {
	value.map(|value| row(label, agent_surface::compact_tokens(value)))
}

fn duration_label(ms: u64) -> String {
	if ms >= 3_600_000 {
		format!("{}h {}m", ms / 3_600_000, ms % 3_600_000 / 60_000)
	} else if ms >= 60_000 {
		format!("{}m {}s", ms / 60_000, ms % 60_000 / 1_000)
	} else {
		format!("{:.1}s", ms as f64 / 1_000.)
	}
}

fn duration_icon() -> impl IntoElement {
	gpui::canvas(
		|_, _, _| (),
		|bounds, _, window, _| {
			let mut path = PathBuilder::stroke(gpui::px(1.));

			for step in 0..=32 {
				let angle = step as f32 * TAU / 32.;
				let p = bounds.origin
					+ gpui::point(
						gpui::px(5.5 + 4.25 * angle.cos()),
						gpui::px(5.5 + 4.25 * angle.sin()),
					);

				if step == 0 {
					path.move_to(p);
				} else {
					path.line_to(p);
				}
			}

			path.move_to(bounds.origin + gpui::point(gpui::px(5.5), gpui::px(2.5)));
			path.line_to(bounds.origin + gpui::point(gpui::px(5.5), gpui::px(5.5)));
			path.line_to(bounds.origin + gpui::point(gpui::px(7.5), gpui::px(6.5)));

			if let Ok(path) = path.build() {
				window.paint_path(path, gpui::rgb(TEXT_MUTED));
			}
		},
	)
	.size(gpui::px(11.))
	.flex_none()
}

fn details_panel(
	key: &str,
	duration: Option<String>,
	usage: Option<&AgentTurnUsageDto>,
) -> AnyElement {
	let mut panel = gpui::div()
		.id(SharedString::from(format!("response-detail-panel-{}", key)))
		.debug_selector(|| "native-turn-usage".into())
		.occlude()
		.cursor_default()
		.on_click(|_, _, cx| cx.stop_propagation())
		.w(gpui::px(272.))
		.whitespace_normal()
		.p(gpui::px(14.))
		.rounded(gpui::px(12.))
		.bg(gpui::rgb(0x29292d))
		.text_size(gpui::px(12.))
		.line_height(gpui::px(15.))
		.flex()
		.flex_col()
		.gap(gpui::px(2.))
		.shadow(vec![BoxShadow {
			inset: false,
			color: gpui::rgba(0x00000024).into(),
			offset: gpui::point(gpui::px(0.), gpui::px(4.)),
			blur_radius: gpui::px(12.),
			spread_radius: gpui::px(-3.),
		}]);

	panel = panel.child(
		gpui::div()
			.flex()
			.justify_between()
			.mb(gpui::px(8.))
			.child(
				gpui::div()
					.text_size(gpui::px(13.))
					.font_weight(FontWeight::MEDIUM)
					.child("Turn details"),
			)
			.child(
				gpui::div().text_color(gpui::rgb(TEXT_MUTED)).child(duration.unwrap_or_default()),
			),
	);

	if let Some(usage) = usage {
		panel = panel
			.child(
				gpui::div()
					.flex()
					.justify_end()
					.text_color(gpui::rgb(TEXT_MUTED))
					.child(gpui::div().w(gpui::px(58.)).text_right().child("Input"))
					.child(gpui::div().w(gpui::px(58.)).text_right().child("Output")),
			)
			.child(usage_row("This turn", Some(usage.input_tokens), Some(usage.output_tokens)));

		if let Some(details) = &usage.details {
			if details.last_input.is_some() || details.last_output.is_some() {
				panel = panel.child(usage_row(
					"Last response",
					details.last_input,
					details.last_output,
				));
			}
			if details.cached_input.is_some() {
				panel = panel.child(usage_row("  Cached", details.cached_input, None));
			}
			if details.reasoning_output.is_some() {
				panel = panel.child(usage_row("  Reasoning", None, details.reasoning_output));
			}

			panel = panel
				.child(gpui::div().h(gpui::px(6.)))
				.children(number("Model responses", details.responses))
				.children(number("Conversation total", details.thread_total))
				.children(number("Context capacity", details.context_capacity));
		}
	}

	panel.into_any_element()
}
