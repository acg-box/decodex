//! Work and optional Debug activities share the top-center space, with one expanded at a time.
#[path = "ui_performance_chart.rs"] mod chart;
#[path = "performance_host.rs"] mod host;
#[path = "ui_performance_readout.rs"] mod readout;

use std::{
	rc::Rc,
	sync::atomic::AtomicU8,
	time::{Duration, Instant},
};

use gpui::{
	AnyElement, Context, FontWeight, IntoElement, KeyDownEvent, MouseMoveEvent, Render, Role, Task,
	Window,
	prelude::{
		FluentBuilder, InteractiveElement, ParentElement, StatefulInteractiveElement, Styled,
	},
	profiler::FrameDurationSnapshot,
};

use crate::{
	shell::Titlebar,
	ui_live_activity::{self, Allocation, CAPSULE_HEIGHT, Extent, ISLAND_GAP, Morph, Presentation},
	ui_motion, ui_preferences,
	ui_theme::{self, TEXT},
	ui_work_metrics::Summary,
};
use readout::TEXT_SIZE;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Expansion {
	#[default]
	Closed,
	Hover,
	Clicked,
}
impl Expansion {
	fn is_open(self) -> bool {
		self != Self::Closed
	}

	fn clicked(self) -> Self {
		if self == Self::Clicked { Self::Closed } else { Self::Clicked }
	}
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum Activity {
	#[default]
	Work,
	Debug,
}
impl Activity {
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

#[derive(Clone, Copy)]
struct PillFrame {
	activity: Activity,
	width: f32,
	height: f32,
	expanded: bool,
	fraction: f32,
	details_opacity: f32,
	canvas: Extent,
	clip_x: f32,
}

pub(crate) struct PerformanceIsland {
	expansion: Expansion,
	activity: Activity,
	hover_intent: ui_motion::HoverIntent<Activity>,
	titlebar: Option<Titlebar>,
	task: Option<Task<()>>,
	live_clock: Option<Task<()>>,
	metrics: Metrics,
	host: host::HostSample,
	cpu_percent: Option<f64>,
	display_hz: Option<f64>,
	history: chart::History,
	work: Option<Summary>,
	available_width: Option<f32>,
	morphs: [Morph; 2],
}
impl PerformanceIsland {
	#[cfg(feature = "visual-capture")]
	#[allow(dead_code)] // Used only by the isolated native frame qualification binary.
	pub(crate) fn visual_expansion(&mut self, debug: bool, open: bool, cx: &mut Context<Self>) {
		self.hover_intent.cancel();
		self.activity = if debug { Activity::Debug } else { Activity::Work };
		self.expansion = if open { Expansion::Clicked } else { Expansion::Closed };
		cx.notify();
	}

	pub(crate) fn new(_: &mut Context<Self>) -> Self {
		Self {
			expansion: Expansion::Closed,
			activity: Activity::Work,
			hover_intent: Default::default(),
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
			morphs: Default::default(),
		}
	}

	pub(crate) fn with_titlebar(mut self, titlebar: Titlebar) -> Self {
		self.titlebar = Some(titlebar);

		self
	}

	pub(crate) fn set_work(&mut self, work: Option<Summary>, cx: &mut Context<Self>) {
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
				self.close();
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
		ui_motion::begin_interaction();
		self.hover_intent.cancel();
		self.expansion =
			if self.activity == activity { self.expansion.clicked() } else { Expansion::Clicked };
		self.activity = activity;
	}

	fn close(&mut self) {
		if self.expansion.is_open() {
			ui_motion::begin_interaction();
		}
		self.hover_intent.cancel();
		self.expansion = Expansion::Closed;
	}

	fn hover_pill(
		&mut self,
		activity: Activity,
		expandable: bool,
		hovered: bool,
		cx: &mut Context<Self>,
	) {
		if !expandable {
			if self.hover_intent.target() == Some(&activity) {
				self.hover_intent.cancel();
			}
			return;
		}
		if self.expansion == Expansion::Clicked {
			return;
		}

		let current = (self.expansion == Expansion::Hover).then_some(self.activity);
		self.hover_intent.update(activity, hovered, current, cx, |island, next, cx| {
			island.hover_intent.settled();
			if let Some(activity) = next {
				island.activity = activity;
				island.expansion = Expansion::Hover;
			} else {
				island.expansion = Expansion::Closed;
			}
			cx.notify();
		});
	}

