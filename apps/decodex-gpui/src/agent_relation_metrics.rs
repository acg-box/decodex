//! Compare exact completed turns, never conversation age or inferred spending.
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
		entries
			.iter()
			.rev()
			.find_map(|entry| {
				if let decodex_protocol::AgentTimelineContent::TurnBoundary {
					completed: true,
					duration_ms,
					usage,
					..
				} = &entry.content
				{
					Some(Self {
						tokens: usage
							.as_ref()
							.map(|u| u.input_tokens.saturating_add(u.output_tokens)),
						duration: *duration_ms,
					})
				} else {
					None
				}
			})
			.unwrap_or_default()
	}

	fn label(self) -> String {
		format!(
			"{} tokens · {}",
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
				"Share of reported tokens",
			),
			(
				metrics.duration,
				baselines.1,
				TIME,
				time_label(metrics.duration),
				"Relative to the longest reported turn",
			),
		] {
			let part = fraction(value, baseline);
			let tip = format!(
				"{meaning}: {}\nLatest completed turn per agent. Missing values are excluded.\nThese bars compare usage, not task completion or a budget.",
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
		let running = nodes.iter().filter(|n| n.running()).count();
		let waiting = nodes
			.iter()
			.filter(|n| {
				matches!(
					n.status.as_str(),
					"Approval" | "Input needed" | "Needs you" | "Waiting on work" | "Waiting"
				)
			})
			.count();
		let unknown = nodes
			.iter()
			.filter(|n| matches!(n.status.as_str(), "Unknown" | "Status unavailable"))
			.count();

		let mut panels = gpui::div().flex().gap(gpui::px(24.)).w_full();
		for (is_tokens, title, color) in
			[(true, "Token share", TOKENS), (false, "Time · vs longest turn", TIME)]
		{
			let mut ranked: Vec<_> = nodes
				.iter()
				.filter_map(|n| {
					let m = self.metrics_for(n);
					(if is_tokens { m.tokens } else { m.duration }).map(|v| (*n, v))
				})
				.collect();
			ranked.sort_by_key(|(_, value)| std::cmp::Reverse(*value));
			let coverage = ranked.len();
			let baseline = if is_tokens {
				ranked.iter().map(|(_, v)| u128::from(*v)).sum()
			} else {
				ranked.first().map(|(_, v)| u128::from(*v)).unwrap_or(0)
			};
			let mut panel =
				gpui::div().flex_1().min_w_0().flex().flex_col().gap(gpui::px(4.)).child(
					gpui::div()
						.flex()
						.justify_between()
						.text_size(gpui::px(11.))
						.child(gpui::div().text_color(gpui::rgb(color)).child(title))
						.child(
							gpui::div()
								.text_color(gpui::rgb(TEXT_MUTED))
								.child(format!("{coverage}/{} reported", nodes.len())),
						),
				);
			if ranked.is_empty() {
				panel = panel.child(
					gpui::div()
						.text_size(gpui::px(11.))
						.text_color(gpui::rgb(TEXT_MUTED))
						.child("No completed-turn data"),
				);
			}
			for (node, value) in ranked.into_iter().take(3) {
				let row = node.row.clone().expect("agent node");
				let keyrow = row.clone();
				let label = if is_tokens {
					format!(
						"{} tok · {}",
						super::super::super::compact_tokens(value),
						percentage(fraction(Some(value), baseline))
					)
				} else {
					format!(
						"{} · {}",
						time_label(Some(value)),
						percentage(fraction(Some(value), baseline))
					)
				};
				let tip = format!(
					"{}\nLatest completed turn: {}\nNot cumulative. Cached input is included. Missing observations are excluded.",
					node.title,
					self.metrics_for(node).label()
				);
				panel = panel.child(
					gpui::div()
						.id(SharedString::from(format!("rank-{is_tokens}-{}", node.key)))
						.role(Role::Button)
						.aria_label(format!("{}: {label}", node.title))
						.tab_index(0)
						.flex()
						.items_center()
						.gap(gpui::px(8.))
						.h(gpui::px(18.))
						.cursor_pointer()
						.hover(|d| d.bg(gpui::rgba(0xffffff08)))
						.tooltip(move |_, cx| cx.new(|_| RelationTip(tip.clone())).into())
						.child(
							gpui::div()
								.w(gpui::px(128.))
								.min_w_0()
								.text_size(gpui::px(11.))
								.text_ellipsis()
								.whitespace_nowrap()
								.child(node.title.clone()),
						)
						.child(
							gpui::div()
								.flex_1()
								.h(gpui::px(4.))
								.rounded_full()
								.bg(gpui::rgba(0xffffff08))
								.child(
									gpui::div()
										.h_full()
										.w(gpui::relative(
											fraction(Some(value), baseline).unwrap_or(0.),
										))
										.rounded_full()
										.bg(gpui::rgba((color << 8) | 0xb0)),
								),
						)
						.child(
							gpui::div()
								.w(gpui::px(108.))
								.text_size(gpui::px(11.))
								.text_color(gpui::rgb(TEXT_MUTED))
								.child(label),
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
			panels = panels.child(panel);
		}
		let status = if self.command_connection_ready() {
			format!("{running} running · {waiting} waiting · {unknown} unknown")
		} else {
			"Connection unavailable · status may be stale".into()
		};
		gpui::div()
			.flex_none()
			.w_full()
			.min_w_0()
			.px_2()
			.py_1()
			.border_t_1()
			.border_color(gpui::rgba(0xffffff0c))
			.child(
				gpui::div()
					.flex()
					.justify_between()
					.mb_1()
					.text_size(gpui::px(11.))
					.text_color(gpui::rgb(TEXT_MUTED))
					.child("Latest completed turn · top 3")
					.child(status),
			)
			.child(panels)
			.into_any_element()
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
		assert_eq!(result.tokens, Some(510));
		assert_eq!(fraction(None, 510), None);
	}
}
