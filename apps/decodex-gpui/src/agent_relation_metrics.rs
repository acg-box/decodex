//! Per-agent usage, time, rate and the share of recorded conversation tokens.
use super::{AgentSurface, AnyElement, Context, Node};
use crate::ui_theme::foreground;
use gpui::{App, IntoElement, RenderOnce, Window, prelude::*};
pub(in super::super) type Metrics = crate::ui_work_metrics::Sample;
const BAR_WIDTH: f32 = 68.;

/// One comparison range per graph, derived only from source-bound samples.
/// Cumulative counts and observed rates have no common capacity or task target.
#[derive(Clone, Copy, Default)]
pub(super) struct Scale([f64; 3]);
impl Scale {
	pub(super) fn new(samples: impl IntoIterator<Item = Metrics>) -> Self {
		let mut maxima = [0_f64; 3];
		for sample in samples {
			for (maximum, value) in maxima.iter_mut().zip(values(sample)) {
				if let Some(value) = value {
					*maximum = maximum.max(value);
				}
			}
		}
		Self(maxima)
	}

	fn fills(self, sample: Metrics) -> [f32; 3] {
		let values = values(sample);
		std::array::from_fn(|index| match values[index] {
			Some(value) if self.0[index] > 0. => (value / self.0[index]).clamp(0., 1.) as f32,
			_ => 0.,
		})
	}
}

fn values(sample: Metrics) -> [Option<f64>; 3] {
	[
		sample.tokens.map(|value| value as f64),
		sample.time().map(|value| value as f64),
		sample.rate().map(|(value, _)| value),
	]
}

// Graph labels have bounded magnitudes. Full native counters and time remain in
// the accessible description; the graph never grows when a counter gains a digit.
fn compact(mut value: f64) -> String {
	let units = ["", "K", "M", "B", "T", "P", "E", "Z"];
	let mut unit = 0;
	while value >= 999.95 && unit + 1 < units.len() {
		value /= 1000.;
		unit += 1;
	}
	format!("{value:.1}{}", units[unit])
}

fn time_label(ms: Option<u64>) -> String {
	match ms {
		Some(ms) if ms / 3_600_000 >= 1000 => {
			let days = ms / 86_400_000;
			if days < 1000 {
				format!("{days}d {}h", ms / 3_600_000 % 24)
			} else {
				format!("{}y", compact(days as f64 / 365.25))
			}
		},
		_ => crate::ui_work_metrics::time_label(ms),
	}
}

fn labels(metrics: Metrics) -> [String; 3] {
	let tokens = metrics.tokens.map_or_else(
		|| "—".into(),
		|value| if value < 1000 { value.to_string() } else { compact(value as f64) },
	);
	let rate = metrics.rate().map_or_else(|| "—".into(), |(value, _)| compact(value));
	[tokens, time_label(metrics.time()), format!("{rate}/s")]
}

fn fraction(value: Option<u64>, total: u128) -> Option<f32> {
	value.filter(|_| total > 0).map(|value| (value as f64 / total as f64).clamp(0., 1.) as f32)
}

fn percentage(value: Option<f32>) -> String {
	match value {
		Some(value) if value > 0. && value < 0.01 => "<1%".into(),
		Some(value) => format!("{:.0}%", value * 100.),
		None => "—".into(),
	}
}

fn value_text(text: String, width: f32, layout: super::card::Layout) -> gpui::Div {
	gpui::div()
		.absolute()
		.left_0()
		.top_0()
		.w(gpui::px(width))
		.h(gpui::px(layout.metric_line()))
		.flex()
		.items_center()
		.justify_center()
		.whitespace_nowrap()
		.text_size(gpui::px(layout.metric_size()))
		.line_height(gpui::px(layout.metric_line()))
		.font_weight(gpui::FontWeight::MEDIUM)
		.child(text)
}