	fn pill(
		&self,
		activity: Activity,
		allocation: Allocation,
		placement: (Extent, bool),
		content: &Rc<readout::Content>,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let (control, surface, _, _, _) = activity.ids();
		let expanded = content.expandable()
			&& self.expanded(activity)
			&& allocation.presentation == Presentation::Expanded;
		let open_height = content.height();
		let (Extent { width, height }, paired) = placement;
		let fraction =
			((height - CAPSULE_HEIGHT) / (open_height - CAPSULE_HEIGHT).max(1.)).clamp(0., 1.);
		let details_opacity = ((fraction - 0.55) / 0.45).clamp(0., 1.);
		let canvas =
			Extent { width: content.sizing().expanded.max(width), height: open_height.max(height) };
		let clip_x = (canvas.width - width)
			* if paired { if activity == Activity::Work { 1. } else { 0. } } else { 0.5 };
		let frame = PillFrame {
			activity,
			width,
			height,
			expanded,
			fraction,
			details_opacity,
			canvas,
			clip_x,
		};
		let owner = cx.entity().downgrade();
		let dismiss = owner.clone();
		let content = content.clone();
		gpui::div()
			.debug_selector(move || control.into())
			.relative()
			.w(gpui::px(width))
			.h(gpui::px(CAPSULE_HEIGHT))
			.flex_none()
			.child(
				gpui::deferred(
					gpui::div()
						.absolute()
						.left(gpui::px(-clip_x))
						.top_0()
						.debug_selector(move || format!("activity-canvas-{activity:?}"))
						.w(gpui::px(canvas.width))
						.h(gpui::px(canvas.height))
						.child(
							ui_motion::material_surface(
								surface,
								f64::from(
									CAPSULE_HEIGHT / 2.
										+ fraction
											* (ui_live_activity::EXPANDED_RADIUS
												- CAPSULE_HEIGHT / 2.),
								),
								move |window, cx| {
									owner
										.update(cx, |island, cx| {
											island.pill_surface(frame, &content, window, cx)
										})
										.unwrap_or_else(|_| gpui::div().into_any_element())
								},
								move |cx| {
									let _ = dismiss.update(cx, |island, cx| {
										if island.hover_intent.target() == Some(&activity) {
											island.hover_intent.cancel();
										}
										if island.activity == activity && island.expansion.is_open()
										{
											island.close();
											cx.notify();
										}
									});
								},
							)
							.clip(gpui::Bounds::new(
								gpui::point(gpui::px(clip_x), gpui::px(0.)),
								gpui::size(gpui::px(width), gpui::px(height)),
							)),
						),
				)
				.with_priority(2),
			)
			.when_some(self.titlebar.as_ref(), |d, titlebar| d.child(titlebar.control()))
			.into_any_element()
	}

