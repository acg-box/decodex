//! Preserve a visible native row after pagination, independently of user-message rail marks.
use std::{
	cell::RefCell,
	collections::{BTreeMap, BTreeSet},
	mem,
	rc::Rc,
	time::Duration,
};

use gpui::{AnyElement, InteractiveElement, IntoElement, ParentElement, Styled, Window};

use crate::shell::agent_surface::{
	activity::HistoryKey,
	native_timeline::{
		self, AgentSurface, AgentTimelineEntry, AgentTimelinePage, AgentWorkItemDto, Binding,
		Context,
	},
};

type RowKey = (u64, u8, String);

pub(super) const ROW_GAP: f32 = 8.;

#[derive(Default)]
pub(super) struct Viewport(Rc<RefCell<Geometry>>);
impl Viewport {
	fn prepare_layout(&self, layout: Layout) {
		let mut state = self.0.borrow_mut();

		if state.layout != Some(layout) {
			state.rows.clear();

			state.layout = Some(layout);
		}
	}

	/// Preserve measured space outside a bounded overscan buffer.
	/// Never estimate heights or cull while a pagination anchor is pending.
	pub(super) fn offscreen_height(
		&self,
		entry: &AgentTimelineEntry,
		offset: f32,
		height: f32,
	) -> Option<f32> {
		let state = self.0.borrow();

		if state.pending.is_some() || state.pinned.as_ref() == Some(&row_key(entry)) || height <= 0.
		{
			return None;
		}

		let (top, row_height) = *state.rows.get(&row_key(entry))?;
		let y = top + offset;
		let overscan = (height * 0.25).clamp(80., 160.);

		(y + row_height < -overscan || y > height + overscan).then_some(row_height)
	}

	pub(super) fn request_latest(&self) {
		self.0.borrow_mut().latest_requested = true;
	}

	pub(super) fn take_latest_request(&self) -> bool {
		mem::take(&mut self.0.borrow_mut().latest_requested)
	}

	pub(super) fn retain(&self, entries: &[AgentTimelineEntry]) {
		let keys = entries.iter().map(row_key).collect::<BTreeSet<_>>();
		let mut state = self.0.borrow_mut();

		state.rows.retain(|key, _| keys.contains(key));

		if state.pending.as_ref().is_some_and(|anchor| !keys.contains(&anchor.key)) {
			state.pending = None;
			state.process_motion_until = None;
		}
	}

	fn capture(&self, offset: f32, height: f32) {
		let mut state = self.0.borrow_mut();

		state.revision = state.revision.wrapping_add(1);
		state.pending = state
			.rows
			.iter()
			.find(|(_, (top, row_height))| {
				*top + offset < height && *top + *row_height + offset > 0.0
			})
			.map(|(key, (top, _))| Anchor {
				key: key.clone(),
				viewport_top: *top + offset,
				revision: state.revision,
				scheduled: false,
			});
	}
}

#[derive(Default)]
struct Geometry {
	rows: BTreeMap<RowKey, (f32, f32)>,
	folded: BTreeSet<RowKey>,
	layout: Option<Layout>,
	pinned: Option<RowKey>,
	pending: Option<Anchor>,
	process_motion_until: Option<std::time::Instant>,
	revision: u64,
	latest_requested: bool,
}
impl Geometry {
	fn measure(&mut self, key: RowKey, top: f32, height: f32) -> Option<u64> {
		self.rows.insert(key.clone(), (top, height));

		let pending = self.pending.as_mut()?;

		if pending.key != key || pending.scheduled {
			return None;
		}

		pending.scheduled = true;

		Some(pending.revision)
	}

	fn finish(&mut self, revision: u64, maximum: f32) -> Option<f32> {
		if self.pending.as_ref()?.revision != revision {
			return None;
		}

		let pending = self.pending.take()?;
		let (top, _) = self.rows.get(&pending.key)?;
		let offset = (pending.viewport_top - *top).clamp(-maximum.max(0.0), 0.0);

		if self.process_motion_until.is_some_and(|until| std::time::Instant::now() < until) {
			self.pending = Some(Anchor { scheduled: false, ..pending });
		} else {
			self.process_motion_until = None;
		}

		Some(offset)
	}
}

