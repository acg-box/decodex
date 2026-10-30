//! Compact response statistics with hover details, independent of transcript layout.

use std::{
	cell::Cell,
	rc::Rc,
	time::{Duration, Instant},
};

use gpui::{
	self, Anchor, AnyElement, App, Bounds, Context, Div, IntoElement, Pixels, RenderOnce, Role,
	SharedString, Stateful, Task, Window,
	prelude::{
		FluentBuilder as _, InteractiveElement as _, ParentElement as _,
		StatefulInteractiveElement as _, Styled as _,
	},
};

use crate::{
	shell::agent_surface::{
		self, markdown,
		ui_theme::{CAPTION_SIZE, TEXT, TEXT_MUTED},
	},
	ui_motion::{self, Tween},
	ui_theme::{self, METADATA_GAP},
};
use decodex_protocol::{AgentHistoryEntryDto, AgentTurnTimingDto, AgentTurnUsageDto};

const METRICS_HEIGHT: f32 = 24.;

#[derive(IntoElement)]
pub(super) struct FinishedFooter {
	pub visible: bool,
	pub ready: Option<Rc<Cell<bool>>>,
	pub child: Option<AnyElement>,
}
impl RenderOnce for FinishedFooter {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let target =
			if self.visible && self.ready.is_none_or(|ready| ready.get()) { 1. } else { 0. };
		// Keep this slot mounted while streaming. Saved replies start at their final size.
		let state = window.use_keyed_state("response-footer-motion", cx, |_, _| Tween::new(target));
		let now = Instant::now();
		let (progress, moving) = state.update(cx, |motion, _| {
			motion.target(target, now);

			(motion.sample(now), motion.moving(now))
		});

		if moving {
			ui_motion::request_frame(window, cx);
		}

		gpui::div()
			.debug_selector(|| "response-footer".into())
			.w_full()
			.flex_none()
			.h(gpui::px((METRICS_HEIGHT + METADATA_GAP) * progress))
			.when(progress < 1., |row| row.overflow_hidden())
			.opacity(progress)
			.children(self.child)
	}
}

#[derive(IntoElement)]
pub(super) struct ResponseMetrics {
	pub key: String,
	pub duration_ms: Option<u64>,
	pub status: Option<String>,
	pub usage: Option<AgentTurnUsageDto>,
	pub timing: Option<AgentTurnTimingDto>,
}
impl RenderOnce for ResponseMetrics {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let details = Rc::new(MetricsDetails::new(&self));
		let duration = details.duration.clone();
		let has_duration = duration.is_some();
		let label = duration.or(self.status).unwrap_or_else(|| "—".into());
		let state = window.use_keyed_state(
			SharedString::from(format!("turn-details-{}", self.key)),
			cx,
			|_, _| MetricsPopover::default(),
		);
		let (open, anchor) = state.read_with(cx, |state, _| (state.open, state.anchor));
		let hover = state.clone();
		let click = state.clone();
		let measure = state.clone();
		let panel_hover = state.clone();
		let outside = state.clone();
		let dismiss = state.clone();
		let key = SharedString::from(format!("turn-metrics-popover-{}", self.key));

