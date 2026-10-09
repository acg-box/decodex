//! Compare recorded native thread totals; duration describes the latest completed turn.
use super::*;

const TOKENS: u32 = 0xa99aef;
const TIME: u32 = 0x91a5b9;

#[derive(Clone, Copy, Default)]
pub(in super::super) struct Metrics {
	tokens: Option<u64>,
	duration: Option<u64>,
}
impl Metrics {
	pub(in super::super) fn from_entries(entries: &[decodex_protocol::AgentTimelineEntry]) -> Self {
		let tokens = entries.iter().rev().find_map(|entry| match &entry.content {
			decodex_protocol::AgentTimelineContent::TurnBoundary { usage: Some(usage), .. } =>
				usage.details.as_ref().and_then(|details| details.thread_total),
			_ => None,
		});
		let duration = entries
			.iter()
			.rev()
			.find_map(|entry| match &entry.content {
				decodex_protocol::AgentTimelineContent::TurnBoundary {
					completed: true,
					duration_ms,
					..
				} => Some(*duration_ms),
				_ => None,
			})
			.flatten();
		Self { tokens, duration }
	}

	fn label(self) -> String {
		format!(
			"Recorded total: {} tokens\nLatest completed turn: {}",
			self.tokens.map(super::super::super::compact_tokens).unwrap_or_else(|| "—".into()),
			time_label(self.duration)
		)
	}
}
fn time_label(ms: Option<u64>) -> String {
	match ms {
		Some(ms) if ms >= 60_000 => format!("{}m {}s", ms / 60_000, ms / 1000 % 60),
		Some(ms) => format!("{:.1}s", ms as f64 / 1000.),
		None => "—".into(),
	}
}
fn fraction(value: Option<u64>, baseline: u128) -> Option<f32> {
	value.filter(|_| baseline > 0).map(|v| (v as f64 / baseline as f64).clamp(0., 1.) as f32)
}
fn percentage(value: Option<f32>) -> String {
	match value {
		Some(v) if v > 0. && v < 0.01 => "<1%".into(),
		Some(v) => format!("{:.0}%", v * 100.),
		None => "—".into(),
	}
}
fn metric_track(value: Option<f32>, color: u32, scale: f32) -> AnyElement {
	gpui::div()
		.w_full()
		.flex()
		.gap(gpui::px(1.5 * scale))
		.children((0..12).map(|i| {
			let filled = (value.unwrap_or(0.) * 12. - i as f32).clamp(0., 1.);
			gpui::div()
				.flex_1()
				.h(gpui::px(3. * scale))
				.rounded(gpui::px(scale))
				.overflow_hidden()
				.bg(gpui::rgba(0xffffff10))
				.child(
					gpui::div()
						.h_full()
						.w(gpui::relative(filled))
						.bg(gpui::rgba((color << 8) | 0xc8)),
				)
		}))
		.into_any_element()
}

impl AgentSurface {
	fn metrics_for(&self, node: &Node) -> Metrics {
		node.row
			.as_ref()
			.and_then(|r| {
				self.work_board.briefs.get(&r.key).filter(|b| b.stamp == self.brief_stamp(r))
			})
			.map(|b| b.metrics)
			.unwrap_or_default()
	}

	fn metric_baselines(&self, graph: &Graph) -> (u128, u128) {
		graph.nodes.iter().map(|n| self.metrics_for(n)).fold((0, 0), |(tokens, time), m| {
			(
				tokens + u128::from(m.tokens.unwrap_or(0)),
				time.max(u128::from(m.duration.unwrap_or(0))),
			)
		})
	}

