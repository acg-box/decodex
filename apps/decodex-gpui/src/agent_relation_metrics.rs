//! Per-agent usage, time, rate and the share of recorded conversation tokens.
use gpui::{
	App, Div, FontWeight, HighlightStyle, IntoElement, RenderOnce, Role, SharedString, StyledText,
	TextRun, Window,
	prelude::{InteractiveElement as _, ParentElement, StatefulInteractiveElement as _, Styled},
};

use crate::{
	shell::agent_surface::work_board::relations::{
		AgentSurface, AnyElement, Context, Node, Row, card,
	},
	ui_motion,
	ui_theme::{
		self, AMBER, BLUE, FONT_FAMILY, GREEN,
		foreground::{PRIMARY, SECONDARY},
	},
	ui_work_metrics::{self, Sample},
};

const BAR_WIDTH: f32 = 64.;
// Fixed display rulers; never task completion, context limits, or budgets.
const REFERENCES: [f64; 3] = [2_000_000., 1_800_000., 100.];

#[derive(IntoElement)]
struct Readout {
	key: String,
	metrics: Sample,
	layout: crate::shell::agent_surface::work_board::relations::card::Layout,
}
impl RenderOnce for Readout {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		render_hud(
			self.metrics,
			&self.key,
			self.layout.on_display(window.scale_factor()),
			window,
			cx,
		)
	}
}

impl AgentSurface {
	pub(super) fn metrics_for(&self, node: &Node) -> Sample {
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
		layout: crate::shell::agent_surface::work_board::relations::card::Layout,
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
			.id(SharedString::from(selector.clone()))
			.debug_selector(move || selector.clone())
			.role(Role::Group)
			.aria_label(format!("{description} Fixed display rulers: 2 million tokens, 30 minutes of generation, and 100 tokens per second. Double end marks show values above the ruler. Empty tracks mean missing data or zero; the printed value distinguishes them. These are not limits or completion."))
			.flex_none()
			.child(Readout { key: node.key.clone(), metrics, layout })
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

	pub(in super::super) fn open_graph_agent(&mut self, row: &Row, cx: &mut Context<Self>) {
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
		self.work_board.view.picked_project = None;

		cx.notify();
	}
}

fn ruler(value: Option<f64>, reference: f64) -> Option<(f32, bool)> {
	value.map(|value| ((value / reference).clamp(0., 1.) as f32, value > reference))
}

fn values(sample: Sample) -> [Option<f64>; 3] {
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
		value /= 1_000.;
		unit += 1;
	}

	format!("{value:.1}{}", units[unit])
}

fn time_label(ms: Option<u64>) -> String {
	match ms {
		Some(ms) if ms / 3_600_000 >= 1_000 => {
			let days = ms / 86_400_000;

			if days < 1_000 {
				format!("{days}d {}h", ms / 3_600_000 % 24)
			} else {
				format!("{}y", compact(days as f64 / 365.25))
			}
		},
		_ => ui_work_metrics::time_label(ms),
	}
}