#[derive(Clone, Copy, PartialEq)]
struct Layout {
	window_width: f32,
	left_width: f32,
	right_width: f32,
	transcript_width: f32,
	left_visible: bool,
	right_visible: bool,
	graph_expanded: bool,
}

struct Anchor {
	key: RowKey,
	viewport_top: f32,
	revision: u64,
	scheduled: bool,
}

impl AgentSurface {
	pub(super) fn native_pagination_settling(&self) -> bool {
		self.timeline.native.viewport.0.borrow().pending.is_some()
	}

	pub(in super::super) fn prepare_history_layout(&self, window: &Window) {
		let width = self
			.selected
			.as_ref()
			.and_then(|work| self.timeline.scroll.get(work))
			.map_or(0., |scroll| f32::from(scroll.bounds().size.width));

		self.timeline.native.viewport.prepare_layout(Layout {
			window_width: f32::from(window.viewport_size().width),
			left_width: self.workspace.sidebar_width,
			right_width: self.workspace.agent_panel_width,
			transcript_width: width,
			left_visible: self.workspace.sidebar_visible,
			right_visible: self.workspace.agent_tree_visible,
			graph_expanded: self.workspace.graph_expanded,
		});
	}

	pub(in super::super) fn anchor_process_toggle(
		&mut self,
		work: &str,
		entry: &AgentTimelineEntry,
	) {
		self.timeline.latest_follow_work = None;

		self.timeline.follow_paused.insert(work.into());

		self.timeline.navigation = None;

		let key = row_key(entry);
		let mut state = self.timeline.native.viewport.0.borrow_mut();

		state.process_motion_until = Some(std::time::Instant::now() + Duration::from_millis(240));

		if let (Some((top, _)), Some(scroll)) =
			(state.rows.get(&key), self.timeline.scroll.get(work))
		{
			let viewport_top = *top + f32::from(scroll.offset().y);

			state.revision += 1;
			state.pending =
				Some(Anchor { key, viewport_top, revision: state.revision, scheduled: false });
		}

		state.rows.clear();

		self.timeline.wheel_scroll = None;
	}

	pub(super) fn prepare_process_folds(&self, _work: &AgentWorkItemDto, hidden: &BTreeSet<usize>) {
		let folded = hidden.iter().map(|i| row_key(&self.timeline.native.entries[*i])).collect();
		let mut state = self.timeline.native.viewport.0.borrow_mut();

		if state.folded != folded {
			state.folded = folded;

			state.rows.clear();
		}
	}

	pub(in super::super) fn cancel_native_scroll_anchor(&self) {
		let mut state = self.timeline.native.viewport.0.borrow_mut();

		state.pending = None;
		state.process_motion_until = None;
		state.latest_requested = false;
	}

	pub(in super::super) fn prepend_native_history(
		&mut self,
		binding: &Binding,
		cursor: &str,
		page: AgentTimelinePage,
	) -> bool {
		if self.timeline.native.binding.as_ref() == Some(binding)
			&& !self.timeline.native.show_saved
			&& self.timeline.navigation.is_none()
			&& let Some(scroll) = self.timeline.scroll.get(&binding.work)
		{
			self.timeline
				.native
				.viewport
				.capture(scroll.offset().y.into(), scroll.bounds().size.height.into());
		} else {
			self.cancel_native_scroll_anchor();
		}

		let accepted = self.timeline.native.prepend(binding, cursor, page);

		if accepted {
			// The saved viewport anchor now owns the offset in the new layout.
			self.timeline.wheel_scroll = None;
		}
		if !accepted {
			self.cancel_native_scroll_anchor();
		}

		accepted
	}