	pub(super) fn node_metrics(&self, node: &Node, graph: &Graph, scale: f32) -> AnyElement {
		let metrics = self.metrics_for(node);
		let baselines = self.metric_baselines(graph);
		let mut body = gpui::div().w_full().flex().gap(gpui::px(12. * scale));
		for (value, baseline, color, label, meaning) in [
			(
				metrics.tokens,
				baselines.0,
				TOKENS,
				metrics
					.tokens
					.map(super::super::super::compact_tokens)
					.map(|n| format!("{n} tok"))
					.unwrap_or_else(|| "— tok".into()),
				"Share of recorded conversation tokens",
			),
			(
				metrics.duration,
				baselines.1,
				TIME,
				time_label(metrics.duration),
				"Latest completed turn, relative to the longest reported turn",
			),
		] {
			let part = fraction(value, baseline);
			let tip = format!(
				"{meaning}: {}\nToken counters are cumulative native conversation totals. Duration is the latest completed turn. Missing values are excluded.\nThese bars compare usage, not task completion or a budget.",
				percentage(part)
			);
			body = body.child(
				gpui::div()
					.id(SharedString::from(format!("metric-{color}-{}", node.key)))
					.flex_1()
					.min_w_0()
					.flex()
					.flex_col()
					.gap(gpui::px(5. * scale))
					.tooltip(move |_, cx| cx.new(|_| RelationTip(tip.clone())).into())
					.child(
						gpui::div()
							.flex()
							.items_center()
							.justify_between()
							.gap(gpui::px(4. * scale))
							.text_size(gpui::px(11. * scale))
							.line_height(gpui::px(14. * scale))
							.child(gpui::div().text_color(gpui::rgb(TEXT_MUTED)).child(label))
							.child(
								gpui::div()
									.text_size(gpui::px(10. * scale))
									.text_color(gpui::rgb(color))
									.child(percentage(part)),
							),
					)
					.child(metric_track(part, color, scale)),
			);
		}
		body.into_any_element()
	}

