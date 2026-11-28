//! Compare native agent lifetimes on the same wall clock, within one parent turn.
use super::*;
use crate::ui_time;

#[derive(Clone, Default)]
pub(super) struct Timing {
	start: Option<u64>,
	finish: Option<u64>,
}

impl Timing {
	pub(super) fn record(&mut self, tool: &str, at: u64) {
		match tool {
			"subAgentActivity/started" => {
				self.start.get_or_insert(at);
			},
			"subAgentActivity/completed" | "subAgentActivity/interrupted" => {
				if self.start.is_some_and(|start| at >= start) {
					self.finish.get_or_insert(at);
				}
			},
			_ => {},
		}
	}

	fn interval(&self, now: Option<u64>) -> Option<(u64, u64)> {
		let start = self.start?;
		let end = self.finish.or(now)?;
		(end >= start).then_some((start, end))
	}
}

// Count each millisecond once, including when three or more agents overlap.
fn overlap(intervals: &[(u64, u64)], bounds: (u64, u64)) -> u64 {
	let mut events = BTreeMap::<u64, i32>::new();
	for &(start, end) in intervals {
		let (start, end) = (start.max(bounds.0), end.min(bounds.1));
		if start < end {
			*events.entry(start).or_default() += 1;
			*events.entry(end).or_default() -= 1;
		}
	}
	let (mut count, mut previous, mut total) = (0, bounds.0, 0);
	for (at, change) in events {
		if count >= 2 {
			total += at - previous;
		}
		previous = at;
		count += change;
	}
	total
}