	/// One layout node for consecutive offscreen rows, retaining their exact anchors.
	pub(super) fn native_history_spacer(
		&self,
		work: &AgentWorkItemDto,
		rows: Vec<(&AgentTimelineEntry, f32)>,
	) -> AnyElement {
		let scroll = self.timeline.scroll[&work.id].clone();
		let geometry = self.timeline.native.viewport.0.clone();
		let mut height = 0.;
		let rows: Vec<_> = rows
			.into_iter()
			.map(|(entry, row_height)| {
				let offset = height;

				height += row_height + ROW_GAP;

				let mark = self
					.timeline
					.marks
					.get(&HistoryKey::native(
						work.codex_thread_id.as_deref().unwrap_or_default(),
						entry,
					))
					.map(|mark| mark.position.clone());

				(row_key(entry), offset, row_height, mark)
			})
			.collect();

		height = (height - ROW_GAP).max(0.);

		gpui::div()
			.w_full()
			.flex_none()
			.debug_selector(|| "native-history-spacer".into())
			.child(gpui::div().h(gpui::px(height)))
			.on_children_prepainted(move |bounds, _, _| {
				let Some(bounds) = bounds.first() else { return };
				let top = f32::from(bounds.origin.y - scroll.bounds().origin.y - scroll.offset().y);
				let mut geometry = geometry.borrow_mut();

				for (key, offset, height, mark) in &rows {
					geometry.rows.insert(key.clone(), (top + offset, *height));

					if let Some(mark) = mark {
						mark.set(top + offset);
					}
				}
			})
			.into_any_element()
	}

	pub(super) fn native_scroll_row(
		&self,
		work: &AgentWorkItemDto,
		entry: &AgentTimelineEntry,
		row: AnyElement,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let Some(scroll) = self.timeline.scroll.get(&work.id).cloned() else {
			return row;
		};
		let key = row_key(entry);
		let geometry = self.timeline.native.viewport.0.clone();
		let surface = cx.entity().downgrade();
		let owner = work.id.clone();
		let selection = geometry.clone();
		let selected_key = key.clone();

		gpui::div()
			.w_full()
			.min_w_0()
			.flex_none()
			.child(row)
			.capture_any_mouse_down(move |_, _, _| {
				// Keep the interacted row alive so scrolling cannot discard text selection.
				selection.borrow_mut().pinned = Some(selected_key.clone());
			})
			.on_children_prepainted(move |bounds, _, cx| {
				let Some(bounds) = bounds.first() else {
					return;
				};
				let top = f32::from(bounds.origin.y - scroll.bounds().origin.y - scroll.offset().y);
				let Some(revision) =
					geometry.borrow_mut().measure(key.clone(), top, bounds.size.height.into())
				else {
					return;
				};
				let (geometry, surface, scroll, owner) =
					(geometry.clone(), surface.clone(), scroll.clone(), owner.clone());

				cx.defer(move |cx| {
					let _ = surface.update(cx, |s, cx| {
						if !Rc::ptr_eq(&geometry, &s.timeline.native.viewport.0)
							|| s.selected.as_ref() != Some(&owner)
							|| s.timeline.native.show_saved
						{
							return;
						}

						if let Some(offset) =
							geometry.borrow_mut().finish(revision, scroll.max_offset().y.into())
							&& (f32::from(scroll.offset().y) - offset).abs() > 0.1
						{
							scroll.set_offset(gpui::point(scroll.offset().x, gpui::px(offset)));
							cx.notify();
						}
					});
				});
			})
			.into_any_element()
	}
}

fn row_key(entry: &AgentTimelineEntry) -> RowKey {
	let (position, kind, id) = native_timeline::key(entry);

	(position, kind, id.into())
}

#[cfg(test)]
mod tests {
	use std::{future, thread};

	use crate::shell::agent_surface::native_timeline::scroll::{
		self, AgentSurface, AgentTimelineEntry, AgentTimelinePage, Anchor, Binding, Geometry,
		Layout, Viewport,
	};

