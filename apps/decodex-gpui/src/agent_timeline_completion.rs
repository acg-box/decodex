//! Retain the displayed process until its completion transition has finished.
use std::{
	cell::{Cell, RefCell},
	collections::{BTreeMap, BTreeSet},
	rc::Rc,
	time::{Duration, Instant},
};

use gpui::{
	AnyElement, App, IntoElement, RenderOnce, Window,
	prelude::{FluentBuilder as _, InteractiveElement as _, ParentElement as _, Styled as _},
};

use super::super::response_metrics::FooterPresentation;
use crate::{
	frame_trace,
	shell::agent_surface::native_timeline::{
		AgentTimelineContent, AgentTimelineEntry, ItemKey,
		groups::{Group, HEADER_HEIGHT},
		motion::ToolBatch,
	},
	ui_motion,
	ui_theme::PROCESS_STEP_GAP,
};

const DURATION: Duration = Duration::from_millis(280);

#[derive(Default)]
pub(super) struct Completions(RefCell<Presented>);
impl Completions {
	pub(super) fn prepare(
		&self,
		entries: &[AgentTimelineEntry],
		groups: &[Group],
		batches: &BTreeMap<usize, ToolBatch>,
		row_height: impl Fn(&AgentTimelineEntry) -> Option<f32>,
	) {
		let mut state = self.0.borrow_mut();

		state.closing.retain(|turn, motion| {
			!motion.finished.get() && groups.iter().any(|g| &g.turn == turn && !g.expanded)
		});

		let folded: BTreeSet<_> = groups
			.iter()
			.flat_map(|group| group.indices.iter().map(|i| entries[*i].position))
			.collect();
		let reduced = ui_motion::reduced();

		for group in groups {
			if group.expanded || reduced {
				continue;
			}
			let anchor = entries[group.first_index].position;
			let replaces_batch = state.rows.get(&anchor).copied().flatten();
			let header_present = state.headers.contains(&group.turn) || replaces_batch.is_some();
			let outgoing = group
				.indices
				.iter()
				.filter_map(|i| {
					let entry = &entries[*i];
					if entry.position == anchor
						&& replaces_batch.is_some_and(|batch| !batch.expanded)
					{
						// The completed process takes over the visible batch caption in place.
						return None;
					}

					let batch = state.rows.get(&entry.position).copied().or_else(|| {
						ItemKey::for_entry(entry)
							.is_some_and(|key| state.live.contains(&key))
							.then_some(None)
					})?;
					Some((entry.position, batch))
				})
				.collect::<BTreeMap<_, _>>();

			if !outgoing.is_empty() {
				let ready = if let AgentTimelineContent::Item { turn_id, item_id, kind, .. } =
					&entries[group.final_index].content
					&& kind == "agentMessage"
				{
					let key = ItemKey::new(turn_id, item_id);
					state.outputs.entry(key).or_default().ready.clone()
				} else {
					Rc::new(Cell::new(true))
				};
				let height = group
					.indices
					.iter()
					.map(|i| &entries[*i])
					.filter(|entry| outgoing.contains_key(&entry.position))
					.enumerate()
					.try_fold(0., |height, (i, entry)| {
						let extent = row_height(entry)?;
						let keeps_header = entry.position == anchor && replaces_batch.is_some();
						let extra = if keeps_header {
							-HEADER_HEIGHT
						} else if i > 0 || header_present {
							PROCESS_STEP_GAP
						} else {
							0.
						};
						Some(height + (extent + extra).max(0.))
					});

				state.closing.insert(
					group.turn.clone(),
					Rc::new(Completion::new(outgoing, header_present, ready, height)),
				);
			}
		}

		state.rows = entries
			.iter()
			.enumerate()
			.filter_map(|(i, entry)| {
				let inside_batch = batches
					.range(..i)
					.next_back()
					.is_some_and(|(start, batch)| i < start + batch.count);
				(!inside_batch && !folded.contains(&entry.position))
					.then(|| (entry.position, batches.get(&i).copied()))
			})
			.collect();
		state.headers = groups.iter().map(|g| g.turn.clone()).collect();

		for entry in entries {
			if let Some(key) = ItemKey::for_entry(entry) {
				state.live.remove(&key);
			}
		}
	}

