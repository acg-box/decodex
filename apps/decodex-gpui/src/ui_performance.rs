//! Work and optional Debug activities share the top-center space, with one expanded at a time.
#[path = "ui_performance_chart.rs"] mod chart;
#[path = "performance_host.rs"] mod host;
#[path = "ui_performance_readout.rs"] mod readout;
#[cfg(test)]
#[path = "ui_performance_tests.rs"]
mod tests;
use crate::{ui_live_activity::CAPSULE_HEIGHT, ui_motion, ui_theme::TEXT};
use gpui::{
	Context, FontWeight, IntoElement, KeyDownEvent, Render, Role, Task, Window, prelude::*,
	profiler::FrameDurationSnapshot,
};
use std::{
	sync::atomic::AtomicU8,
	time::{Duration, Instant},
};

pub(crate) fn preference(value: Option<bool>) -> bool {
	static CACHE: AtomicU8 = AtomicU8::new(u8::MAX);
	crate::ui_preferences::boolean("debugMode", &CACHE, value, false)
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Expansion {
	#[default]
	Closed,
	Hover,
	Clicked,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Activity {
	#[default]
	Work,
	Debug,
}
impl Activity {
	fn header_key(self) -> &'static str {
		match self {
			Self::Work => "activity-work-header",
			Self::Debug => "activity-debug-header",
		}
	}

	fn ids(self) -> (&'static str, &'static str, &'static str, &'static str, &'static str) {
		match self {
			Self::Work => (
				"activity-work-control",
				"activity-work-island",
				"activity-island-toggle",
				"activity-work-width",
				"activity-work-height",
			),
			Self::Debug => (
				"activity-debug-control",
				"performance-island",
				"performance-island-toggle",
				"activity-debug-width",
				"activity-debug-height",
			),
		}
	}
}
impl Expansion {
	fn is_open(self) -> bool {
		self != Self::Closed
	}

	fn clicked(self) -> Self {
		if self == Self::Clicked { Self::Closed } else { Self::Clicked }
	}
}

#[derive(Default)]
struct Metrics {
	fps: f64,
	draw_ms: f64,
	submit_ms: f64,
	draw_frames: u64,
	ready_samples: u64,
}
impl Metrics {
	fn collect(
		mut current: FrameDurationSnapshot,
		previous: &FrameDurationSnapshot,
		seconds: f64,
	) -> Option<Self> {
		current.draw_duration_histogram.subtract(&previous.draw_duration_histogram).ok()?;
		current.dirty_to_present_histogram.subtract(&previous.dirty_to_present_histogram).ok()?;
		let draws = &current.draw_duration_histogram;
		Some(Self {
			fps: draws.len() as f64 / seconds.max(0.001),
			draw_ms: draws.value_at_quantile(0.95) as f64 / 1_000_000.,
			submit_ms: current.dirty_to_present_histogram.value_at_quantile(0.95) as f64
				/ 1_000_000.,
			draw_frames: draws.len(),
			ready_samples: current.dirty_to_present_histogram.len(),
		})
	}
}
fn bytes(value: Option<f64>) -> String {
	value.map_or_else(|| "—".into(), |v| format!("{:.1} GB", v / 1_000_000_000.))
}

pub(crate) struct PerformanceIsland {
	expansion: Expansion,
	activity: Activity,
	titlebar: Option<crate::shell::Titlebar>,
	task: Option<Task<()>>,
	live_clock: Option<Task<()>>,
	metrics: Metrics,
	host: host::HostSample,
	cpu_percent: Option<f64>,
	display_hz: Option<f64>,
	history: chart::History,
	work: Option<crate::ui_work_metrics::Summary>,
	available_width: Option<f32>,
}
impl PerformanceIsland {
	pub(crate) fn new(_: &mut Context<Self>) -> Self {
		Self {
			expansion: Expansion::Closed,
			activity: Activity::Work,
			titlebar: None,
			task: None,
			live_clock: None,
			metrics: Metrics::default(),
			host: host::HostSample::default(),
			cpu_percent: None,
			display_hz: None,
			history: chart::History::new(),
			work: None,
			available_width: None,
		}
	}

	pub(crate) fn with_titlebar(mut self, titlebar: crate::shell::Titlebar) -> Self {
		self.titlebar = Some(titlebar);
		self
	}

	pub(crate) fn set_work(
		&mut self,
		work: Option<crate::ui_work_metrics::Summary>,
		cx: &mut Context<Self>,
	) {
		if self.work != work {
			self.work = work;
			cx.notify();
		}
	}

	pub(crate) fn set_available_width(&mut self, width: f32, cx: &mut Context<Self>) {
		if self.available_width.is_none_or(|old| (old - width).abs() > 0.5) {
			self.available_width = Some(width);
			cx.notify();
		}
	}

	fn reconcile(&mut self, enabled: bool, window: &Window, cx: &mut Context<Self>) {
		if !enabled {
			self.task = None;
			if self.work.is_none() || self.activity == Activity::Debug {
				self.expansion = Expansion::Closed;
			}
			self.activity = Activity::Work;
			self.metrics = Metrics::default();
			self.host = host::HostSample::default();
			self.cpu_percent = None;
			self.history.clear();
			return;
		}
		self.display_hz = host::display_hz(window);
		if self.task.is_some() {
			return;
		}
		let target = window.window_handle();
		// Per-window histograms already exist in GPUI. Do not enable global
		// task traces or retain raw frame events for a long-running HUD.
		let mut previous_frames = window.frame_duration_snapshot();
		self.task = Some(cx.spawn(async move |island, cx| {
			let mut previous = Instant::now();
			loop {
				cx.background_executor().timer(Duration::from_millis(500)).await;
				let host = cx.background_executor().spawn(async { host::HostSample::read() }).await;
				let now = Instant::now();
				let seconds = now.duration_since(previous).as_secs_f64();
				previous = now;
				let Ok(frames) = target.update(cx, |_, window, _| window.frame_duration_snapshot())
				else {
					break;
				};
				if island
					.update(cx, |s, cx| {
						s.cpu_percent = host
							.cpu_seconds
							.zip(s.host.cpu_seconds)
							.map(|(next, last)| ((next - last) / seconds * 100.).max(0.));
						s.host = host;
						let metrics = Metrics::collect(frames.clone(), &previous_frames, seconds)
							.unwrap_or_default();
						s.record(metrics);
						cx.notify();
					})
					.is_err()
				{
					break;
				}
				previous_frames = frames;
			}
		}));
	}

	fn record(&mut self, metrics: Metrics) {
		// GPUI's animation histogram excludes inactive windows. Draw samples
		// remain valid with full background rendering; missing data is not zero.
		if metrics.draw_frames > 0 {
			chart::record(&mut self.history, Instant::now(), metrics.draw_ms);
		}
		self.metrics = metrics;
	}

	fn gpu_label(&self) -> String {
		self.host
			.gpu_percent
			.map_or_else(|| "System GPU —".into(), |v| format!("System GPU {v:.0}%"))
	}

	fn reconcile_live_clock(&mut self, cx: &mut Context<Self>) {
		let enabled = self.expanded(Activity::Work)
			&& self.work.as_ref().is_some_and(|work| !work.live.is_empty());
		if !enabled {
			self.live_clock = None;
		} else if self.live_clock.is_none() {
			// The clock advances once per second. Source events update actions and
			// counters; animation frames only interpolate their presentation.
			self.live_clock = Some(cx.spawn(async move |island, cx| {
				loop {
					cx.background_executor().timer(Duration::from_secs(1)).await;
					if island
						.update(cx, |_, cx| {
							if ui_motion::animations_active() {
								cx.notify();
							}
						})
						.is_err()
					{
						break;
					}
				}
			}));
		}
	}
}
impl PerformanceIsland {
	fn expanded(&self, activity: Activity) -> bool {
		self.activity == activity && self.expansion.is_open()
	}

	fn toggle(&mut self, activity: Activity) {
		self.expansion =
			if self.activity == activity { self.expansion.clicked() } else { Expansion::Clicked };
		self.activity = activity;
	}

	fn hover_pill(
		&mut self,
		activity: Activity,
		expandable: bool,
		hovered: bool,
		cx: &mut Context<Self>,
	) {
		if !expandable || self.expansion == Expansion::Clicked {
			return;
		}
		let next = if hovered {
			Expansion::Hover
		} else if self.activity == activity {
			Expansion::Closed
		} else {
			self.expansion
		};
		if hovered || self.expansion != next {
			if hovered {
				self.activity = activity;
			}
			self.expansion = next;
			cx.notify();
		}
	}

	fn pill(
		&self,
		activity: Activity,
		allocation: crate::ui_live_activity::Allocation,
		content: &readout::Content,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> gpui::AnyElement {
		let (control, surface, toggle, width_key, height_key) = activity.ids();
		let expandable = content.expandable();
		let expanded = expandable
			&& self.expanded(activity)
			&& allocation.presentation == crate::ui_live_activity::Presentation::Expanded;
		let open_height = content.height();
		let width = ui_motion::value(width_key, allocation.width, window, cx);
		let height = ui_motion::value(
			height_key,
			if expanded { open_height } else { CAPSULE_HEIGHT },
			window,
			cx,
		);
		let fraction =
			((height - CAPSULE_HEIGHT) / (open_height - CAPSULE_HEIGHT).max(1.)).clamp(0., 1.);
		// Enter after the contents fit; exit before the capsule starts clipping them.
		let details_opacity = ((fraction - 0.65) / 0.35)
			.clamp(0., 1.)
			.min(((width - (content.sizing().expanded - 24.)) / 24.).clamp(0., 1.));
		let header_key = activity.header_key();
		let header_progress = ui_motion::value(
			header_key,
			if allocation.presentation == crate::ui_live_activity::Presentation::Minimal {
				0.
			} else {
				1.
			},
			window,
			cx,
		);
		let header = content.header(
			header_progress.min(((width - (content.sizing().compact - 24.)) / 24.).clamp(0., 1.)),
		);
		let body =
			(expandable && details_opacity > 0.001).then(|| content.details(self, window, cx));
		gpui::div()
			.debug_selector(move || control.into())
			.relative()
			.w(gpui::px(width))
			.h(gpui::px(CAPSULE_HEIGHT))
			.flex_none()
			.child(
				gpui::deferred(
					gpui::div()
						.id(surface)
						.debug_selector(move || surface.into())
						.relative()
						.w_full()
						.h(gpui::px(height))
						.flex()
						.flex_col()
						.items_center()
						.overflow_hidden()
						.rounded(gpui::px(14. + fraction * 6.))
						.bg(gpui::rgb(0x141418))
						.border_1()
						.border_color(gpui::rgba(0xffffff18))
						.text_size(gpui::px(readout::TEXT_SIZE))
						.font_weight(FontWeight::NORMAL)
						.line_height(gpui::px(16.))
						.whitespace_nowrap()
						.text_color(gpui::rgb(TEXT))
						.font_features(readout::figures())
						.on_hover(cx.listener(move |s, hovered, _, cx| {
							s.hover_pill(activity, expandable, *hovered, cx)
						}))
						.when(expanded, |d| {
							d.on_mouse_down_out(cx.listener(move |s, _, _, cx| {
								if s.activity == activity {
									s.expansion = Expansion::Closed;
									cx.notify();
								}
							}))
						})
						.child(
							gpui::div()
								.id(toggle)
								.debug_selector(move || toggle.into())
								.role(if expandable { Role::Button } else { Role::Group })
								.aria_label(content.label().to_owned())
								.aria_expanded(expanded)
								.h(gpui::px(CAPSULE_HEIGHT))
								.w_full()
								.flex_none()
								.flex()
								.items_center()
								.justify_center()
								.when(expandable, |d| {
									d.tab_index(0)
										.cursor_pointer()
										.on_click(cx.listener(move |s, _, _, cx| {
											s.toggle(activity);
											cx.notify();
										}))
										.on_key_down(cx.listener(
											move |s, e: &KeyDownEvent, _, cx| {
												if ["enter", "space"]
													.contains(&e.keystroke.key.as_str())
												{
													s.toggle(activity);
												} else if e.keystroke.key == "escape" {
													s.expansion = Expansion::Closed;
												} else {
													return;
												}
												cx.notify();
												cx.stop_propagation();
											},
										))
								})
								.child(header),
						)
						.when_some(body, |d, body| {
							d.child(gpui::div().w_full().opacity(details_opacity).child(body))
						}),
				)
				.with_priority(2),
			)
			.when_some(self.titlebar.as_ref(), |d, titlebar| d.child(titlebar.control()))
			.into_any_element()
	}
}
impl Render for PerformanceIsland {
	fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let debug = preference(None);
		self.reconcile(debug, window, cx);
		self.reconcile_live_clock(cx);
		let work = self.work.clone();
		if work.is_none() {
			ui_motion::value("activity-work-width", 0., window, cx);
		}
		if !debug && work.is_none() {
			return gpui::div().into_any_element();
		}
		if work.is_none() {
			self.activity = Activity::Debug;
		}
		let mut activities = Vec::new();
		if let Some(work) = &work {
			activities.push((Activity::Work, readout::Content::work(work, window, cx)));
		}
		if debug {
			activities.push((Activity::Debug, readout::Content::debug(self, window, cx)));
		}
		let selected = self
			.expansion
			.is_open()
			.then(|| activities.iter().position(|(activity, _)| *activity == self.activity))
			.flatten();
		let sizes: Vec<_> = activities.iter().map(|(_, content)| content.sizing()).collect();
		let allocations = crate::ui_live_activity::arrange(
			self.available_width.unwrap_or(f32::from(window.viewport_size().width) / 2.),
			6.,
			&sizes,
			selected,
		);
		let mut islands = gpui::div().flex().items_start().gap(gpui::px(6.));
		for ((activity, content), allocation) in activities.iter().zip(allocations) {
			islands = islands.child(self.pill(*activity, allocation, content, window, cx));
		}
		islands.into_any_element()
	}
}

fn timing(ms: f64, samples: u64) -> String {
	if samples == 0 { "—".into() } else { format!("{ms:.1} ms") }
}