	#[test]
	fn disclosure_anchor_tracks_each_layout_until_motion_finishes() {
		let key = scroll::row_key(&row(1));
		let mut geometry = Geometry {
			pending: Some(Anchor {
				key: key.clone(),
				viewport_top: 100.,
				revision: 1,
				scheduled: false,
			}),
			process_motion_until: Some(
				std::time::Instant::now() + std::time::Duration::from_secs(1),
			),
			..Default::default()
		};

		assert_eq!(geometry.measure(key.clone(), 300., 20.), Some(1));
		assert_eq!(geometry.finish(1, 1_000.), Some(-200.));
		assert_eq!(geometry.measure(key.clone(), 420., 20.), Some(1));
		assert_eq!(geometry.finish(1, 1_000.), Some(-320.));

		geometry.process_motion_until = Some(std::time::Instant::now());

		assert_eq!(geometry.measure(key, 500., 20.), Some(1));
		assert_eq!(geometry.finish(1, 1_000.), Some(-400.));
		assert!(geometry.pending.is_none());
	}

	fn row(position: u64) -> AgentTimelineEntry {
		AgentTimelineEntry {
			position,
			content: decodex_protocol::AgentTimelineContent::Item {
				phase: None,
				app_ui: false,
				turn_id: "turn".into(),
				item_id: format!("item-{position}"),
				kind: "agentMessage".into(),
				text: "Assistant content without a user-message anchor.\n\n".repeat(15),
				truncated: false,
				activity: None,
				attachments: vec![],
			},
		}
	}

	#[test]
	fn measured_rows_keep_space_and_selection_with_a_viewport_buffer() {
		let viewport = Viewport::default();
		let entry = row(1);

		viewport.0.borrow_mut().measure(scroll::row_key(&entry), 5_000., 700.);

		assert_eq!(viewport.offscreen_height(&entry, 0., 900.), Some(700.));
		assert_eq!(viewport.offscreen_height(&entry, -4_000., 900.), None);
		assert_eq!(viewport.offscreen_height(&entry, -5_800., 900.), None);
		assert_eq!(viewport.offscreen_height(&entry, -7_000., 900.), Some(700.));

		viewport.0.borrow_mut().pinned = Some(scroll::row_key(&entry));

		assert_eq!(viewport.offscreen_height(&entry, 0., 900.), None);
	}

	#[test]
	fn layout_changes_invalidate_measured_heights() {
		let viewport = Viewport::default();
		let mut layout = Layout {
			window_width: 1_400.,
			left_width: 240.,
			right_width: 240.,
			transcript_width: 900.,
			left_visible: true,
			right_visible: true,
			graph_expanded: false,
		};

		viewport.prepare_layout(layout);

		let entry = row(1);

		viewport.0.borrow_mut().measure(scroll::row_key(&entry), 5_000., 700.);
		viewport.prepare_layout(layout);

		assert_eq!(viewport.offscreen_height(&entry, 0., 900.), Some(700.));

		layout.left_width = 300.;

		viewport.prepare_layout(layout);

		assert_eq!(viewport.offscreen_height(&entry, 0., 900.), None);
	}

	fn initialize_latest_history(
		s: &mut AgentSurface,
		cx: &mut gpui::Context<AgentSurface>,
	) -> Binding {
		s.visual_workspace_fixture(cx);

		s.workspace.graph_visible = false;

		let work = s
			.snapshot
			.as_mut()
			.unwrap()
			.work_items
			.iter_mut()
			.find(|work| Some(&work.id) == s.selected.as_ref())
			.unwrap();

		work.codex_thread_id = Some("thread".into());

		let binding =
			Binding { work: work.id.clone(), thread: "thread".into(), account: "account".into() };

		assert!(s.timeline.native.replace(
			binding.clone(),
			AgentTimelinePage {
				thread_id: "thread".into(),
				entries: vec![row(10)],
				next_cursor: None,
				weather: Default::default(),
				safety_buffering_turn_id: None,
				active_realtime_session_at_page_start: None,
			}
		));

		cx.notify();

		binding
	}

