//! Inline time range. Dragging previews the range; releasing applies one query.
use super::{Panel, TimeRange};
use crate::{ui_motion, ui_theme};
use decodex_protocol::AgentDeliveryExtent;
use gpui::{Context, IntoElement, KeyDownEvent, MouseButton, prelude::*};

const MINUTE: i64 = 60_000_000;
const MINIMUM_SPAN: i64 = 14 * 24 * 60;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum Period {
	Today,
	Week,
	Fortnight,
	#[default]
	All,
}
impl Period {
	pub fn label(self) -> &'static str {
		match self {
			Self::Today => "Today",
			Self::Week => "7d",
			Self::Fortnight => "14d",
			Self::All => "All time",
		}
	}
}

pub(super) struct Slider {
	domain: [i64; 2],
	selected: [i64; 2],
	unbounded: [bool; 2],
	open_start: bool,
	drag: Option<usize>,
	grab: f32,
	pointer: Option<f32>,
	bounds: Option<gpui::Bounds<gpui::Pixels>>,
}
impl Slider {
	pub fn new(extent: Option<AgentDeliveryExtent>, range: TimeRange, period: Period) -> Self {
		let now = std::time::SystemTime::now()
			.duration_since(std::time::UNIX_EPOCH)
			.unwrap_or_default()
			.as_secs() as i64
			/ 60;
		Self::in_period(extent, range, now, period)
	}

	#[cfg(test)]
	fn at(extent: Option<AgentDeliveryExtent>, range: TimeRange, now: i64) -> Self {
		Self::in_period(extent, range, now, Period::All)
	}

	fn in_period(
		extent: Option<AgentDeliveryExtent>,
		range: TimeRange,
		now: i64,
		period: Period,
	) -> Self {
		let last = extent.map_or(now, |e| e.last.div_euclid(MINUTE).max(now));
		let first = match period {
			Period::Today => super::super::day_start(last * MINUTE, 0) / MINUTE,
			Period::Week => last - 7 * 24 * 60,
			Period::Fortnight => last - MINIMUM_SPAN,
			Period::All => extent.map_or(last - MINIMUM_SPAN, |e| {
				e.first.div_euclid(MINUTE).min(last - MINIMUM_SPAN)
			}),
		};
		let selected = [
			range.after.map_or(first, |v| v.div_euclid(MINUTE)),
			range.before.map_or(last, |v| (v - 1).div_euclid(MINUTE)),
		];
		let domain = [first.min(selected[0]), last.max(selected[1])];
		Self {
			domain,
			selected,
			unbounded: [period == Period::All && range.after.is_none(), range.before.is_none()],
			open_start: period == Period::All,
			drag: None,
			grab: 0.,
			pointer: None,
			bounds: None,
		}
	}

	pub fn dragging(&self) -> bool {
		self.drag.is_some()
	}

	fn fraction(&self, end: usize) -> f32 {
		if self.drag == Some(end)
			&& let Some(pointer) = self.pointer
		{
			return pointer;
		}
		if self.domain[0] == self.domain[1] {
			return end as f32;
		}
		(self.selected[end] - self.domain[0]) as f32 / (self.domain[1] - self.domain[0]) as f32
	}

	pub(super) fn range(&self) -> TimeRange {
		TimeRange {
			after: (!self.unbounded[0]).then_some(self.selected[0] * MINUTE),
			before: (!self.unbounded[1]).then_some((self.selected[1] + 1) * MINUTE),
		}
	}

	pub(super) fn covers_domain(&self) -> bool {
		self.selected == self.domain
	}

	pub fn clock_label(&self) -> String {
		if self.range() == TimeRange::default() {
			"All time".into()
		} else {
			let minutes = self.selected[1] - self.selected[0] + 1;
			if minutes >= 24 * 60 {
				let hours = minutes % (24 * 60) / 60;
				if hours == 0 {
					format!("{}d", minutes / (24 * 60))
				} else {
					format!("{}d {hours}h", minutes / (24 * 60))
				}
			} else if minutes >= 60 {
				if minutes % 60 == 0 {
					format!("{}h", minutes / 60)
				} else {
					format!("{}h {}m", minutes / 60, minutes % 60)
				}
			} else {
				format!("{minutes}m")
			}
		}
	}

	fn set(&mut self, end: usize, value: i64) {
		let (first, last) = if end == 0 {
			(self.domain[0], self.selected[1])
		} else {
			(self.selected[0], self.domain[1])
		};
		self.selected[end] = value.clamp(first, last);
		self.unbounded[end] =
			self.selected[end] == self.domain[end] && (end == 1 || self.open_start);
	}