fn readout_value(
	text: String,
	accent: u32,
	amount: f32,
	width: f32,
	fill_selector: String,
	layout: super::card::Layout,
) -> gpui::Div {
	let inset = layout.pixel(0.5).max(0.5);
	let radius = layout.spacing(2.).max(1.);
	gpui::div()
		.relative()
		.w_full()
		.h(gpui::px(layout.metric_line()))
		.flex_none()
		.rounded(gpui::px(radius))
		.bg(crate::ui_theme::rgb(crate::ui_theme::LINE_STRONG))
		.overflow_hidden()
		.child(
			value_text(text.clone(), width, layout)
				.text_color(crate::ui_theme::rgb(foreground::PRIMARY)),
		)
		.child(
			gpui::div()
				.debug_selector(move || fill_selector.clone())
				.absolute()
				.left(gpui::px(inset))
				.top(gpui::px(inset))
				.bottom(gpui::px(inset))
				.w(gpui::px(layout.pixel((width - 2. * inset) * amount)))
				.rounded(gpui::px((radius - inset).max(0.5)))
				.bg(crate::ui_theme::rgb(accent))
				.overflow_hidden()
				// The same label uses inverse ink only over the colored fill.
				// Both copies share one pixel grid and one text origin.
				.child(
					value_text(text, width, layout)
						.left(gpui::px(-inset))
						.top(gpui::px(-inset))
						.text_color(crate::ui_theme::rgb(crate::ui_theme::CANVAS)),
				),
		)
}

#[derive(IntoElement)]
struct Readout {
	key: String,
	metrics: Metrics,
	scale: Scale,
	layout: super::card::Layout,
}

impl RenderOnce for Readout {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let layout = self.layout.on_display(window.scale_factor());
		let [tokens, time, rate] = labels(self.metrics);
		let targets = self.scale.fills(self.metrics);
		let fills: [f32; 3] = std::array::from_fn(|index| {
			crate::ui_motion::value(
				gpui::SharedString::from(format!("graph-stat-fill-{}-{index}", self.key)),
				targets[index],
				window,
				cx,
			)
		});
		let measure = |text: &str| {
			let mut font = window.text_style().font();
			font.weight = gpui::FontWeight::MEDIUM;
			f32::from(
				window
					.text_system()
					.shape_line(
						text.to_owned().into(),
						gpui::px(layout.metric_size()),
						&[gpui::TextRun {
							len: text.len(),
							font: font.clone(),
							..Default::default()
						}],
						None,
					)
					.width,
			)
			.ceil()
		};
		let values = [measure(&tokens), measure(&time), measure(&rate)];
		let width = (values.into_iter().fold(0., f32::max) + 2. * layout.spacing(4.))
			.max(layout.spacing(BAR_WIDTH));
		let token_selector = format!("relation-tokens-{}", self.key);
		let time_selector = format!("relation-time-{}", self.key);
		let rate_selector = format!("relation-rate-{}", self.key);
		let fill_selectors =
			["tokens", "time", "rate"].map(|kind| format!("relation-fill-{}-{kind}", self.key));
		gpui::div()
			.w(gpui::px(width))
			.h(gpui::px(layout.metrics_height()))
			.flex_none()
			.flex()
			.flex_col()
			.whitespace_nowrap()
			.gap(gpui::px(layout.spacing(1.)))
			.text_size(gpui::px(layout.detail_size()))
			.line_height(gpui::px(layout.metric_line()))
			.font_weight(gpui::FontWeight::NORMAL)
			.text_color(crate::ui_theme::rgb(foreground::BODY))
			.child(
				gpui::div()
					.debug_selector(move || token_selector.clone())
					.w_full()
					.flex_none()
					.child(readout_value(
						tokens,
						crate::ui_theme::BLUE,
						fills[0],
						width,
						fill_selectors[0].clone(),
						layout,
					)),
			)
			.child(
				gpui::div()
					.debug_selector(move || time_selector.clone())
					.w_full()
					.flex_none()
					.child(readout_value(
						time,
						crate::ui_theme::AMBER,
						fills[1],
						width,
						fill_selectors[1].clone(),
						layout,
					)),
			)
			.child(
				gpui::div()
					.debug_selector(move || rate_selector.clone())
					.w_full()
					.flex_none()
					.child(readout_value(
						rate,
						crate::ui_theme::GREEN,
						fills[2],
						width,
						fill_selectors[2].clone(),
						layout,
					)),
			)
	}
}

impl AgentSurface {
	pub(super) fn metrics_for(&self, node: &Node) -> Metrics {
		node.row
			.as_ref()
			.and_then(|row| {
				self.work_board
					.briefs
					.get(&row.key)
					.filter(|brief| brief.source_thread == row.thread)
			})
			.map(|brief| brief.metrics)
			.unwrap_or_default()
	}