	#[gpui::test]
	fn latest_arrival_scrolls_new_layout_but_respects_a_later_wheel_gesture(
		cx: &mut gpui::TestAppContext,
	) {
		for (cancel, cold, summary) in [
			(false, false, false),
			(true, false, false),
			(false, true, false),
			(false, false, true),
			(true, false, true),
			(false, true, true),
		] {
			let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

			visual.simulate_resize(gpui::size(gpui::px(1_400.), gpui::px(500.)));

			let binding = surface.update(visual, initialize_latest_history);

			visual.update(|window, cx| {
				window.draw(cx).clear();
			});

			surface.update(visual, |s, cx| {
				s.workspace.details_visible = true;
				cx.notify();
			});
			visual.update(|window, cx| window.draw(cx).clear());
			std::thread::sleep(std::time::Duration::from_millis(250));
			visual.update(|window, cx| window.draw(cx).clear());
			let button = visual.debug_bounds("native-latest-action").unwrap();

			visual.simulate_click(button.center(), Default::default());

			let scroll = surface.read_with(visual, |s, _| {
				assert_eq!(
					s.timeline.native.entries,
					vec![row(10)],
					"keep the visible page while the read is pending"
				);
				assert!(s.timeline.native.viewport.0.borrow().latest_requested);

				s.timeline.scroll[&binding.work].clone()
			});

			if cancel {
				visual.simulate_event(gpui::ScrollWheelEvent {
					position: scroll.bounds().center(),
					delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(-40.))),
					..Default::default()
				});
			}

			surface.update(visual, |s, cx| {
				if cold {
					s.timeline.scroll.remove(&binding.work);
				}
				if summary {
					s.refresh_native_summary(
						binding.clone(),
						(100..105).map(|position| row(position).content).collect(),
					);
				} else {
					assert!(s.refresh_native_history(
						binding.clone(),
						AgentTimelinePage {
							thread_id: "thread".into(),
							entries: (100..105).map(row).collect(),
							next_cursor: None,
							weather: Default::default(),
							safety_buffering_turn_id: None,
							active_realtime_session_at_page_start: None,
						}
					));
				}

				cx.notify();
			});
			visual.update(|window, cx| {
				window.draw(cx).clear();
			});

			let scroll = surface.read_with(visual, |s, _| s.timeline.scroll[&binding.work].clone());

			assert!(scroll.max_offset().y > gpui::px(1_000.));

			let distance = (scroll.offset().y + scroll.max_offset().y).abs();