	fn begin(&mut self, x: f32) {
		let Some(bounds) = self.bounds else { return };
		let fraction = (x - f32::from(bounds.left())) / f32::from(bounds.size.width).max(1.);
		let end =
			usize::from((fraction - self.fraction(1)).abs() < (fraction - self.fraction(0)).abs());
		let offset = (fraction - self.fraction(end)) * f32::from(bounds.size.width);
		self.drag = Some(end);
		self.grab = if offset.abs() <= 8. { offset } else { 0. };
		if offset.abs() > 8. {
			self.move_to(x);
		}
	}

	fn move_to(&mut self, x: f32) {
		let (Some(mut end), Some(bounds)) = (self.drag, self.bounds) else { return };
		let fraction = ((x - self.grab - f32::from(bounds.left()))
			/ f32::from(bounds.size.width).max(1.))
		.clamp(0., 1.);
		let value =
			self.domain[0] as f64 + fraction as f64 * (self.domain[1] - self.domain[0]) as f64;
		if self.selected[0] == self.selected[1] {
			end = usize::from(value > self.selected[0] as f64);
			self.drag = Some(end);
		}
		self.set(end, value.round() as i64);
		let other = self.fraction(1 - end);
		self.pointer = Some(if end == 0 { fraction.min(other) } else { fraction.max(other) });
	}

	fn release(&mut self) {
		self.drag = None;
		self.pointer = None;
	}

	fn key(&mut self, end: usize, e: &KeyDownEvent) -> bool {
		let step = if e.keystroke.modifiers.shift { 60 } else { 1 };
		let next = match e.keystroke.key.as_str() {
			"left" | "down" => self.selected[end] - step,
			"right" | "up" => self.selected[end] + step,
			"home" => self.domain[0],
			"end" => self.domain[1],
			_ => return false,
		};
		self.set(end, next);
		true
	}
}

impl Panel {
	pub(super) fn time_slider(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
		let p = self.filter.slider.as_ref();
		let mut track = gpui::div()
			.id("delivery-time-track")
			.debug_selector(|| "delivery-time-track".into())
			.role(gpui::Role::Group)
			.aria_label("Time range slider")
			.h(gpui::px(22.))
			.relative()
			.when(p.is_some(), |d| d.cursor_pointer())
			.on_mouse_down(
				MouseButton::Left,
				cx.listener(|s, e: &gpui::MouseDownEvent, window, cx| {
					if let Some(p) = &mut s.filter.slider {
						p.begin(f32::from(e.position.x));
						window.focus(
							&s.filter.time_focus.as_ref().expect("time focus")[p.drag.unwrap_or(0)],
							cx,
						);
						cx.stop_propagation();
						cx.notify();
					}
				}),
			)
			.child(self.time_drag_events(cx))
			.child(Track {
				from: p.map_or(0., |p| p.fraction(0)),
				until: p.map_or(1., |p| p.fraction(1)),
				dragging: p.is_some_and(Slider::dragging),
				enabled: p.is_some(),
			});
		if let Some(p) = p {
			for end in 0..2 {
				track = track.child(self.time_thumb(end, p, cx));
			}
		}
		gpui::div()
			.flex()
			.flex_col()
			.child(track)
			.child(
				gpui::div()
					.px(gpui::px(6.))
					.h(gpui::px(if self.width >= 220. { 16. } else { 28. }))
					.flex()
					.justify_between()
					.text_size(gpui::px(10.))
					.line_height(gpui::px(13.))
					.text_color(crate::ui_theme::rgb(ui_theme::TEXT_MUTED))
					.children((0..2).map(|end| {
						gpui::div()
							.flex()
							.flex_col()
							.when(end == 1, |d| d.items_end())
							.when(p.is_some_and(|p| p.drag == Some(end)), |d| {
								d.text_color(crate::ui_theme::rgb(ui_theme::BLUE))
							})
							.child(p.map_or_else(
								|| "—".into(),
								|p| stamp(p.selected[end], self.width >= 220.),
							))
							.when(self.width < 220., |d| {
								d.child(p.map_or_else(
									|| "—".into(),
									|p| super::super::clock(p.selected[end] * MINUTE),
								))
							})
					})),
			)
			.into_any_element()
	}

