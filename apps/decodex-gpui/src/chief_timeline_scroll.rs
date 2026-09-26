//! Preserve a visible native row after pagination, independently of user-message rail marks.
use super::{
	Binding, ChiefSurface, ChiefTimelineEntry, ChiefTimelinePage, ChiefWorkItemDto, Context, key,
};
use gpui::{AnyElement, IntoElement, ParentElement, Styled, div, point, px};
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

type RowKey = (u64, u8, String);

fn row_key(entry: &ChiefTimelineEntry) -> RowKey {
	let (position, kind, id) = key(entry);
	(position, kind, id.into())
}

#[derive(Default)]
pub(super) struct Viewport(Rc<RefCell<Geometry>>);

#[derive(Default)]
struct Geometry {
	rows: BTreeMap<RowKey, (f32, f32)>,
	pending: Option<Anchor>,
	revision: u64,
	latest_requested: bool,
}

struct Anchor {
	key: RowKey,
	viewport_top: f32,
	revision: u64,
	scheduled: bool,
}

impl Viewport {
	pub(super) fn request_latest(&self) {
		self.0.borrow_mut().latest_requested = true;
	}

	pub(super) fn take_latest_request(&self) -> bool {
		std::mem::take(&mut self.0.borrow_mut().latest_requested)
	}

	pub(super) fn retain(&self, entries: &[ChiefTimelineEntry]) {
		let keys = entries.iter().map(row_key).collect::<std::collections::BTreeSet<_>>();
		let mut state = self.0.borrow_mut();
		state.rows.retain(|key, _| keys.contains(key));
		if state.pending.as_ref().is_some_and(|anchor| !keys.contains(&anchor.key)) {
			state.pending = None;
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
		Some((pending.viewport_top - *top).clamp(-maximum.max(0.0), 0.0))
	}
}

impl ChiefSurface {
	pub(in super::super) fn cancel_native_scroll_anchor(&self) {
		let mut state = self.native_history.viewport.0.borrow_mut();
		state.pending = None;
		state.latest_requested = false;
	}

	pub(in super::super) fn prepend_native_history(
		&mut self,
		binding: &Binding,
		cursor: &str,
		page: ChiefTimelinePage,
	) -> bool {
		if self.native_history.binding.as_ref() == Some(binding)
			&& !self.native_history.show_saved
			&& self.history_navigation.is_none()
			&& let Some(scroll) = self.transcript_scroll.get(&binding.work)
		{
			self.native_history
				.viewport
				.capture(scroll.offset().y.into(), scroll.bounds().size.height.into());
		} else {
			self.cancel_native_scroll_anchor();
		}
		let accepted = self.native_history.prepend(binding, cursor, page);
		if !accepted {
			self.cancel_native_scroll_anchor();
		}
		accepted
	}