			if cancel {
				assert!(distance > gpui::px(100.));
			} else {
				assert!(distance < gpui::px(1.), "summary={summary} cold={cold}: {distance:?}");
			}
		}
	}

	#[gpui::test]
	fn background_refresh_keeps_history_height_and_reading_position(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_400.), gpui::px(700.)));

		let work = surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

			s.workspace.graph_visible = false;

			let work = s.selected.clone().unwrap();

			s.snapshot
				.as_mut()
				.unwrap()
				.work_items
				.iter_mut()
				.find(|item| item.id == work)
				.unwrap()
				.codex_thread_id = Some("thread".into());
			s.timeline.native.requested = Some((work.clone(), "thread".into()));

			assert!(s.timeline.native.replace(
				Binding { work: work.clone(), thread: "thread".into(), account: "account".into() },
				AgentTimelinePage {
					thread_id: "thread".into(),
					entries: (0..4).map(row).collect(),
					next_cursor: None,
					weather: Default::default(),
					safety_buffering_turn_id: None,
					active_realtime_session_at_page_start: None
				}
			));

			s.timeline.latest_follow_work = None;

			s.timeline.follow_paused.insert(work.clone());
			cx.notify();

			work
		});

		visual.update(|window, cx| {
			window.draw(cx).clear();
		});

		// The fixture closes the initially reserved dock. Measure refreshes only
		// after that independent panel animation has settled.
		thread::sleep(std::time::Duration::from_millis(240));

		visual.update(|window, cx| {
			window.draw(cx).clear();
		});

		let scroll = surface.read_with(visual, |s, _| s.timeline.scroll[&work].clone());

		scroll.set_offset(gpui::point(gpui::px(0.), gpui::px(-300.)));
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});

		let maximum = scroll.max_offset();
		let offset = scroll.offset();

		for pending in [true, false, true, false] {
			surface.update(visual, |s, cx| {
				s.timeline.native.task =
					pending.then(|| cx.spawn(async |_, _| future::pending::<()>().await));

				cx.notify();
			});

			visual.update(|window, cx| {
				window.draw(cx).clear();
			});

			assert_eq!(
				scroll.max_offset(),
				maximum,
				"background refresh must not insert or remove rows"
			);
			assert_eq!(
				scroll.offset(),
				offset,
				"background refresh must preserve reading position"
			);
		}
	}

	#[test]
	fn evicted_rows_and_obsolete_layout_callbacks_cannot_move_the_viewport() {
		let viewport = Viewport::default();
		let key = scroll::row_key(&row(10));

		viewport.0.borrow_mut().measure(key.clone(), 100., 300.);
		viewport.capture(-150., 400.);

		let old_revision = viewport.0.borrow_mut().measure(key.clone(), 600., 300.).unwrap();

		viewport.capture(-650., 400.);

		assert!(viewport.0.borrow_mut().finish(old_revision, 1_000.).is_none());

		let revision = viewport.0.borrow_mut().measure(key, 800., 300.).unwrap();

		assert_eq!(viewport.0.borrow_mut().finish(revision, 1_000.), Some(-850.));

		viewport.capture(-850., 400.);
		viewport.retain(&[row(1)]);

		assert!(viewport.0.borrow().pending.is_none());
		assert!(viewport.0.borrow().rows.is_empty());
	}

	#[gpui::test]
	fn trimming_newer_rows_preserves_the_visible_row_and_bounds_geometry(
		cx: &mut gpui::TestAppContext,
	) {
		let short = |position| {
			let mut entry = row(position);

			if let decodex_protocol::AgentTimelineContent::Item { text, .. } = &mut entry.content {
				*text = "Agent message.".into();
			}

			entry
		};
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_400.), gpui::px(500.)));

		let binding = surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

			s.workspace.graph_visible = false;

			let work = s
				.snapshot
				.as_mut()
				.unwrap()
				.work_items
				.iter_mut()
				.find(|work| Some(&work.id) == s.selected.as_ref())
				.unwrap();

			work.codex_thread_id = Some("thread".into());

			let binding = Binding {
				work: work.id.clone(),
				thread: "thread".into(),
				account: "account".into(),
			};

			assert!(s.timeline.native.replace(
				binding.clone(),
				AgentTimelinePage {
					thread_id: "thread".into(),
					entries: (10..1_010).map(short).collect(),
					next_cursor: Some("older".into()),
					weather: Default::default(),
					safety_buffering_turn_id: None,
					active_realtime_session_at_page_start: None,
				}
			));

			cx.notify();

			binding
		});

		visual.update(|window, cx| {
			window.draw(cx).clear();
		});

		let key = scroll::row_key(&row(11));

		surface.update(visual, |s, cx| {
			let top = s.timeline.native.viewport.0.borrow().rows[&key].0;

			s.timeline.scroll[&binding.work]
				.set_offset(gpui::point(gpui::px(0.), gpui::px(40. - top)));
			cx.notify();
		});

		visual.update(|window, cx| {
			window.draw(cx).clear();
		});

		let before = surface.update(visual, |s, cx| {
			let before = s.timeline.native.viewport.0.borrow().rows[&key].0
				+ f32::from(s.timeline.scroll[&binding.work].offset().y);

			assert!(s.prepend_native_history(
				&binding,
				"older",
				AgentTimelinePage {
					thread_id: "thread".into(),
					entries: (1..4).map(short).collect(),
					next_cursor: None,
					weather: Default::default(),
					safety_buffering_turn_id: None,
					active_realtime_session_at_page_start: None,
				}
			));
			assert!(s.timeline.native.browsing_window);

			cx.notify();

			before
		});

		for _ in 0..3 {
			visual.update(|window, cx| {
				window.draw(cx).clear();
			});
			visual.run_until_parked();
		}

		surface.read_with(visual, |s, _| {
			let geometry = s.timeline.native.viewport.0.borrow();
			let after =
				geometry.rows[&key].0 + f32::from(s.timeline.scroll[&binding.work].offset().y);

			assert!((after - before).abs() < 1., "trim moved row from {before} to {after}");
			assert_eq!(s.timeline.native.entries.len(), 1_000);
			assert_eq!(geometry.rows.len(), 1_000);
			assert!(!geometry.rows.contains_key(&scroll::row_key(&row(1_009))));
		});
	}

	#[gpui::test]
	fn older_page_preserves_visible_assistant_row_without_user_rail_marks(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_400.), gpui::px(500.)));

		let binding = surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

			s.workspace.graph_visible = false;

			let work = s
				.snapshot
				.as_mut()
				.unwrap()
				.work_items
				.iter_mut()
				.find(|work| Some(&work.id) == s.selected.as_ref())
				.unwrap();

			work.codex_thread_id = Some("thread".into());

			let binding = Binding {
				work: work.id.clone(),
				thread: "thread".into(),
				account: "account".into(),
			};

			assert!(s.timeline.native.replace(
				binding.clone(),
				AgentTimelinePage {
					thread_id: "thread".into(),
					entries: (10..13).map(row).collect(),
					next_cursor: Some("older".into()),
					weather: Default::default(),
					safety_buffering_turn_id: None,
					active_realtime_session_at_page_start: None,
				}
			));

			cx.notify();

			binding
		});

		visual.update(|window, cx| {
			window.draw(cx).clear();
		});

		let key = scroll::row_key(&row(11));

		surface.update(visual, |s, cx| {
			assert!(s.timeline.marks.is_empty());

			let top = s.timeline.native.viewport.0.borrow().rows[&key].0;

			s.timeline.scroll[&binding.work]
				.set_offset(gpui::point(gpui::px(0.), gpui::px(40. - top)));
			cx.notify();
		});

		visual.update(|window, cx| {
			window.draw(cx).clear();
		});

		let before = surface.update(visual, |s, cx| {
			let before = s.timeline.native.viewport.0.borrow().rows[&key].0
				+ f32::from(s.timeline.scroll[&binding.work].offset().y);

			assert!(s.prepend_native_history(
				&binding,
				"older",
				AgentTimelinePage {
					thread_id: "thread".into(),
					entries: (1..4).map(row).collect(),
					next_cursor: None,
					weather: Default::default(),
					safety_buffering_turn_id: None,
					active_realtime_session_at_page_start: None,
				}
			));

			cx.notify();

			before
		});

		for _ in 0..3 {
			visual.update(|window, cx| {
				window.draw(cx).clear();
			});
			visual.run_until_parked();
		}

		surface.read_with(visual, |s, _| {
			let geometry = s.timeline.native.viewport.0.borrow();
			let after =
				geometry.rows[&key].0 + f32::from(s.timeline.scroll[&binding.work].offset().y);

			assert!(
				(after - before).abs() < 1.,
				"row moved from {before} to {after}; offset={:?}, max={:?}, pending={:?}",
				s.timeline.scroll[&binding.work].offset(),
				s.timeline.scroll[&binding.work].max_offset(),
				geometry.pending.as_ref().map(|a| (&a.key, a.viewport_top, a.scheduled))
			);
			assert!(geometry.pending.is_none());
		});
	}
}