impl AgentSurface {
	pub(super) fn agent_timing(
		&self,
		thread: &str,
		turn: &str,
		members: &BTreeMap<String, Participant>,
		selected: Option<&str>,
		cx: &mut Context<Self>,
	) -> Option<Div> {
		let now = self
			.timeline
			.native
			.binding
			.as_ref()
			.filter(|b| b.thread == thread)
			.and_then(|b| self.conversation_record(&b.work))
			.filter(|w| w.active_turn_id.as_deref() == Some(turn))
			.and_then(|_| u64::try_from(ui_time::now() / 1000).ok());
		let mut agents: Vec<_> = members
			.iter()
			.filter_map(|(id, m)| {
				let live = now.filter(|_| {
					matches!(
						m.state,
						ParticipantState::Working
							| ParticipantState::NeedsInput
							| ParticipantState::NeedsApproval
					)
				});
				Some((id, m, m.timing.interval(live)?))
			})
			.collect();
		let start = agents.iter().map(|a| a.2.0).min()?;
		let end = agents.iter().map(|a| a.2.1).max()?;
		let intervals: Vec<_> = agents.iter().map(|a| a.2).collect();
		let span = (end - start).max(1) as f64;
		let mut peers = Vec::new();
		let bounds = if let Some(id) = selected {
			let (_, _, bounds) = *agents.iter().find(|a| a.0 == id)?;
			peers.extend(
				agents
					.iter()
					.filter(|a| a.0 != id && a.2.0 < bounds.1 && bounds.0 < a.2.1)
					.map(|a| a.1.name.as_str()),
			);
			agents.retain(|a| a.0 == id);
			bounds
		} else {
			(start, end)
		};
		let total = agents.len();
		if total > 4 {
			agents.sort_by_key(|a| std::cmp::Reverse(a.2.1 - a.2.0));
			agents.truncate(4);
		}
		agents.sort_by_key(|a| (a.2.0, a.0));
		let heading = if total > 4 { "Agent timeline (longest 4)" } else { "Agent timeline" };
		let mut body = reading_body().child(
			gpui::div()
				.flex()
				.flex_wrap()
				.items_baseline()
				.justify_between()
				.gap(gpui::px(4.))
				.child(section(heading))
				.when(selected.is_none() && intervals.len() > 1, |row| {
					let label = format!("{} elapsed", ui_time::duration(end - start));
					row.child(
						gpui::div()
							.id("agent-timeline-span")
							.role(Role::Label)
							.aria_label(label.clone())
							.text_size(gpui::px(ui_theme::NAVIGATION_DETAIL_SIZE))
							.text_color(ui_theme::rgb(SECONDARY))
							.child(label)
							.hover_hint(
								"Time from the first observed start to the last observed activity.",
							),
					)
				}),
		);
		for (id, member, (from, to)) in agents {
			let duration = ui_time::duration(to - from);
			let observed = if member.timing.finish.is_some() {
				duration
			} else {
				format!("{duration} so far")
			};
			let offset = (from - start) as f64 / span;
			let width = (to - from) as f64 / span;
			let page = ReferencePage::Participant {
				thread: thread.into(),
				turn: turn.into(),
				agent: id.clone(),
			};
			let label = format!("{} ({}). Open contribution", member.name, observed);
			let track = gpui::div()
				.relative()
				.h(gpui::px(4.))
				.w_full()
				.rounded_full()
				.bg(ui_theme::rgb(SECONDARY).opacity(0.14))
				.child(
					gpui::div()
						.absolute()
						.h_full()
						.rounded_full()
						.min_w(gpui::px(1.))
						.left(gpui::relative(offset as f32))
						.w(gpui::relative(width as f32))
						.bg(ui_theme::rgb(BLUE).opacity(0.85)),
				);
			body = body.child(
				self.reference_control(
					format!("agent-timing-{turn}-{id}"),
					label,
					move |s, cx| s.show_reference(page.clone(), cx),
					cx,
				)
				.w_full()
				.flex_col()
				.items_start()
				.gap(gpui::px(3.))
				.px(gpui::px(3.))
				.py(gpui::px(4.))
				.bg(gpui::transparent_black())
				.child(
					gpui::div()
						.w_full()
						.flex()
						.items_center()
						.justify_between()
						.gap(gpui::px(4.))
						.child(gpui::div().min_w_0().truncate().child(member.name.clone()))
						.child(
							gpui::div()
								.flex_none()
								.text_color(ui_theme::rgb(SECONDARY))
								.child(observed),
						),
				)
				.child(track)
				.hover_hint("Open contribution")
				.smooth(),
			);
		}
		if intervals.len() > 1 {
			let parallel = overlap(&intervals, bounds);
			let summary = if parallel > 0 {
				let time = ui_time::duration(parallel);
				match peers.as_slice() {
					[] => format!("{time} in parallel"),
					[other] => format!("{time} alongside {other}"),
					others => format!("{time} alongside {} agents", others.len()),
				}
			} else if intervals.len() == members.len()
				&& members.values().all(|m| m.timing.finish.is_some())
			{
				"Ran sequentially".into()
			} else {
				return Some(body);
			};
			body = body.child(
				gpui::div()
					.id("agent-overlap")
					.role(Role::Label)
					.aria_label(summary.clone())
					.text_size(gpui::px(ui_theme::NAVIGATION_DETAIL_SIZE))
					.text_color(ui_theme::rgb(SECONDARY))
					.child(summary)
					.hover_hint(if peers.is_empty() {
						"Elapsed time when at least two recorded agent runs were open.".to_owned()
					} else {
						format!("Overlapping agents: {}", peers.join(", "))
					}),
			);
		}
		Some(body)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn overlapping_runs_count_wall_time_once_and_clip_to_the_selected_agent() {
		let intervals = [(0, 30), (10, 40), (20, 25), (40, 45)];
		assert_eq!(overlap(&intervals, (0, 45)), 20);
		assert_eq!(overlap(&intervals, (20, 25)), 5);
		assert_eq!(overlap(&intervals, (40, 45)), 0);
		let mut clock = Timing::default();
		clock.record("subAgentActivity/completed", 8);
		clock.record("subAgentActivity/started", 10);
		assert_eq!(clock.interval(None), None);
		assert_eq!(clock.interval(Some(13)), Some((10, 13)));
		clock.record("subAgentActivity/interrupted", 14);
		clock.record("subAgentActivity/completed", 29);
		assert_eq!(clock.interval(Some(50)), Some((10, 14)));
	}
}