		gpui::div()
			.id(SharedString::from(format!("response-metrics-{}", self.key)))
			.debug_selector(|| "turn-metrics-hover".into())
			.role(Role::Button)
			.aria_label("Turn metrics")
			.relative()
			.cursor_pointer()
			.flex_none()
			.whitespace_nowrap()
			.h(gpui::px(METRICS_HEIGHT))
			.flex()
			.items_center()
			.gap(gpui::px(6.))
			.text_size(gpui::px(CAPTION_SIZE))
			.text_color(ui_theme::rgb(TEXT_MUTED))
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
			.hover(|d| d.text_color(ui_theme::rgb(TEXT)))
			.active(|d| d.text_color(ui_theme::rgb(TEXT).opacity(0.8)))
			.on_hover(move |inside, _, cx| {
				hover.update(cx, |state, cx| state.hover(HoverTarget::Trigger, *inside, cx))
			})
			.on_click(move |_, _, cx| {
				cx.stop_propagation();
				click.update(cx, |state, cx| {
					let pinned = state.pinned;
					state.set_open(!pinned, !pinned, cx);
				});
			})
			.child(
				gpui::canvas(
					move |bounds, _, cx| measure.update(cx, |state, _| state.anchor = bounds),
					|_, _, _, _| {},
				)
				.absolute()
				.inset_0(),
			)
			.child(
				gpui::deferred(
					gpui::anchored()
						.anchor(Anchor::BottomLeft)
						.position(anchor.origin)
						.offset(gpui::point(gpui::px(0.), gpui::px(-ui_theme::POPOVER_GAP)))
						.snap_to_window_with_margin(gpui::px(8.))
						.child(
							gpui::div().w(gpui::px(260.)).child(
								ui_motion::popover_lazy(key, open, move |_, _| {
									let panel_hover = panel_hover.clone();
									let outside = outside.clone();
									details_panel(&details)
										.on_hover(move |inside, _, cx| {
											panel_hover.update(cx, |state, cx| {
												state.hover(HoverTarget::Panel, *inside, cx)
											})
										})
										.on_mouse_down_out(move |event, _, cx| {
											outside.update(cx, |state, cx| {
												if !state.anchor.contains(&event.position) {
													state.set_open(false, false, cx);
												}
											})
										})
										.into_any_element()
								})
								.preview()
								.above()
								.trigger(Rc::new(Cell::new(Some(anchor))))
								.on_dismiss(move |cx| {
									dismiss
										.update(cx, |state, cx| state.set_open(false, false, cx));
								}),
							),
						),
				)
				.with_priority(4),
			)
	}
}

struct MetricsDetails {
	key: String,
	duration: Option<String>,
	input: Option<u64>,
	output: Option<u64>,
	calls: Option<u64>,
	tokens: Vec<(&'static str, String)>,
	activity: Vec<(&'static str, String)>,
}
impl MetricsDetails {
	fn new(metrics: &ResponseMetrics) -> Self {
		Self {
			key: metrics.key.clone(),
			duration: metrics.duration_ms.map(duration_label),
			input: metrics.usage.as_ref().map(|usage| usage.input_tokens),
			output: metrics.usage.as_ref().map(|usage| usage.output_tokens),
			calls: metrics
				.usage
				.as_ref()
				.and_then(|usage| usage.details.as_ref())
				.and_then(|details| details.responses)
				.filter(|count| *count > 0),
			tokens: metrics.usage.as_ref().map(usage_rows).unwrap_or_default(),
			activity: metrics.timing.as_ref().map(timing_rows).unwrap_or_default(),
		}
	}
}
#[derive(Default)]
struct MetricsPopover {
	open: bool,
	pinned: bool,
	trigger_hovered: bool,
	panel_hovered: bool,
	pending: Option<Task<()>>,
	anchor: Bounds<Pixels>,
}
impl MetricsPopover {
	fn set_open(&mut self, open: bool, pinned: bool, cx: &mut Context<Self>) {
		self.pending.take();
		self.open = open;
		self.pinned = pinned;
		cx.notify();
	}

	fn hover(&mut self, target: HoverTarget, inside: bool, cx: &mut Context<Self>) {
		match target {
			HoverTarget::Trigger => self.trigger_hovered = inside,
			HoverTarget::Panel => self.panel_hovered = inside,
		}
		let inside = self.trigger_hovered || self.panel_hovered;
		self.pending.take();
		if self.pinned || inside == self.open {
			return;
		}
		let delay = Duration::from_millis(if inside { 180 } else { 100 });
		self.pending = Some(cx.spawn(async move |popover, cx| {
			cx.background_executor().timer(delay).await;
			let _ = popover.update(cx, |state, cx| state.set_open(inside, false, cx));
		}));
	}
}

enum HoverTarget {
	Trigger,
	Panel,
}

pub(super) fn footer(
	entry: &AgentHistoryEntryDto,
	identity: &str,
	metrics: Option<AnyElement>,
) -> AnyElement {
	let text = markdown::response_text(&entry.text);
	let text = if entry.weather.is_empty() {
		text
	} else {
		format!(
			"{}\n\n{}",
			text,
			entry.weather.iter().map(|f| f.markdown()).collect::<Vec<_>>().join("\n\n")
		)
	};

	gpui::div()
		.mt(gpui::px(METADATA_GAP))
		.flex()
		.items_center()
		.gap(gpui::px(2.))
		.child(metrics.unwrap_or_else(|| agent_surface::reply_metrics(entry).into_any_element()))
		.when(entry.kind == "assistant", |row| {
			row.child(markdown::response_copy_button(
				&format!("copy-response-{identity}"),
				"Copy response",
				text,
			))
		})
		.into_any_element()
}

fn row(label: &'static str, value: impl Into<SharedString>) -> Div {
	gpui::div()
		.h(gpui::px(20.))
		.flex()
		.items_center()
		.justify_between()
		.gap(gpui::px(12.))
		.whitespace_nowrap()
		.child(gpui::div().text_color(ui_theme::rgb(ui_theme::foreground::BODY)).child(label))
		.child(
			gpui::div()
				.debug_selector(move || format!("turn-metric-value-{label}"))
				.flex_none()
				.font_features(ui_theme::tabular_figures())
				.text_color(ui_theme::rgb(TEXT))
				.child(value.into()),
		)
}

fn exact_tokens(value: u64) -> String {
	let digits = value.to_string();
	let mut label = String::with_capacity(digits.len() + digits.len() / 3 + 4);
	for (index, digit) in digits.chars().enumerate() {
		if index > 0 && (digits.len() - index).is_multiple_of(3) {
			label.push(',');
		}
		label.push(digit);
	}
	label.push_str(" tok");
	label
}

fn details_section(title: &'static str, rows: &[(&'static str, String)]) -> Div {
	gpui::div()
		.flex()
		.flex_col()
		.gap(gpui::px(4.))
		.child(
			gpui::div()
				.text_size(gpui::px(11.))
				.line_height(gpui::px(14.))
				.text_color(ui_theme::rgb(TEXT_MUTED))
				.child(title),
		)
		.child(gpui::div().flex().flex_col().children(rows.iter().map(|(k, v)| row(k, v.clone()))))
}

