//! Per-agent usage, time, rate and the share of recorded conversation tokens.
use super::{AgentSurface, AnyElement, Context, Node, TEXT_MUTED};
use gpui::prelude::{
	FluentBuilder, InteractiveElement, IntoElement, ParentElement, StatefulInteractiveElement,
	Styled,
};
pub(in super::super) type Metrics = crate::ui_work_metrics::Sample;

const TOKENS: u32 = 0xa99aef;

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
		let selector = format!("relation-metrics-{}", node.key);
		let time_selector = format!("relation-time-{}", node.key);
		let token_selector = format!("relation-tokens-{}", node.key);
		let share = fraction(metrics.tokens, self.recorded_token_total());
		let tokens =
			metrics.tokens.map(super::super::super::compact_tokens).unwrap_or_else(|| "—".into());
		gpui::div()
			.id(gpui::SharedString::from(selector.clone()))
			.debug_selector(move || selector.clone())
			.role(gpui::Role::Group)
			.aria_label(metrics.label())
			.w_full()
			.h(gpui::px(layout.metrics_height()))
			.flex_none()
			.flex()
			.flex_col()
			.justify_between()
			.text_size(gpui::px(9.5 * layout.text_scale))
			.line_height(gpui::px(12. * layout.text_scale))
			.text_color(gpui::rgb(TEXT_MUTED))
			.child(
				gpui::div()
					.debug_selector(move || token_selector.clone())
					.w_full()
					.flex()
					.items_center()
					.gap(gpui::px(4. * layout.text_scale))
					.text_color(gpui::rgb(TOKENS))
					.child(
						crate::ui_work_metrics::Icon::Tokens
							.element(10. * layout.text_scale, TOKENS),
					)
					.child(format!("{tokens} tok"))
					.child(gpui::div().flex_1())
					.child(percentage(share)),
			)
			.child(
				gpui::div()
					.w_full()
					.h(gpui::px(2. * layout.chrome_scale))
					.rounded_full()
					.bg(gpui::rgba(0xa99aef18))
					.when_some(share, |bar, share| {
						bar.child(
							gpui::div()
								.w(gpui::relative(share))
								.h_full()
								.rounded_full()
								.bg(gpui::rgba(0xa99aef99)),
						)
					}),
			)
			.child(
				gpui::div()
					.w_full()
					.flex()
					.items_center()
					.justify_between()
					.child(
						gpui::div()
							.debug_selector(move || time_selector.clone())
							.flex()
							.items_center()
							.gap(gpui::px(3. * layout.text_scale))
							.whitespace_nowrap()
							.child(
								crate::ui_work_metrics::Icon::Time
									.element(9. * layout.text_scale, TEXT_MUTED),
							)
							.child(if self.work_is_archived(&node.key) {
								"Archived".into()
							} else {
								crate::ui_work_metrics::time_label(metrics.time())
							}),
					)
					.child(
						gpui::div()
							.flex_none()
							.flex()
							.items_center()
							.gap(gpui::px(3. * layout.text_scale))
							.child(
								crate::ui_work_metrics::Icon::Rate
									.element(9. * layout.text_scale, TEXT_MUTED),
							)
							.child(format!(
								"{} {}",
								metrics.rate_text(),
								if metrics.stats.is_some() {
									metrics.rate_caption()
								} else {
									"tok/s"
								}
							)),
					),
			)
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