	fn time_drag_events(&self, cx: &Context<Self>) -> gpui::AnyElement {
		let owner = cx.entity().downgrade();
		let events = owner.clone();
		gpui::canvas(
			move |bounds, _, cx| {
				let _ = owner.update(cx, |s, _| {
					if let Some(p) = &mut s.filter.slider {
						p.bounds = Some(bounds);
					}
				});
			},
			move |_, _, window, _| {
				let movement = events.clone();
				window.on_mouse_event(move |e: &gpui::MouseMoveEvent, phase, _, cx| {
					if phase != gpui::DispatchPhase::Capture {
						return;
					}
					let _ = movement.update(cx, |s, cx| {
						if let Some(p) = &mut s.filter.slider
							&& p.dragging()
						{
							if e.pressed_button == Some(MouseButton::Left) {
								p.move_to(f32::from(e.position.x));
								cx.notify();
							} else {
								s.commit_time(cx);
							}
							cx.stop_propagation();
						}
					});
				});
				let release = events.clone();
				window.on_mouse_event(move |_: &gpui::MouseUpEvent, phase, _, cx| {
					if phase == gpui::DispatchPhase::Capture {
						let _ = release.update(cx, |s, cx| {
							if s.filter.slider.as_ref().is_some_and(Slider::dragging) {
								s.commit_time(cx);
							}
						});
					}
				});
			},
		)
		.absolute()
		.top_0()
		.bottom_0()
		.left(gpui::px(6.))
		.right(gpui::px(6.))
		.into_any_element()
	}

	fn time_thumb(&self, end: usize, p: &Slider, cx: &mut Context<Self>) -> gpui::AnyElement {
		let focus = self.filter.time_focus.as_ref().expect("time focus")[end].clone();
		let clicked_focus = focus.clone();
		gpui::div()
			.id(if end == 0 { "delivery-from" } else { "delivery-until" })
			.debug_selector(
				move || if end == 0 { "delivery-from".into() } else { "delivery-until".into() },
			)
			.role(gpui::Role::Slider)
			.aria_label(if end == 0 { "Start time" } else { "End time" })
			.aria_value(stamp(p.selected[end], true))
			.track_focus(&focus)
			.tab_index(0)
			.absolute()
			.left(gpui::relative(p.fraction(end)))
			.ml(gpui::px(-2. - 12. * p.fraction(end)))
			.w(gpui::px(16.))
			.h_full()
			.cursor(gpui::CursorStyle::ResizeLeftRight)
			.on_click(cx.listener(move |_, _, window, cx| {
				window.focus(&clicked_focus, cx);
			}))
			.on_key_down(cx.listener(move |s, e: &KeyDownEvent, _, cx| {
				if let Some(p) = &mut s.filter.slider
					&& p.key(end, e)
				{
					cx.stop_propagation();
					cx.notify();
				}
			}))
			.on_key_up(cx.listener(|s, e: &gpui::KeyUpEvent, _, cx| {
				if ["left", "right", "up", "down", "home", "end"]
					.contains(&e.keystroke.key.as_str())
				{
					s.commit_time(cx);
					cx.stop_propagation();
				}
			}))
			.into_any_element()
	}

	fn commit_time(&mut self, cx: &mut Context<Self>) {
		let Some(p) = &mut self.filter.slider else { return };
		p.release();
		let range = p.range();
		if range != self.filter.range {
			self.filter.range = range;
			self.reload(cx);
		} else {
			cx.notify();
		}
	}
}
#[derive(gpui::IntoElement)]
struct Track {
	from: f32,
	until: f32,
	dragging: bool,
	enabled: bool,
}
impl gpui::RenderOnce for Track {
	fn render(self, window: &mut gpui::Window, cx: &mut gpui::App) -> impl IntoElement {
		let from =
			ui_motion::direct_value("delivery-range-from", self.from, self.dragging, window, cx);
		let until =
			ui_motion::direct_value("delivery-range-until", self.until, self.dragging, window, cx);
		let mut d = gpui::div()
			.absolute()
			.top_0()
			.bottom_0()
			.left(gpui::px(6.))
			.right(gpui::px(6.))
			.opacity(if self.enabled { 1. } else { 0.25 })
			.child(
				gpui::div()
					.absolute()
					.top(gpui::px(9.5))
					.h(gpui::px(3.))
					.w_full()
					.rounded_full()
					.bg(crate::ui_theme::rgba(0xffffff20)),
			)
			.child(
				gpui::div()
					.absolute()
					.top(gpui::px(9.5))
					.h(gpui::px(3.))
					.left(gpui::relative(from))
					.w(gpui::relative((until - from).max(0.)))
					.rounded_full()
					.bg(crate::ui_theme::rgb(ui_theme::BLUE)),
			);
		for fraction in [from, until] {
			d = d.child(
				gpui::div()
					.absolute()
					.left(gpui::relative(fraction))
					.ml(gpui::px(-5.))
					.top(gpui::px(3.))
					.w(gpui::px(10.))
					.h(gpui::px(16.))
					.rounded(gpui::px(4.))
					.bg(crate::ui_theme::rgb(0xc6cce0))
					.flex()
					.items_center()
					.justify_center()
					.child(
						gpui::div()
							.w(gpui::px(1.))
							.h(gpui::px(6.))
							.rounded_full()
							.bg(crate::ui_theme::rgba(0x17171c70)),
					),
			);
		}
		d
	}
}
fn stamp(minute: i64, with_time: bool) -> String {
	let d = super::super::local(minute * MINUTE);
	let date = format!("{:04}-{:02}-{:02}", d.tm_year + 1900, d.tm_mon + 1, d.tm_mday);
	if with_time { format!("{date} {:02}:{:02}", d.tm_hour, d.tm_min) } else { date }
}