fn usage_rows(usage: &AgentTurnUsageDto) -> Vec<(&'static str, String)> {
	let Some(details) = usage.details.as_ref() else { return Vec::new() };
	let tokens = |value| format!("{} tok", agent_surface::compact_tokens(value));
	let part = |value, total: Option<u64>| {
		let value_label = tokens(value);
		match total.filter(|total| *total > 0) {
			Some(total) => format!("{value_label} · {:.0}%", value as f64 / total as f64 * 100.),
			None => value_label,
		}
	};
	[
		(
			"Cached input",
			details
				.cached_input
				.filter(|value| *value > 0)
				.map(|value| part(value, details.last_input)),
		),
		(
			"Reasoning output",
			details
				.reasoning_output
				.filter(|value| *value > 0)
				.map(|value| part(value, details.last_output)),
		),
	]
	.into_iter()
	.filter_map(|(label, value)| value.map(|value| (label, value)))
	.collect()
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

fn timing_rows(timing: &AgentTurnTimingDto) -> Vec<(&'static str, String)> {
	[
		("Output activity", timing.model_output_ms),
		("Tools", timing.tool_activity_ms),
		("Compaction", timing.compaction_ms),
		("Review", timing.auto_review_ms),
		("Waiting for you", timing.user_request_ms),
	]
	.into_iter()
	.filter_map(|(label, value)| {
		value
			.filter(|ms| *ms > 0)
			.map(|ms| (label, if ms < 1_000 { format!("{ms}ms") } else { duration_label(ms) }))
	})
	.collect()
}

fn duration_icon() -> impl IntoElement {
	gpui::svg()
		.data(br#"<svg xmlns="http://www.w3.org/2000/svg" width="11" height="11"><g fill="none" stroke="white" stroke-width="1"><circle cx="5.5" cy="5.5" r="4.25"/><path d="M5.5 2.5V5.5L7.5 6.5"/></g></svg>"#)
		.text_color(ui_theme::rgb(TEXT_MUTED))
		.size(gpui::px(11.))
		.flex_none()
}

fn details_panel(details: &MetricsDetails) -> Stateful<Div> {
	// Render one surface for both hover previews and immediate click inspection.
	gpui::div()
		.id(SharedString::from(format!("response-detail-panel-{}", details.key)))
		.debug_selector(|| "native-turn-usage".into())
		.role(Role::Group)
		.aria_label("Turn metrics details")
		.cursor_default()
		.occlude()
		.on_click(|_, _, cx| cx.stop_propagation())
		.w_full()
		.whitespace_normal()
		.p(gpui::px(12.))
		.text_color(ui_theme::rgb(TEXT))
		.text_size(gpui::px(12.))
		.line_height(gpui::px(18.))
		.flex()
		.flex_col()
		.gap(gpui::px(10.))
		.child(
			gpui::div()
				.flex()
				.flex_col()
				.child(row("Time", details.duration.clone().unwrap_or_else(|| "—".into())))
				.child(row("Input", details.input.map(exact_tokens).unwrap_or_else(|| "—".into())))
				.child(row(
					"Output",
					details.output.map(exact_tokens).unwrap_or_else(|| "—".into()),
				))
				.children(details.calls.map(|count| row("Model calls", count.to_string()))),
		)
		.when(!details.tokens.is_empty(), |panel| {
			panel.child(details_section("Last model call", &details.tokens))
		})
		.when(!details.activity.is_empty(), |panel| {
			panel.child(
				details_section("Activity · may overlap", &details.activity)
					.debug_selector(|| "observed-timing-details".into()),
			)
		})
}

#[cfg(test)]
mod tests {
	use super::{AgentTurnTimingDto, AgentTurnUsageDto, timing_rows, usage_rows};
	use gpui::AppContext as _;

	#[gpui::test]
	fn entering_details_keeps_the_preview_open_in_either_hover_callback_order(
		cx: &mut gpui::TestAppContext,
	) {
		for panel_first in [true, false] {
			let popover = cx.new(|_| super::MetricsPopover::default());
			popover.update(cx, |state, cx| {
				state.set_open(true, false, cx);
				state.hover(super::HoverTarget::Trigger, true, cx);
				if panel_first {
					state.hover(super::HoverTarget::Panel, true, cx);
					state.hover(super::HoverTarget::Trigger, false, cx);
				} else {
					state.hover(super::HoverTarget::Trigger, false, cx);
					state.hover(super::HoverTarget::Panel, true, cx);
				}
			});
			cx.run_until_parked();
			cx.executor().advance_clock(std::time::Duration::from_millis(300));
			cx.run_until_parked();
			popover.read_with(cx, |state, _| assert!(state.open));
		}
	}

	#[test]
	fn details_omit_unobserved_and_zero_duration_activity() {
		let rows = timing_rows(&AgentTurnTimingDto {
			model_output_ms: Some(1_500),
			tool_activity_ms: Some(0),
			auto_review_ms: Some(42),
			..Default::default()
		});
		assert_eq!(rows, vec![("Output activity", "1.5s".into()), ("Review", "42ms".into())]);
		assert!(timing_rows(&Default::default()).is_empty());
	}

	#[test]
	fn cache_and_reasoning_shares_use_the_last_call_instead_of_the_turn_total() {
		let mut usage = AgentTurnUsageDto {
			input_tokens: 800,
			output_tokens: 400,
			details: Some(decodex_protocol::AgentUsageDetailsDto {
				last_input: Some(100),
				cached_input: Some(80),
				last_output: Some(40),
				reasoning_output: Some(10),
				thread_total: Some(99_000),
				context_capacity: Some(1_000_000),
				..Default::default()
			}),
		};
		assert_eq!(
			usage_rows(&usage),
			vec![
				("Cached input", "80 tok · 80%".into()),
				("Reasoning output", "10 tok · 25%".into()),
			]
		);
		usage.input_tokens = 100;
		usage.output_tokens = 40;
		let details = usage.details.as_mut().unwrap();
		details.cached_input = Some(0);
		details.reasoning_output = Some(0);
		assert!(
			usage_rows(&usage).is_empty(),
			"thread totals and context limits do not describe one turn"
		);
	}
}