	pub(in super::super) fn relation_metrics(
		&self,
		graph: &Graph,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let nodes: Vec<_> = graph.nodes.iter().filter(|n| n.row.is_some()).collect();
		let mut ranked: Vec<_> =
			nodes.iter().filter_map(|n| self.metrics_for(n).tokens.map(|v| (*n, v))).collect();
		ranked.sort_by_key(|(_, value)| std::cmp::Reverse(*value));
		let total: u128 = ranked.iter().map(|(_, v)| u128::from(*v)).sum();
		let mut distribution =
			gpui::div().flex().w_full().h(gpui::px(28.)).rounded(gpui::px(4.)).overflow_hidden();
		for (node, value) in ranked.iter().filter(|(_, v)| *v > 0) {
			let share = fraction(Some(*value), total).unwrap_or(0.);
			let row = node.row.clone().expect("agent node");
			let keyrow = row.clone();
			let tip = format!(
				"{}\n{}\n{} of recorded conversation totals. Includes cached input. Missing totals are excluded.",
				node.title,
				self.metrics_for(node).label(),
				percentage(Some(share))
			);
			distribution = distribution.child(
				gpui::div()
					.id(SharedString::from(format!("usage-share-{}", node.key)))
					.role(Role::Button)
					.aria_label(tip.clone())
					.tab_index(0)
					.w(gpui::relative(share))
					.h_full()
					.flex_none()
					.min_w_0()
					.overflow_hidden()
					.flex()
					.items_center()
					.relative()
					.cursor_pointer()
					.hover(|d| d.bg(gpui::rgba(0xffffff08)))
					.tooltip(move |_, cx| cx.new(|_| RelationTip(tip.clone())).into())
					.child(
						gpui::div()
							.w_full()
							.min_w_0()
							.flex()
							.items_center()
							.gap_1()
							.px_2()
							.pb_1()
							.text_size(gpui::px(11.))
							.when(share >= 0.12, |d| {
								d.child(
									gpui::div()
										.flex_1()
										.min_w_0()
										.text_ellipsis()
										.whitespace_nowrap()
										.child(node.title.clone()),
								)
							})
							.when(share >= 0.04, |d| {
								d.child(
									gpui::div()
										.flex_none()
										.whitespace_nowrap()
										.child(percentage(Some(share))),
								)
							}),
					)
					.child(
						gpui::div()
							.absolute()
							.bottom_0()
							.left(gpui::px(2.))
							.right(gpui::px(2.))
							.h(gpui::px(2.))
							.rounded_full()
							.bg(gpui::rgba((TOKENS << 8) | 0x90)),
					)
					.on_click(cx.listener(move |s, _, _, cx| s.inspect_station(&row, cx)))
					.on_key_down(cx.listener(move |s, e: &gpui::KeyDownEvent, _, cx| {
						if ["enter", "space"].contains(&e.keystroke.key.as_str()) {
							s.inspect_station(&keyrow, cx);
							cx.stop_propagation();
						}
					})),
			);
		}
		let amount = if ranked.is_empty() {
			"—".into()
		} else {
			match u64::try_from(total) {
				Ok(value) => super::super::super::compact_tokens(value),
				Err(_) => format!("{:.1}B", total as f64 / 1_000_000_000.),
			}
		};
		let tip = "Recorded cumulative conversation tokens, including cached input. Missing totals are excluded. The segments show each agent's share, not task progress.";
		let summary = gpui::div()
			.id("graph-usage-total")
			.flex_none()
			.w(gpui::px(138.))
			.flex()
			.flex_col()
			.gap(gpui::px(3.))
			.tooltip(move |_, cx| cx.new(|_| RelationTip(tip.into())).into())
			.child(
				gpui::div()
					.flex()
					.items_baseline()
					.gap(gpui::px(5.))
					.child(
						gpui::div()
							.text_size(gpui::px(17.))
							.line_height(gpui::px(20.))
							.font_weight(gpui::FontWeight::MEDIUM)
							.child(amount),
					)
					.child(
						gpui::div()
							.text_size(gpui::px(11.))
							.text_color(gpui::rgb(TEXT_MUTED))
							.child("tokens"),
					),
			)
			.child(
				gpui::div()
					.text_size(gpui::px(10.))
					.line_height(gpui::px(14.))
					.text_color(gpui::rgb(TEXT_MUTED))
					.child(format!("{} agents · {} links", graph.agent_count(), graph.edges.len())),
			);
		let mut body = gpui::div()
			.flex_none()
			.w_full()
			.min_w_0()
			.px(gpui::px(12.))
			.py(gpui::px(8.))
			.border_t_1()
			.border_color(gpui::rgba(0xffffff0c))
			.child(
				gpui::div()
					.flex()
					.items_center()
					.gap(gpui::px(16.))
					.child(summary)
					.child(gpui::div().flex_1().min_w_0().child(distribution)),
			);

		if !self.command_connection_ready() {
			body = body.child(
				gpui::div()
					.text_size(gpui::px(11.))
					.text_color(gpui::rgb(AMBER))
					.child("Disconnected · last known state"),
			);
		}
		body.into_any_element()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn percentages_preserve_missing_zero_and_small_reported_values() {
		assert_eq!(fraction(None, 100), None);
		assert_eq!(fraction(Some(0), 0), None);
		assert_eq!(fraction(Some(0), 100), Some(0.));
		assert_eq!(percentage(fraction(Some(25), 100)), "25%");
		assert_eq!(percentage(fraction(Some(1), 1000)), "<1%");
		assert_eq!(fraction(Some(u64::MAX), u128::from(u64::MAX) * 2), Some(0.5));
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
	fn latest_turn_metrics_do_not_mix_older_usage_with_new_duration() {
		let result = Metrics::from_entries(&[
			turn("old", Some(100), Some(500)),
			turn("new", Some(200), None),
		]);
		assert_eq!(result.duration, Some(200));
		assert_eq!(result.tokens, None);
		let result = Metrics::from_entries(&[turn("new", Some(200), Some(500))]);
		assert_eq!(result.tokens, None);
		assert_eq!(fraction(None, 510), None);
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
		assert_eq!(result.duration, None);
		assert_eq!(Metrics::from_entries(&[turn("only-delta", Some(200), Some(500))]).tokens, None);
	}
}