	pub(super) fn node_metrics(
		&self,
		node: &Node,
		layout: super::card::Layout,
		scale: Scale,
	) -> AnyElement {
		let metrics = self.metrics_for(node);
		let share = fraction(metrics.tokens, self.recorded_token_total());
		let selector = format!("relation-metrics-{}", node.key);
		let description = match share {
			Some(share) => format!(
				"{} {} of all recorded tokens; usage allocation.",
				metrics.label(),
				percentage(Some(share))
			),
			None => metrics.label(),
		};
		gpui::div()
			.id(gpui::SharedString::from(selector.clone()))
			.debug_selector(move || selector.clone())
			.role(gpui::Role::Group)
			.aria_label(format!("{description} Bar fills compare recorded values with the largest agent in this graph; they do not show completion."))
			.flex_none()
			.child(Readout { key: node.key.clone(), metrics, scale, layout })
			.into_any_element()
	}

	fn recorded_token_total(&self) -> u128 {
		self.work_board
			.usage
			.iter()
			.flat_map(|usage| usage.values())
			.filter_map(|(_, sample)| sample.tokens)
			.map(u128::from)
			.sum()
	}

	pub(in super::super) fn open_graph_agent(&mut self, row: &super::Row, cx: &mut Context<Self>) {
		self.work_board.view.edge = None;
		self.work_board.focus = None;
		self.handoffs.focus = None;
		// Reveal this agent after the smaller canvas has its real bounds, keeping
		// its zoom and tree geometry. Ordinary navigation keeps its camera.
		self.work_board.view.reveal_after_resize |= self.workspace.graph_expanded;
		self.work_board.view.resize_anchor = None;
		self.work_board.view.hover = Default::default();
		self.workspace.graph_expanded = false;
		if row.native {
			if let Some(thread) = &row.thread {
				self.open_native_agent(&row.work, thread, cx);
			}
		} else {
			self.open_page(&row.work, cx);
		}
		self.work_board.view.picked_agent = Some(row.key.clone());
		cx.notify();
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn bar_ranges_compare_each_recorded_metric_and_keep_missing_values_empty() {
		let sample = |tokens, ms, output| Metrics {
			tokens: Some(tokens),
			stats: Some(decodex_protocol::AgentThreadMetrics {
				completed_ms: Some(ms),
				rate: Some(decodex_protocol::AgentOutputRate {
					estimated: true,
					active: false,
					output_tokens: output,
					duration_ms: 1_000,
					started_at_ms: 0,
					completed_at_ms: Some(1_000),
				}),
				..Default::default()
			}),
			..Default::default()
		};
		let first = sample(1_000, 10_000, 100);
		let second = sample(500, 20_000, 20);
		let zero = sample(0, 0, 0);
		let missing = Metrics::default();
		let scale = Scale::new([first, second, zero, missing]);
		assert_eq!(scale.fills(first), [1., 0.5, 1.]);
		assert_eq!(scale.fills(second), [0.5, 1., 0.2]);
		assert_eq!(scale.fills(zero), [0.; 3]);
		assert_eq!(scale.fills(missing), [0.; 3]);
		assert_eq!(Scale::new([zero, missing]).fills(zero), [0.; 3]);
	}

	struct Fixture {
		zoom: f32,
		metrics: Metrics,
	}
	impl gpui::Render for Fixture {
		fn render(&mut self, window: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
			let layout =
				super::super::card::Layout::new(self.zoom).on_display(window.scale_factor());
			gpui::div()
				.w(gpui::px(layout.width))
				.px(gpui::px(layout.spacing(super::super::card::INSET_X)))
				.font_family(crate::ui_theme::FONT_FAMILY)
				.child(Readout {
					key: "fixture".into(),
					metrics: self.metrics,
					scale: Scale([1_000_000., 120_000., 100.]),
					layout,
				})
		}
	}

	#[gpui::test]
	fn metric_values_stay_in_one_narrow_three_row_block_across_readable_zoom(
		cx: &mut gpui::TestAppContext,
	) {
		let (view, visual) = cx.add_window_view(|_, _| Fixture {
			zoom: 1.,
			metrics: Metrics {
				tokens: Some(514_759),
				stats: Some(decodex_protocol::AgentThreadMetrics {
					completed_ms: Some(76_000),
					rate: Some(decodex_protocol::AgentOutputRate {
						estimated: true,
						active: false,
						output_tokens: 731,
						duration_ms: 10_000,
						started_at_ms: 0,
						completed_at_ms: Some(10_000),
					}),
					..Default::default()
				}),
				..Default::default()
			},
		});
		let handle = visual.update(|window, _| window.window_handle());
		for density in [1., 2.] {
			visual.simulate_window_scale_factor_change(handle, density);
			for zoom in [1., 0.7, 0.69, 0.6, 0.55, 0.5, 0.45] {
				view.update(visual, |s, cx| {
					s.zoom = zoom;
					cx.notify();
				});
				visual.update(crate::ui_frame::draw_frame);
				let tokens = visual.debug_bounds("relation-tokens-fixture").unwrap();
				let time = visual.debug_bounds("relation-time-fixture").unwrap();
				let layout = super::super::card::Layout::new(zoom).on_display(density);
				let rate = visual.debug_bounds("relation-rate-fixture").unwrap();
				for (upper, lower) in [(tokens, time), (time, rate)] {
					assert_eq!(upper.left(), lower.left());
					assert_eq!(upper.size.width, lower.size.width);
					assert!(lower.top() >= upper.bottom());
					assert!(f32::from(lower.top() - upper.bottom()) <= layout.spacing(1.) + 0.5);
				}
				assert!(f32::from(tokens.size.width) <= BAR_WIDTH);
				assert!(f32::from(rate.bottom() - tokens.top()) <= layout.metrics_height() + 0.5);
				let inset = layout.pixel(0.5).max(0.5);
				for (name, track, fraction) in [
					("relation-fill-fixture-tokens", tokens, 0.514759),
					("relation-fill-fixture-time", time, 76_000. / 120_000.),
					("relation-fill-fixture-rate", rate, 0.731),
				] {
					let fill = visual.debug_bounds(name).unwrap();
					assert_eq!(fill.left(), track.left() + gpui::px(inset));
					assert_eq!(fill.top(), track.top() + gpui::px(inset));
					assert!(fill.bottom() < track.bottom());
					assert!(fill.right() < track.right());
					let inner_width = f32::from(track.size.width) - 2. * inset;
					assert!(
						(f32::from(fill.size.width) - inner_width * fraction).abs() <= 1. / density,
						"{name} fill lost its proportion at {zoom} on {density}x: {fill:?}"
					);
				}
				for name in
					["relation-tokens-fixture", "relation-time-fixture", "relation-rate-fixture"]
				{
					if let Some(bounds) = visual.debug_bounds(name) {
						assert!(
							f32::from(bounds.right())
								<= layout.width - layout.spacing(super::super::card::INSET_X) + 0.5,
							"{name} overflows at {zoom} on {density}x: {bounds:?}"
						);
					}
				}
			}
		}
	}
	#[test]
	fn token_share_retains_unknown_zero_and_small_values() {
		assert_eq!(fraction(None, 100), None);
		assert_eq!(fraction(Some(0), 0), None);
		assert_eq!(percentage(fraction(Some(0), 100)), "0%");
		assert_eq!(percentage(fraction(Some(25), 100)), "25%");
		assert_eq!(percentage(fraction(Some(1), 1000)), "<1%");
	}

	fn turn(
		id: &str,
		duration: Option<u64>,
		tokens: Option<u64>,
	) -> decodex_protocol::AgentTimelineEntry {
		decodex_protocol::AgentTimelineEntry {
			position: 0,
			content: decodex_protocol::AgentTimelineContent::TurnBoundary {
				turn_id: id.into(),
				completed: true,
				status: None,
				duration_ms: duration,
				usage_summary: None,
				usage: tokens.map(|input_tokens| decodex_protocol::AgentTurnUsageDto {
					input_tokens,
					output_tokens: 10,
					details: None,
				}),
				error: None,
			},
		}
	}
	#[test]
	fn page_durations_and_delta_tokens_are_not_cumulative_metrics() {
		let result = Metrics::from_entries(&[
			turn("old", Some(100), Some(500)),
			turn("new", Some(200), None),
		]);
		assert_eq!(result.stats, None);
		assert_eq!(result.tokens, None);
		let result = Metrics::from_entries(&[turn("new", Some(200), Some(500))]);
		assert_eq!(result.tokens, None);
	}
	#[test]
	fn cumulative_tokens_use_latest_native_counter_without_summing_snapshots() {
		let mut first = turn("first", Some(100), Some(50));
		let mut second = turn("second", Some(200), Some(20));
		for (entry, total) in [(&mut first, 1000), (&mut second, 1250)] {
			if let decodex_protocol::AgentTimelineContent::TurnBoundary {
				usage: Some(usage), ..
			} = &mut entry.content
			{
				usage.details = Some(decodex_protocol::AgentUsageDetailsDto {
					thread_total: Some(total),
					..Default::default()
				});
			}
		}
		let latest = turn("third", None, Some(500));
		let result = Metrics::from_entries(&[first, second, latest]);
		assert_eq!(result.tokens, Some(1250));
		assert_eq!(result.stats, None);
		assert_eq!(Metrics::from_entries(&[turn("only-delta", Some(200), Some(500))]).tokens, None);
	}
}