	pub(super) fn native_scroll_row(
		&self,
		work: &ChiefWorkItemDto,
		entry: &ChiefTimelineEntry,
		row: AnyElement,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let Some(scroll) = self.transcript_scroll.get(&work.id).cloned() else {
			return row;
		};
		let key = row_key(entry);
		let geometry = self.native_history.viewport.0.clone();
		let surface = cx.entity().downgrade();
		let owner = work.id.clone();
		div()
			.w_full()
			.min_w_0()
			.flex_none()
			.child(row)
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
						if !Rc::ptr_eq(&geometry, &s.native_history.viewport.0)
							|| s.selected.as_ref() != Some(&owner)
							|| s.native_history.show_saved
						{
							return;
						}
						if let Some(offset) =
							geometry.borrow_mut().finish(revision, scroll.max_offset().y.into())
						{
							scroll.set_offset(point(scroll.offset().x, px(offset)));
							cx.notify();
						}
					});
				});
			})
			.into_any_element()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use decodex_protocol::ChiefTimelineContent as Content;

	fn row(position: u64) -> ChiefTimelineEntry {
		ChiefTimelineEntry {
			position,
			content: Content::Item {
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

	#[gpui::test]
	fn latest_arrival_scrolls_new_layout_but_respects_a_later_wheel_gesture(
		cx: &mut gpui::TestAppContext,
	) {
		for (cancel, cold) in [(false, false), (true, false), (false, true)] {
			let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
			visual.simulate_resize(gpui::size(px(1400.), px(500.)));
			let binding = surface.update(visual, |s, cx| {
				s.visual_workspace_fixture(cx);
				s.graph_visible = false;
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
				assert!(s.native_history.replace(
					binding.clone(),
					ChiefTimelinePage {
						thread_id: "thread".into(),
						entries: vec![row(10)],
						next_cursor: None,
						active_realtime_session_at_page_start: None,
					}
				));
				cx.notify();
				binding
			});
			visual.update(|window, cx| {
				window.draw(cx).clear();
			});
			let button = visual.debug_bounds("native-latest-action").unwrap();
			visual.simulate_click(button.center(), Default::default());
			let scroll = surface.read_with(visual, |s, _| {
				assert_eq!(
					s.native_history.entries,
					vec![row(10)],
					"keep the visible page while the read is pending"
				);
				assert!(s.native_history.viewport.0.borrow().latest_requested);
				s.transcript_scroll[&binding.work].clone()
			});
			if cancel {
				visual.simulate_event(gpui::ScrollWheelEvent {
					position: scroll.bounds().center(),
					delta: gpui::ScrollDelta::Pixels(point(px(0.), px(-40.))),
					..Default::default()
				});
			}
			surface.update(visual, |s, cx| {
				if cold {
					s.transcript_scroll.remove(&binding.work);
				}
				assert!(s.refresh_native_history(
					binding.clone(),
					ChiefTimelinePage {
						thread_id: "thread".into(),
						entries: (100..105).map(row).collect(),
						next_cursor: None,
						active_realtime_session_at_page_start: None,
					}
				));
				cx.notify();
			});
			visual.update(|window, cx| {
				window.draw(cx).clear();
			});
			let scroll =
				surface.read_with(visual, |s, _| s.transcript_scroll[&binding.work].clone());
			assert!(scroll.max_offset().y > px(1000.));
			let distance = (scroll.offset().y + scroll.max_offset().y).abs();
			if cancel {
				assert!(distance > px(100.));
			} else {
				assert!(distance < px(1.));
			}
		}
	}

	#[test]
	fn evicted_rows_and_obsolete_layout_callbacks_cannot_move_the_viewport() {
		let viewport = Viewport::default();
		let key = row_key(&row(10));
		viewport.0.borrow_mut().measure(key.clone(), 100., 300.);
		viewport.capture(-150., 400.);
		let old_revision = viewport.0.borrow_mut().measure(key.clone(), 600., 300.).unwrap();
		viewport.capture(-650., 400.);
		assert!(viewport.0.borrow_mut().finish(old_revision, 1000.).is_none());
		let revision = viewport.0.borrow_mut().measure(key, 800., 300.).unwrap();
		assert_eq!(viewport.0.borrow_mut().finish(revision, 1000.), Some(-850.));
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
			if let Content::Item { text, .. } = &mut entry.content {
				*text = "Agent message.".into();
			}
			entry
		};
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		visual.simulate_resize(gpui::size(px(1400.), px(500.)));
		let binding = surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.graph_visible = false;
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
			assert!(s.native_history.replace(
				binding.clone(),
				ChiefTimelinePage {
					thread_id: "thread".into(),
					entries: (10..1010).map(short).collect(),
					next_cursor: Some("older".into()),
					active_realtime_session_at_page_start: None,
				}
			));
			cx.notify();
			binding
		});
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});
		let key = row_key(&row(11));
		surface.update(visual, |s, cx| {
			let top = s.native_history.viewport.0.borrow().rows[&key].0;
			s.transcript_scroll[&binding.work].set_offset(point(px(0.), px(40. - top)));
			cx.notify();
		});
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});
		let before = surface.update(visual, |s, cx| {
			let before = s.native_history.viewport.0.borrow().rows[&key].0
				+ f32::from(s.transcript_scroll[&binding.work].offset().y);
			assert!(s.prepend_native_history(
				&binding,
				"older",
				ChiefTimelinePage {
					thread_id: "thread".into(),
					entries: (1..4).map(short).collect(),
					next_cursor: None,
					active_realtime_session_at_page_start: None,
				}
			));
			assert!(s.native_history.browsing_window);
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
			let geometry = s.native_history.viewport.0.borrow();
			let after =
				geometry.rows[&key].0 + f32::from(s.transcript_scroll[&binding.work].offset().y);
			assert!((after - before).abs() < 1., "trim moved row from {before} to {after}");
			assert_eq!(s.native_history.entries.len(), 1000);
			assert_eq!(geometry.rows.len(), 1000);
			assert!(!geometry.rows.contains_key(&row_key(&row(1009))));
		});
	}

	#[gpui::test]
	fn older_page_preserves_visible_assistant_row_without_user_rail_marks(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		visual.simulate_resize(gpui::size(px(1400.), px(500.)));
		let binding = surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.graph_visible = false;
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
			assert!(s.native_history.replace(
				binding.clone(),
				ChiefTimelinePage {
					thread_id: "thread".into(),
					entries: (10..13).map(row).collect(),
					next_cursor: Some("older".into()),
					active_realtime_session_at_page_start: None,
				}
			));
			cx.notify();
			binding
		});
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});
		let key = row_key(&row(11));
		surface.update(visual, |s, cx| {
			assert!(s.history_marks.is_empty());
			let top = s.native_history.viewport.0.borrow().rows[&key].0;
			s.transcript_scroll[&binding.work].set_offset(point(px(0.), px(40. - top)));
			cx.notify();
		});
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});
		let before = surface.update(visual, |s, cx| {
			let before = s.native_history.viewport.0.borrow().rows[&key].0
				+ f32::from(s.transcript_scroll[&binding.work].offset().y);
			assert!(s.prepend_native_history(
				&binding,
				"older",
				ChiefTimelinePage {
					thread_id: "thread".into(),
					entries: (1..4).map(row).collect(),
					next_cursor: None,
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
			let geometry = s.native_history.viewport.0.borrow();
			let after =
				geometry.rows[&key].0 + f32::from(s.transcript_scroll[&binding.work].offset().y);
			assert!(
				(after - before).abs() < 1.,
				"row moved from {before} to {after}; offset={:?}, max={:?}, pending={:?}",
				s.transcript_scroll[&binding.work].offset(),
				s.transcript_scroll[&binding.work].max_offset(),
				geometry.pending.as_ref().map(|a| (&a.key, a.viewport_top, a.scheduled))
			);
			assert!(geometry.pending.is_none());
		});
	}
}
