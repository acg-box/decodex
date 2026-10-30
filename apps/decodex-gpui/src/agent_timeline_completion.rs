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

use crate::{
	frame_trace,
	shell::agent_surface::native_timeline::{
		AgentTimelineEntry,
		groups::{Group, HEADER_HEIGHT},
	},
	ui_motion,
};

const DURATION: Duration = Duration::from_millis(280);

#[derive(Default)]
pub(super) struct Completions(RefCell<Presented>);
impl Completions {
	pub(super) fn prepare(&self, entries: &[AgentTimelineEntry], groups: &[Group]) {
		let mut state = self.0.borrow_mut();

		state.closing.retain(|turn, motion| {
			!motion.finished.get() && groups.iter().any(|g| &g.turn == turn && !g.expanded)
		});

		let mut folded = BTreeSet::new();

		for group in groups {
			let positions: BTreeSet<_> =
				group.indices.iter().map(|i| entries[*i].position).collect();
			let outgoing = group
				.indices
				.iter()
				.filter_map(|i| {
					let entry = &entries[*i];

					(state.normal.contains(&entry.position)
						|| state.live.contains(super::key(entry).2))
					.then_some(entry.position)
				})
				.collect::<BTreeSet<_>>();

			folded.extend(positions);

			if !group.expanded && !outgoing.is_empty() && !ui_motion::reduced() {
				let header_present = state.headers.contains(&group.turn);
				let ready = state
					.outputs
					.entry(super::key(&entries[group.final_index]).2.into())
					.or_default()
					.clone();

				state.closing.insert(
					group.turn.clone(),
					Rc::new(Completion::new(outgoing, header_present, ready)),
				);
			}
		}

		state.normal = entries.iter().map(|e| e.position).filter(|p| !folded.contains(p)).collect();
		state.headers = groups.iter().map(|g| g.turn.clone()).collect();

		for entry in entries {
			state.live.remove(super::key(entry).2);
		}
	}

	pub(super) fn present_live(&self, item: &str) {
		self.0.borrow_mut().live.insert(item.into());
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

	pub(super) fn output(&self, item: &str) -> Rc<Cell<bool>> {
		self.0.borrow_mut().outputs.entry(item.into()).or_default().clone()
	}
}

pub(super) struct Completion {
	pub outgoing: BTreeSet<u64>,
	header_present: bool,
	ready: Rc<Cell<bool>>,
	started: Cell<Option<Instant>>,
	height: Cell<Option<f32>>,
	finished: Cell<bool>,
}
impl Completion {
	fn new(outgoing: BTreeSet<u64>, header_present: bool, ready: Rc<Cell<bool>>) -> Self {
		Self {
			outgoing,
			header_present,
			ready,
			started: Cell::new(None),
			height: Cell::new(None),
			finished: Cell::new(false),
		}
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
		let height = self.motion.height.get();

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
										self.motion.height.set(Some(bounds.size.height.into()));

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
	normal: BTreeSet<u64>,
	live: BTreeSet<String>,
	headers: BTreeSet<String>,
	closing: BTreeMap<String, Rc<Completion>>,
	outputs: BTreeMap<String, Rc<Cell<bool>>>,
}

#[cfg(test)]
mod tests {
	use crate::shell::agent_surface::native_timeline::completion::{
		BTreeSet, Cell, Completion, DURATION, Duration, Instant, Rc,
	};

	#[test]
	fn completion_waits_for_the_displayed_answer_and_finishes_without_a_replay() {
		let now = Instant::now();
		let ready = Rc::new(Cell::new(false));
		let motion = Completion::new(BTreeSet::from([1, 2]), false, ready.clone());

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
		let motion = Completion::new(BTreeSet::from([1]), false, Rc::new(Cell::new(false)));

		assert_eq!(motion.progress(Instant::now(), true), 1.);
		assert_eq!(motion.progress(Instant::now(), false), 1.);
	}
}