	pub(super) fn present_live(&self, turn: &str, item: &str) {
		self.0.borrow_mut().live.insert(ItemKey::new(turn, item));
	}

	pub(super) fn replace_history(&mut self, same_source: bool) {
		// A refreshed page can replace its window without replacing the conversation.
		// Preserve what was displayed, including rows already promoted from the stream.
		if !same_source {
			*self = Self::default();
		}
	}

	pub(super) fn get(&self, group: &Group) -> Option<Rc<Completion>> {
		self.0.borrow().closing.get(&group.turn).cloned()
	}

	pub(super) fn output(&self, turn: &str, item: &str) -> Rc<Cell<bool>> {
		self.footer(turn, item).ready
	}

	pub(super) fn footer(&self, turn: &str, item: &str) -> FooterPresentation {
		self.0.borrow_mut().outputs.entry(ItemKey::new(turn, item)).or_default().clone()
	}
}

pub(super) struct Completion {
	/// Displayed source rows. A batch replaces its member rows with one disclosure.
	pub outgoing: BTreeMap<u64, Option<ToolBatch>>,
	pub header_present: bool,
	ready: Rc<Cell<bool>>,
	started: Cell<Option<Instant>>,
	height: Cell<Option<f32>>,
	height_limit: Option<f32>,
	finished: Cell<bool>,
}
impl Completion {
	fn new(
		outgoing: BTreeMap<u64, Option<ToolBatch>>,
		header_present: bool,
		ready: Rc<Cell<bool>>,
		height_limit: Option<f32>,
	) -> Self {
		Self {
			outgoing,
			header_present,
			ready,
			started: Cell::new(None),
			height: Cell::new(None),
			height_limit,
			finished: Cell::new(false),
		}
	}

	pub(super) fn batch(&self, position: u64) -> Option<ToolBatch> {
		self.outgoing.get(&position).copied().flatten()
	}

	fn progress(&self, now: Instant, reduced: bool) -> f32 {
		if self.height.get().is_some() && self.ready.get() && self.started.get().is_none() {
			self.started.set(Some(now));
		}

		let t = if reduced || self.finished.get() {
			1.
		} else {
			self.started.get().map_or(0., |at| {
				(now.saturating_duration_since(at).as_secs_f32() / DURATION.as_secs_f32()).min(1.)
			})
		};

		self.finished.set(t == 1.);

		t * t * (3. - 2. * t)
	}
}

#[derive(IntoElement)]
pub(super) struct Closing {
	pub motion: Rc<Completion>,
	pub position: u64,
	pub header: AnyElement,
	pub body: AnyElement,
}
impl RenderOnce for Closing {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let progress = self.motion.progress(Instant::now(), ui_motion::reduced());
		let height = self.motion.height.get().or(self.motion.height_limit);

		frame_trace::row_arrival(self.position, "fold", progress, height.unwrap_or(0.));
		// Retire the outgoing tree on the next frame, before a new pointer gesture.
		ui_motion::request_frame(window, cx);

		let header_progress = if self.motion.header_present { 1. } else { progress };

		gpui::div()
			.w_full()
			.min_w_0()
			.flex()
			.flex_col()
			.child(
				gpui::div()
					.w_full()
					.h(gpui::px(HEADER_HEIGHT * header_progress))
					.flex_none()
					.overflow_hidden()
					.opacity(header_progress)
					.child(self.header),
			)
			.when(progress < 1., |block| {
				block.child(
					gpui::div()
						.w_full()
						.min_w_0()
						.flex_none()
						.overflow_hidden()
						.debug_selector(|| "turn-process-finishing".into())
						.when_some(height, |body, h| body.h(gpui::px(h * (1. - progress))))
						.opacity(1. - progress)
						.child(
							gpui::div()
								.w_full()
								.min_w_0()
								.flex_none()
								.on_children_prepainted(move |bounds, window, cx| {
									if let Some(bounds) = bounds.first() {
										let measured = f32::from(bounds.size.height);
										self.motion.height.set(Some(
											self.motion
												.height_limit
												.map_or(measured, |limit| limit.min(measured)),
										));

										if self.motion.started.get().is_none() {
											ui_motion::request_frame(window, cx);
										}
									}
								})
								.child(self.body),
						),
				)
			})
	}
}