fn labels(metrics: Sample) -> [String; 3] {
	let tokens = metrics.tokens.map_or_else(
		|| "—".into(),
		|value| if value < 1_000 { value.to_string() } else { compact(value as f64) },
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

fn render_hud(
	sample: Sample,
	key: &str,
	layout: crate::shell::agent_surface::work_board::relations::card::Layout,
	window: &mut Window,
	cx: &mut App,
) -> Div {
	let labels = labels(sample);
	let values = values(sample);
	let colors = [BLUE, AMBER, GREEN];
	let widest = hud_label_width(&labels, layout, window);
	let width = layout.pixel(layout.spacing(BAR_WIDTH).max(54.).max(widest.ceil() + 6.));
	let inset = layout.pixel(0.5).max(0.5);
	let hud_selector = format!("relation-hud-{key}");
	let mut block = gpui::div()
		.debug_selector(move || hud_selector.clone())
		.w(gpui::px(width))
		.h(gpui::px(layout.metrics_height()))
		.flex_none()
		.flex()
		.flex_col()
		.rounded(gpui::px(2.5))
		.overflow_hidden();

	for index in 0..3 {
		let readout = ruler(values[index], REFERENCES[index]);
		let fill = ui_motion::value(
			SharedString::from(format!("graph-stat-fill-{key}-{index}")),
			readout.map_or(0., |(fraction, _)| fraction),
			window,
			cx,
		);
		let inner = width - 2. * inset;
		let amount = layout.pixel(inner * fill);
		let color = colors[index];
		let kind = ["tokens", "time", "rate"][index];
		let selector = format!("relation-{kind}-{key}");
		let fill_selector = format!("relation-fill-{key}-{kind}");
		let track_selector = format!("relation-track-{key}-{kind}");
		let mut row = gpui::div()
			.debug_selector(move || selector.clone())
			.relative()
			.w(gpui::px(width))
			.h(gpui::px(layout.metric_line()))
			.flex_none()
			.child(
				gpui::div()
					.debug_selector(move || track_selector.clone())
					.absolute()
					.top_0()
					.left_0()
					.right_0()
					.bottom(gpui::px(layout.gap()))
					.bg(ui_theme::rgb(color).opacity(0.06)),
			);

		if amount > 0. {
			row = row.child(
				gpui::div()
					.debug_selector(move || fill_selector.clone())
					.absolute()
					.left(gpui::px(inset))
					.top(gpui::px(inset))
					.bottom(gpui::px(inset + layout.gap()))
					.w(gpui::px(amount))
					.rounded(gpui::px(1.))
					.bg(ui_theme::rgb(color).opacity(0.26)),
			);
		}
		if readout.is_some_and(|(_, exceeded)| exceeded) {
			for right in [inset, inset * 4.] {
				row = row.child(
					gpui::div()
						.absolute()
						.right(gpui::px(right))
						.top(gpui::px(inset))
						.bottom(gpui::px(inset))
						.w(gpui::px(inset))
						.bg(ui_theme::rgb(color).opacity(0.85)),
				);
			}
		}

		let text = if readout.is_some() { labels[index].as_str() } else { "—" };
		let units = text.char_indices().filter_map(|(start, character)| {
			(character.is_ascii_alphabetic() || character == '/').then_some((
				start..start + character.len_utf8(),
				HighlightStyle {
					color: Some(ui_theme::rgb(SECONDARY).into()),
					..Default::default()
				},
			))
		});

		row = row.child(
			gpui::div()
				.absolute()
				.inset_0()
				.flex()
				.items_center()
				.justify_center()
				.text_size(gpui::px(layout.metric_size()))
				.line_height(gpui::px(layout.metric_line()))
				.font_weight(FontWeight::NORMAL)
				.font_features(card::number_features())
				.text_color(ui_theme::rgb(PRIMARY))
				.whitespace_nowrap()
				.child(StyledText::new(text.to_owned()).with_highlights(units)),
		);
		block = block.child(row);
	}

	block
}

fn hud_label_width(labels: &[String; 3], layout: super::card::Layout, window: &Window) -> f32 {
	let mut font = window.text_style().font();

	font.family = FONT_FAMILY.into();
	font.weight = FontWeight::NORMAL;
	font.features = super::card::number_features();

	labels
		.iter()
		.map(|text| {
			f32::from(
				window
					.text_system()
					.shape_line(
						text.clone().into(),
						gpui::px(layout.metric_size()),
						&[TextRun { len: text.len(), font: font.clone(), ..Default::default() }],
						None,
					)
					.width,
			)
		})
		.fold(0., f32::max)
}

#[cfg(test)]
mod tests {
	use crate::shell::agent_surface::work_board::relations::{
		card,
		metrics::{
			self, BAR_WIDTH, IntoElement, ParentElement, REFERENCES, Readout, Sample, Styled,
			Window,
		},
	};

	struct Fixture {
		zoom: f32,
		metrics: Sample,
	}
	impl gpui::Render for Fixture {
		fn render(&mut self, window: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
			let layout =
				super::super::card::Layout::new(self.zoom).on_display(window.scale_factor());

			gpui::div()
				.w(gpui::px(layout.width))
				.px(gpui::px(layout.spacing(super::super::card::INSET_X)))
				.font_family(crate::ui_theme::FONT_FAMILY)
				.child(Readout { key: "fixture".into(), metrics: self.metrics, layout })
		}
	}

	#[test]
	fn fixed_rulers_keep_unknown_zero_and_over_range_distinct() {
		assert_eq!(metrics::ruler(None, REFERENCES[0]), None);
		assert_eq!(metrics::ruler(Some(0.), REFERENCES[0]), Some((0., false)));
		assert_eq!(metrics::ruler(Some(1_000_000.), REFERENCES[0]), Some((0.5, false)));
		assert_eq!(metrics::ruler(Some(2_000_000.), REFERENCES[0]), Some((1., false)));
		assert_eq!(metrics::ruler(Some(3_000_000.), REFERENCES[0]), Some((1., true)));
	}

	#[gpui::test]
	fn metric_values_stay_in_one_narrow_three_row_block_across_readable_zoom(
		cx: &mut gpui::TestAppContext,
	) {
		let (view, visual) = cx.add_window_view(|_, _| hud_fixture());
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
					assert_eq!(lower.top(), upper.bottom(), "rows form one continuous block");
				}

				assert!(f32::from(tokens.size.width) <= BAR_WIDTH);
				assert_eq!(f32::from(rate.bottom() - tokens.top()), layout.metrics_height());
				assert!(layout.metric_size() >= 10., "compact tracks keep the readable font floor");

				visual.update(|window, _| {
					let mut font = window.text_style().font();

					font.family = crate::ui_theme::FONT_FAMILY.into();
					font.weight = gpui::FontWeight::NORMAL;
					font.features = card::number_features();

					let widths = ["000000", "111111", "888888", "999999"].map(|text| {
						window
							.text_system()
							.shape_line(
								text.into(),
								gpui::px(layout.metric_size()),
								&[gpui::TextRun {
									len: text.len(),
									font: font.clone(),
									..Default::default()
								}],
								None,
							)
							.width
					});

					assert!(
						widths.windows(2).all(|pair| pair[0] == pair[1]),
						"numeric figures have equal native advances"
					);
				});

				assert!(f32::from(tokens.size.height) <= layout.metric_size() + 1.);

				let inset = layout.pixel(0.5).max(0.5);

				for (selector, row) in [
					("relation-track-fixture-tokens", tokens),
					("relation-track-fixture-time", time),
					("relation-track-fixture-rate", rate),
				] {
					let track = visual.debug_bounds(selector).unwrap();

					assert_eq!(track.top(), row.top());
					assert_eq!(track.bottom(), row.bottom() - gpui::px(layout.gap()));
				}
				for (name, track, fraction) in [
					("relation-fill-fixture-tokens", tokens, 514_759. / 2_000_000.),
					("relation-fill-fixture-time", time, 76_000. / 1_800_000.),
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

	fn hud_fixture() -> Fixture {
		Fixture {
			zoom: 1.,
			metrics: Sample {
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
		}
	}

	#[test]
	fn token_share_retains_unknown_zero_and_small_values() {
		assert_eq!(metrics::fraction(None, 100), None);
		assert_eq!(metrics::fraction(Some(0), 0), None);
		assert_eq!(metrics::percentage(metrics::fraction(Some(0), 100)), "0%");
		assert_eq!(metrics::percentage(metrics::fraction(Some(25), 100)), "25%");
		assert_eq!(metrics::percentage(metrics::fraction(Some(1), 1_000)), "<1%");
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
				timing: None,
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
		let result = Sample::from_entries(&[
			turn("old", Some(100), Some(500)),
			turn("new", Some(200), None),
		]);

		assert_eq!(result.stats, None);
		assert_eq!(result.tokens, None);

		let result = Sample::from_entries(&[turn("new", Some(200), Some(500))]);

		assert_eq!(result.tokens, None);
	}

	#[test]
	fn cumulative_tokens_use_latest_native_counter_without_summing_snapshots() {
		let mut first = turn("first", Some(100), Some(50));
		let mut second = turn("second", Some(200), Some(20));

		for (entry, total) in [(&mut first, 1_000), (&mut second, 1_250)] {
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
		let result = Sample::from_entries(&[first, second, latest]);

		assert_eq!(result.tokens, Some(1_250));
		assert_eq!(result.stats, None);
		assert_eq!(Sample::from_entries(&[turn("only-delta", Some(200), Some(500))]).tokens, None);
	}
}