	fn pill_surface(
		&self,
		frame: PillFrame,
		content: &readout::Content,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let PillFrame {
			activity,
			width,
			height,
			expanded,
			fraction,
			details_opacity,
			canvas,
			clip_x,
		} = frame;
		let (_, surface, toggle, _, _) = activity.ids();
		let expandable = content.expandable();
		let header = content.header(
			((width - content.sizing().minimal)
				/ (content.sizing().compact - content.sizing().minimal).max(1.))
			.clamp(0., 1.),
			width - 2.,
		);
		let body =
			(expandable && details_opacity > 0.001).then(|| content.details(self, window, cx));
		let surface = gpui::div()
			.id(surface)
			.debug_selector(move || surface.into())
			.absolute()
			.left(gpui::px(clip_x))
			.top_0()
			.w(gpui::px(width))
			.h(gpui::px(height))
			.flex()
			.flex_col()
			.items_center()
			.overflow_hidden()
			.rounded(gpui::px(
				CAPSULE_HEIGHT / 2.
					+ fraction * (ui_live_activity::EXPANDED_RADIUS - CAPSULE_HEIGHT / 2.),
			))
			.border_1()
			.border_color(ui_theme::rgb(TEXT).opacity(0.1))
			.text_size(gpui::px(TEXT_SIZE))
			.font_weight(FontWeight::NORMAL)
			.line_height(gpui::px(16.))
			.whitespace_nowrap()
			.text_color(ui_theme::rgb(TEXT))
			.font_features(ui_theme::tabular_figures())
			.on_mouse_move(cx.listener(move |s, event: &MouseMoveEvent, _, cx| {
				if event.pressed_button.is_none() && !cx.has_active_drag() {
					s.hover_pill(activity, expandable, true, cx);
				}
			}))
			.on_hover(cx.listener(move |s, hovered, _, cx| {
				// Native MouseExited does not clear GPUI's last in-window position.
				// Only a real movement can reopen; layout-derived hover can only close.
				if !hovered {
					s.hover_pill(activity, expandable, false, cx);
				}
			}))
			.when(expanded, |d| {
				d.on_mouse_down_out(cx.listener(move |s, _, _, cx| {
					if s.activity == activity {
						s.close();
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
					.h(gpui::px(CAPSULE_HEIGHT - 2.))
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
							.on_key_down(cx.listener(move |s, e: &KeyDownEvent, _, cx| {
								if ["enter", "space"].contains(&e.keystroke.key.as_str()) {
									s.toggle(activity);
								} else if e.keystroke.key == "escape" {
									s.close();
								} else {
									return;
								}
								cx.notify();
								cx.stop_propagation();
							}))
					})
					.child(header),
			)
			.when_some(body, |d, body| {
				d.child(
					gpui::div()
						.relative()
						.left(gpui::px(-clip_x))
						.w(gpui::px(canvas.width - 2.))
						.flex_none()
						.opacity(details_opacity)
						.child(body),
				)
			})
			.into_any_element();
		gpui::div().relative().w_full().h(gpui::px(canvas.height)).child(surface).into_any_element()
	}
}

impl Render for PerformanceIsland {
	fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let debug = preference(None);

		self.reconcile(debug, window, cx);
		self.reconcile_live_clock(cx);

		let work = self.work.clone();
		if self.hover_intent.target().is_some_and(|activity| match activity {
			Activity::Work => work.is_none(),
			Activity::Debug => !debug,
		}) {
			self.hover_intent.cancel();
		}
		if !debug {
			ui_motion::hide_material_surface(Activity::Debug.ids().1, window, cx);
		}
		if work.is_none() {
			ui_motion::hide_material_surface(Activity::Work.ids().1, window, cx);
		}

		if !debug && work.is_none() {
			return gpui::div().into_any_element();
		}
		if work.is_none() {
			self.activity = Activity::Debug;
		}

		let mut activities = Vec::new();

		if let Some(work) = &work {
			activities.push((Activity::Work, Rc::new(readout::Content::work(work, window, cx))));
		}

		if debug {
			activities.push((Activity::Debug, Rc::new(readout::Content::debug(self, window, cx))));
		}

		let selected = self
			.expansion
			.is_open()
			.then(|| activities.iter().position(|(activity, _)| *activity == self.activity))
			.flatten();
		let sizes: Vec<_> = activities.iter().map(|(_, content)| content.sizing()).collect();
		let available =
			self.available_width.unwrap_or(f32::from(window.viewport_size().width) / 2.);
		let resting = ui_live_activity::arrange(available, ISLAND_GAP, &sizes, None);
		let allocations = ui_live_activity::arrange(available, ISLAND_GAP, &sizes, selected);
		let now = Instant::now();
		let extents: Vec<_> = activities
			.iter()
			.zip(&allocations)
			.map(|((activity, content), allocation)| {
				let open =
					self.expanded(*activity) && allocation.presentation == Presentation::Expanded;
				let (extent, moving) = self.morphs[usize::from(*activity == Activity::Debug)]
					.sample(
						Extent {
							width: allocation.width,
							height: if open { content.height() } else { CAPSULE_HEIGHT },
						},
						now,
						ui_motion::reduced(),
					);
				if moving {
					ui_motion::request_frame(window, cx);
				}
				let scale = window.scale_factor();
				Extent {
					width: (extent.width * scale).round() / scale,
					height: (extent.height * scale).round() / scale,
				}
			})
			.collect();
		let paired = activities.len() == 2;
		let mut islands = gpui::div().relative().flex().items_start().gap(gpui::px(ISLAND_GAP));

		for (index, (((activity, content), allocation), extent)) in
			activities.iter().zip(allocations).zip(extents).enumerate()
		{
			let pill = self.pill(*activity, allocation, (extent, paired), content, cx);
			islands = islands.child(if paired {
				// Fixed resting slots keep the inner edges stable through pixel rounding.
				gpui::div()
					.relative()
					.w(gpui::px(resting[index].width))
					.h(gpui::px(CAPSULE_HEIGHT))
					.flex_none()
					.child(
						gpui::div()
							.absolute()
							.top_0()
							.when(index == 0, |d| d.right_0())
							.when(index != 0, |d| d.left_0())
							.w(gpui::px(extent.width))
							.child(pill),
					)
					.into_any_element()
			} else {
				pill
			});
		}

		islands.into_any_element()
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

pub(crate) fn preference(value: Option<bool>) -> bool {
	static CACHE: AtomicU8 = AtomicU8::new(u8::MAX);

	ui_preferences::boolean("debugMode", &CACHE, value, false)
}

fn bytes(value: Option<f64>) -> String {
	value.map_or_else(|| "—".into(), |v| format!("{:.1} GB", v / 1_000_000_000.))
}

fn timing(ms: f64, samples: u64) -> String {
	if samples == 0 { "—".into() } else { format!("{ms:.1} ms") }
}

#[cfg(test)]
#[path = "ui_performance_tests.rs"]
mod tests;