#[derive(Default)]
struct Presented {
	rows: BTreeMap<u64, Option<ToolBatch>>,
	live: BTreeSet<ItemKey>,
	headers: BTreeSet<String>,
	closing: BTreeMap<String, Rc<Completion>>,
	outputs: BTreeMap<ItemKey, FooterPresentation>,
}

#[cfg(test)]
mod tests {
	use crate::shell::agent_surface::native_timeline::{
		AgentTimelineContent, AgentTimelineEntry,
		completion::{BTreeSet, Cell, Completion, Completions, DURATION, Duration, Instant, Rc},
		groups,
	};

	#[test]
	fn a_steer_or_terminal_boundary_closes_without_waiting_for_a_missing_final_answer() {
		for status in ["failed", "interrupted", "steer"] {
			let mut entries = vec![
				AgentTimelineEntry {
					position: 1,
					content: AgentTimelineContent::Item {
						turn_id: "turn".into(),
						item_id: "reasoning".into(),
						kind: "reasoning".into(),
						phase: None,
						text: "Checking the result".into(),
						truncated: false,
						attachments: Vec::new(),
						activity: None,
						collaboration: None,
					},
				},
				AgentTimelineEntry {
					position: 2,
					content: AgentTimelineContent::TurnBoundary {
						started_at_micros: None,
						turn_id: "turn".into(),
						completed: true,
						status: Some(status.into()),
						duration_ms: None,
						timing: None,
						usage_summary: None,
						usage: None,
						error: None,
					},
				},
			];
			if status == "steer" {
				let mut input = entries[0].content.clone();
				if let AgentTimelineContent::Item { kind, item_id, .. } = &mut input {
					*kind = "userMessage".into();
					*item_id = "steer".into();
				}
				entries[1].content = input;
			}
			let groups = groups::groups(&entries, &BTreeSet::new());
			let state = Completions::default();
			state.present_live("turn", "reasoning");
			state.prepare(&entries, &groups, &Default::default(), |_| None);
			let motion = state.get(&groups[0]).expect("displayed process closes");
			motion.height.set(Some(120.));
			let now = Instant::now();
			assert_eq!(motion.progress(now, false), 0.);
			assert_eq!(motion.progress(now + DURATION, false), 1., "{status}");
		}
	}

	#[test]
	fn completion_waits_for_the_displayed_answer_and_finishes_without_a_replay() {
		let now = Instant::now();
		let ready = Rc::new(Cell::new(false));
		let motion = Completion::new([(1, None), (2, None)].into(), false, ready.clone(), None);

		motion.height.set(Some(120.));

		assert_eq!(motion.progress(now, false), 0.);
		assert_eq!(motion.progress(now + Duration::from_secs(1), false), 0.);

		ready.set(true);

		assert_eq!(motion.progress(now + Duration::from_secs(1), false), 0.);

		let middle = motion.progress(now + Duration::from_secs(1) + DURATION / 2, false);

		assert!((middle - 0.5).abs() < f32::EPSILON);
		assert_eq!(motion.progress(now + Duration::from_secs(1) + DURATION, false), 1.);
		assert_eq!(motion.progress(now + Duration::from_secs(2), false), 1.);
	}

	#[test]
	fn background_completion_settles_immediately_and_does_not_restart() {
		let motion = Completion::new([(1, None)].into(), false, Rc::new(Cell::new(false)), None);

		assert_eq!(motion.progress(Instant::now(), true), 1.);
		assert_eq!(motion.progress(Instant::now(), false), 1.);
	}
}