#[cfg(test)]
mod tests {
	use super::*;
	fn extent() -> AgentDeliveryExtent {
		AgentDeliveryExtent { first: 0, last: 120 * MINUTE }
	}
	#[test]
	fn range_clamps_and_covers_the_whole_last_minute() {
		let range = TimeRange { after: Some(30 * MINUTE), before: Some(41 * MINUTE) };
		assert_eq!(Slider::at(Some(extent()), range, 120).range(), range);
		let mut p = Slider::at(Some(extent()), TimeRange::default(), 120);
		p.set(0, 30);
		p.set(1, 40);
		assert_eq!(p.range(), range);
		assert_eq!(p.clock_label(), "11m");
		p.set(0, 200);
		assert_eq!(p.selected, [40, 40]);
		p.set(0, p.domain[0]);
		p.set(1, 200);
		assert_eq!(p.range(), TimeRange::default());
		p.set(0, 40);
		p.set(1, 40);
		p.bounds = Some(gpui::Bounds::new(
			gpui::point(gpui::px(0.), gpui::px(0.)),
			gpui::size(gpui::px(MINIMUM_SPAN as f32), gpui::px(22.)),
		));
		let x = |minute: i64| (minute - (120 - MINIMUM_SPAN)) as f32;
		p.begin(x(40));
		p.move_to(x(42));
		assert_eq!(p.selected, [40, 42]);
		p.release();
		p.set(1, 40);
		p.begin(x(40));
		p.move_to(x(38));
		assert_eq!(p.selected, [38, 40]);
		p.release();
		p.set(0, 30);
		p.begin(x(32));
		assert_eq!(p.selected[0], 30);
		p.move_to(x(33));
		assert_eq!(p.selected[0], 31);
	}
	#[test]
	fn sparse_and_empty_history_keep_a_fortnight_and_preserve_older_history() {
		let now = 100_000;
		for extent in [
			None,
			Some(AgentDeliveryExtent { first: (now - 2) * MINUTE, last: (now - 1) * MINUTE }),
		] {
			let p = Slider::at(extent, TimeRange::default(), now);
			assert_eq!(p.domain, [now - 14 * 24 * 60, now]);
			assert_eq!(p.selected, p.domain);
			assert_eq!(p.range(), TimeRange::default());
		}
		let p = Slider::at(
			Some(AgentDeliveryExtent { first: (now - 60 * 24 * 60) * MINUTE, last: now * MINUTE }),
			TimeRange::default(),
			now,
		);
		assert_eq!(p.domain[0], now - 60 * 24 * 60);
	}
	#[test]
	fn time_presets_zoom_the_track_and_keep_the_start_bounded() {
		let now = 30_000_600;
		for (period, minutes) in [(Period::Week, 7 * 24 * 60), (Period::Fortnight, MINIMUM_SPAN)] {
			let p = Slider::in_period(Some(extent()), TimeRange::default(), now, period);
			assert_eq!(p.domain, [now - minutes, now]);
			assert_eq!(p.range().after, Some((now - minutes) * MINUTE));
			assert!(p.range().before.is_none());
		}
		let mut p = Slider::in_period(Some(extent()), TimeRange::default(), now, Period::Today);
		assert!((0..24 * 60).contains(&(p.domain[1] - p.domain[0])));
		assert!(p.covers_domain());
		p.set(0, p.domain[0] + 1);
		p.set(0, p.domain[0]);
		assert_eq!(p.range().after, Some(p.domain[0] * MINUTE));
		assert!(p.covers_domain());
	}
	#[gpui::test]
	fn inline_slider_applies_only_on_release_and_keeps_its_space(cx: &mut gpui::TestAppContext) {
		let (panel, visual) = cx.add_window_view(|_, _| Panel::default());
		panel.update(visual, |p, cx| p.set_width(160., cx));
		visual.simulate_resize(gpui::size(gpui::px(160.), gpui::px(700.)));
		visual.update(|w, cx| w.draw(cx).clear(cx));
		let empty = visual.debug_bounds("delivery-time-track").unwrap();
		panel.update(visual, |p, cx| {
			p.filter.receive_extent(Some(extent()));
			p.filter.slider = Some(Slider::at(Some(extent()), TimeRange::default(), 120));
			p.feed.receive(vec![], false);
			cx.notify();
		});
		visual.update(|w, cx| w.draw(cx).clear(cx));
		assert_eq!(visual.debug_bounds("delivery-time-track").unwrap(), empty);
		let bounds =
			panel.read_with(visual, |p, _| p.filter.slider.as_ref().unwrap().bounds.unwrap());
		let start = gpui::point(bounds.left(), bounds.center().y);
		let end = gpui::point(bounds.left() + bounds.size.width * 0.25, bounds.center().y);
		visual.simulate_mouse_down(start, MouseButton::Left, Default::default());
		visual.simulate_mouse_move(end, MouseButton::Left, Default::default());
		panel.read_with(visual, |p, _| {
			assert_eq!(p.filter.range, TimeRange::default());
			assert_eq!(p.filter.slider.as_ref().unwrap().selected[0], 120 - MINIMUM_SPAN * 3 / 4);
		});
		visual.simulate_mouse_up(end, MouseButton::Left, Default::default());
		panel.read_with(visual, |p, _| {
			assert_eq!(p.filter.range.after, Some((120 - MINIMUM_SPAN * 3 / 4) * MINUTE));
			assert!(p.feed.loaded);
		});
		visual.update(|w, cx| w.draw(cx).clear(cx));
		visual.simulate_keystrokes("right");
		panel.read_with(visual, |p, _| {
			assert_eq!(p.filter.range.after, Some((120 - MINIMUM_SPAN * 3 / 4) * MINUTE))
		});
		visual.simulate_event(gpui::KeyUpEvent {
			keystroke: gpui::Keystroke::parse("right").unwrap(),
		});
		panel.read_with(visual, |p, _| {
			assert_eq!(p.filter.range.after, Some((121 - MINIMUM_SPAN * 3 / 4) * MINUTE))
		});
		visual.update(|w, cx| w.draw(cx).clear(cx));
		let start = gpui::point(bounds.right() + gpui::px(3.), bounds.center().y);
		let end = gpui::point(bounds.left() + bounds.size.width * 0.75, bounds.center().y);
		visual.simulate_mouse_down(start, MouseButton::Left, Default::default());
		visual.simulate_mouse_move(end, MouseButton::Left, Default::default());
		visual.simulate_mouse_up(end, MouseButton::Left, Default::default());
		panel.read_with(visual, |p, _| assert!(p.filter.range.before.is_some()));
		visual.update(|w, cx| w.draw(cx).clear(cx));
		let reset = visual.debug_bounds("delivery-time-reset").unwrap().center();
		visual.simulate_click(reset, Default::default());
		panel.read_with(visual, |p, _| assert_eq!(p.filter.range, TimeRange::default()));
		visual.update(crate::ui_frame::draw_frame);
		let point = visual.debug_bounds("delivery-time-filter").unwrap().center();
		visual.simulate_click(point, Default::default());
		visual.update(crate::ui_frame::draw_frame);
		let point = visual.debug_bounds("delivery-period-Today").unwrap().center();
		visual.simulate_click(point, Default::default());
		panel.read_with(visual, |p, _| {
			assert_eq!(p.filter.period, Period::Today);
			assert!(p.filter.range.after.is_some());
			assert!(!p.filter.time_menu);
			let s = p.filter.slider.as_ref().unwrap();
			assert!(s.domain[1] - s.domain[0] < 24 * 60);
		});
		visual.update(crate::ui_frame::draw_frame);
		let point = visual.debug_bounds("delivery-time-reset").unwrap().center();
		visual.simulate_click(point, Default::default());
		panel.read_with(visual, |p, _| {
			assert_eq!(p.filter.range, TimeRange::default());
			assert_eq!(p.filter.period, Period::All);
		});
	}
}
