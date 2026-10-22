//! Per-agent usage, time, rate and the share of recorded conversation tokens.
use super::{AgentSurface, AnyElement, Context, Node};
use crate::ui_theme::foreground;
use gpui::{App, IntoElement, RenderOnce, Window, prelude::*};
pub(in super::super) type Metrics = crate::ui_work_metrics::Sample;

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
	[
		format!("{tokens} tok"),
		time_label(metrics.time()),
		format!(
			"{rate} {}",
			if metrics.stats.is_some() { metrics.rate_caption() } else { "tok/s" }
		),
	]
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

fn parts(text: &str, with_unit: bool) -> (&str, Option<&str>) {
	match (with_unit, text.split_once(' ')) {
		(true, Some((value, unit))) => (value, Some(unit)),
		_ => (text, None),
	}
}

fn readout_value(text: String, with_unit: bool, layout: super::card::Layout) -> gpui::Div {
	let (value, unit) = parts(&text, with_unit);
	gpui::div()
		.flex()
		.items_baseline()
		.whitespace_nowrap()
		.gap(gpui::px(layout.spacing(3.)))
		.text_size(gpui::px(layout.metric_size()))
		.line_height(gpui::px(layout.detail_line()))
		.font_weight(gpui::FontWeight::MEDIUM)
		.text_color(gpui::rgb(foreground::BODY))
		.child(value.to_owned())
		.when_some(unit, |row, unit| {
			row.child(
				gpui::div()
					.text_size(gpui::px(layout.detail_size()))
					.font_weight(gpui::FontWeight::NORMAL)
					.text_color(gpui::rgb(foreground::SECONDARY))
					.child(unit.to_owned()),
			)
		})
}

#[derive(IntoElement)]
struct Readout {
	key: String,
	metrics: Metrics,
	share: Option<f32>,
	layout: super::card::Layout,
}

impl RenderOnce for Readout {
	fn render(self, window: &mut Window, _: &mut App) -> impl IntoElement {
		let layout = self.layout.on_display(window.scale_factor());
		let [tokens, time, rate] = labels(self.metrics);
		let share = self.share.map(|value| format!("· {} of tokens", percentage(Some(value))));
		let measure_text = |text: &str, size: f32, weight: gpui::FontWeight| {
			let mut font = window.text_style().font();
			font.weight = weight;
			f32::from(
				window
					.text_system()
					.shape_line(
						text.to_owned().into(),
						gpui::px(size),
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
		let measure = |text: &str, with_unit: bool| {
			let (value, unit) = parts(text, with_unit);
			measure_text(value, layout.metric_size(), gpui::FontWeight::MEDIUM)
				+ unit.map_or(0., |unit| {
					layout.spacing(3.)
						+ measure_text(unit, layout.detail_size(), gpui::FontWeight::NORMAL)
				})
		};
		let available = layout.width - 2. * layout.spacing(super::card::INSET_X);
		let gap = layout.spacing(8.);
		let values = [measure(&tokens, true), measure(&time, false), measure(&rate, true)];
		// Keep real values until their shaped glyphs no longer fit. Allocation is
		// supplementary: it yields before duration or throughput at smaller sizes.
		let show_time = values[0] + gap + values[1] <= available;
		let all_values = values.iter().sum::<f32>() + 2. * gap;
		let show_rate = all_values <= available;
		let show_share = show_rate
			&& share.as_ref().is_some_and(|share| {
				all_values
					+ gap
					+ measure_text(share, layout.detail_size(), gpui::FontWeight::NORMAL)
					<= available
			});
		let token_selector = format!("relation-tokens-{}", self.key);
		let time_selector = format!("relation-time-{}", self.key);
		let rate_selector = format!("relation-rate-{}", self.key);
		let share_selector = format!("relation-token-share-{}", self.key);
		gpui::div()
			.w_full()
			.h(gpui::px(layout.detail_line()))
			.flex_none()
			.flex()
			.items_center()
			.whitespace_nowrap()
			.gap(gpui::px(gap))
			.text_size(gpui::px(layout.detail_size()))
			.line_height(gpui::px(layout.detail_line()))
			.font_weight(gpui::FontWeight::NORMAL)
			.text_color(gpui::rgb(foreground::BODY))
			.child(
				gpui::div()
					.flex_1()
					.min_w_0()
					.flex()
					.items_center()
					.gap(gpui::px(gap))
					.child(
						gpui::div()
							.debug_selector(move || token_selector.clone())
							.flex_none()
							.child(readout_value(tokens, true, layout)),
					)
					.when(show_share, |group| {
						group.child(
							gpui::div()
								.debug_selector(move || share_selector.clone())
								.flex_none()
								.text_color(gpui::rgb(super::card::ALLOCATION_TEXT))
								.child(share.unwrap_or_default()),
						)
					}),
			)
			.when(show_time, |row| {
				row.child(
					gpui::div()
						.debug_selector(move || time_selector.clone())
						.flex_none()
						.child(readout_value(time, false, layout)),
				)
			})
			.when(show_rate, |row| {
				row.child(
					gpui::div()
						.debug_selector(move || rate_selector.clone())
						.flex_none()
						.child(readout_value(rate, true, layout)),
				)
			})
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

	pub(super) fn node_metrics(&self, node: &Node, layout: super::card::Layout) -> AnyElement {
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
			.aria_label(description)
			.w_full()
			.flex_none()
			.child(Readout { key: node.key.clone(), metrics, share, layout })
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
					share: Some(0.3),
					layout,
				})
		}
	}

	#[gpui::test]
	fn metric_values_keep_one_baseline_and_use_available_space_below_seventy_percent(
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
				assert!(time.left() > tokens.right());
				assert_eq!(tokens.top(), time.top());
				if zoom >= 0.6 {
					let rate = visual
						.debug_bounds("relation-rate-fixture")
						.expect("normal token, time and rate values still fit at 60%");
					assert!(rate.left() > time.right());
					assert_eq!(rate.top(), tokens.top());
				}
				let layout = super::super::card::Layout::new(zoom).on_display(density);
				for name in [
					"relation-tokens-fixture",
					"relation-time-fixture",
					"relation-rate-fixture",
					"relation-token-share-fixture",
				] {
					if let Some(bounds) = visual.debug_bounds(name) {
						assert!(
							f32::from(bounds.right())
								<= layout.width - layout.spacing(super::super::card::INSET_X) + 0.5,
							"{name} overflows at {zoom} on {density}x: {bounds:?}"
						);
					}
				}
				if zoom == 1. {
					assert!(visual.debug_bounds("relation-token-share-fixture").is_some());
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
