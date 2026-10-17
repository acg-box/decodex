//! An opt-in performance activity in the window's shared top-center space.
#[path = "ui_performance_chart.rs"] mod chart;
#[path = "performance_host.rs"] mod host;
#[cfg(test)]
#[path = "ui_performance_tests.rs"]
mod tests;
use crate::{
	ui_motion,
	ui_theme::{TEXT, TEXT_MUTED},
};
use gpui::{
	AnyElement, Context, FontWeight, IntoElement, KeyDownEvent, Render, Role, Task, Window,
	prelude::*, profiler::FrameDurationSnapshot,
};
use std::{
	collections::VecDeque,
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
impl Expansion {
	fn is_open(self) -> bool {
		self != Self::Closed
	}

	fn clicked(self) -> Self {
		if self == Self::Clicked { Self::Closed } else { Self::Clicked }
	}

	fn hovered(self, hovered: bool) -> Self {
		match (self, hovered) {
			(Self::Closed, true) => Self::Hover,
			(Self::Hover, false) => Self::Closed,
			_ => self,
		}
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
	task: Option<Task<()>>,
	metrics: Metrics,
	host: host::HostSample,
	cpu_percent: Option<f64>,
	display_hz: Option<f64>,
	history: VecDeque<f64>,
	sampled_at: Instant,
	sample_period: Duration,
}
impl PerformanceIsland {
	pub(crate) fn new(_: &mut Context<Self>) -> Self {
		Self {
			expansion: Expansion::Closed,
			task: None,
			metrics: Metrics::default(),
			host: host::HostSample::default(),
			cpu_percent: None,
			display_hz: None,
			history: VecDeque::new(),
			sampled_at: Instant::now(),
			sample_period: Duration::from_millis(500),
		}
	}

	fn reconcile(&mut self, enabled: bool, window: &Window, cx: &mut Context<Self>) {
		if !enabled {
			self.task = None;
			self.expansion = Expansion::Closed;
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
						s.sample_period = Duration::from_secs_f64(seconds);
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
			self.sampled_at = Instant::now();
			self.history.push_back(metrics.draw_ms);
			if self.history.len() > 32 {
				self.history.pop_front();
			}
		}
		self.metrics = metrics;
	}

	fn details(&self, window: &mut Window, cx: &mut gpui::App) -> AnyElement {
		let budget = self
			.display_hz
			.map_or_else(|| "—".into(), |hz| format!("{hz:.0} Hz · {:.2} ms", 1_000. / hz));
		let cpu = self.cpu_percent.map_or_else(|| "—".into(), |v| format!("{v:.0}%"));
		let rows = [
			("CPU", cpu),
			("Peak RAM", bytes(self.host.peak_memory_bytes)),
			("Latency p95", timing(self.metrics.submit_ms, self.metrics.ready_samples)),
		];
		gpui::div()
			.debug_selector(|| "performance-island-details".into())
			.w(gpui::px(302.))
			.px_3()
			.pb_2()
			.pt_1()
			.flex()
			.flex_col()
			.gap_1()
			.child(
				gpui::div()
					.flex()
					.justify_between()
					.child(format!(
						"Draw p95 {}",
						timing(self.metrics.draw_ms, self.metrics.draw_frames)
					))
					.child(gpui::div().text_color(gpui::rgb(TEXT_MUTED)).child(budget)),
			)
			.child(chart::render(
				&self.history,
				self.display_hz.map(|hz| 1_000. / hz),
				self.sampled_at.elapsed(),
				self.sample_period,
				window,
				cx,
			))
			.child(gpui::div().flex().justify_between().gap_2().children(rows.into_iter().map(
				|(label, value)| {
					gpui::div()
						.flex()
						.gap_1()
						.whitespace_nowrap()
						.child(gpui::div().text_color(gpui::rgb(TEXT_MUTED)).child(label))
						.child(value)
				},
			)))
			.into_any_element()
	}

	fn gpu_label(&self) -> String {
		self.host
			.gpu_percent
			.map_or_else(|| "System GPU —".into(), |v| format!("System GPU {v:.0}%"))
	}
}
impl Render for PerformanceIsland {
	fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let enabled = preference(None);
		self.reconcile(enabled, window, cx);
		if !enabled {
			return gpui::div().into_any_element();
		}
		let expanded = self.expansion.is_open();
		let width = ui_motion::value(
			"activity-island-width",
			if expanded { 304. } else { 208. },
			window,
			cx,
		);
		let height = ui_motion::value(
			"activity-island-height",
			if expanded { 116. } else { 28. },
			window,
			cx,
		);
		let fraction = ((height - 28.) / 88.).clamp(0., 1.);
		let label = if self.metrics.draw_frames == 0 {
			"Measuring…".into()
		} else {
			format!("{:.0} FPS  ·  {}", self.metrics.fps, self.gpu_label())
		};
		gpui::div()
			.relative()
			.w(gpui::px(width))
			.h(gpui::px(28.))
			.child(
				gpui::deferred(
					gpui::div()
						.id("performance-island")
						.debug_selector(|| "performance-island".into())
						.relative()
						.w_full()
						.h(gpui::px(height))
						.overflow_hidden()
						.rounded(gpui::px(14. + fraction * 6.))
						.bg(gpui::rgb(0x141418))
						.border_1()
						.border_color(gpui::rgba(0xffffff18))
						.text_size(gpui::px(11.))
						.line_height(gpui::px(16.))
						.text_color(gpui::rgb(TEXT))
						.on_hover(cx.listener(|s, hovered, _, cx| {
							let next = s.expansion.hovered(*hovered);
							if s.expansion != next {
								s.expansion = next;
								cx.notify();
							}
						}))
						.when(expanded, |d| {
							d.on_mouse_down_out(cx.listener(|s, _, _, cx| {
								s.expansion = Expansion::Closed;
								cx.notify();
							}))
						})
						.child(
							gpui::div()
								.id("performance-island-toggle")
								.debug_selector(|| "performance-island-toggle".into())
								.role(Role::Button)
								.aria_label(format!("Performance details: {label}"))
								.aria_expanded(expanded)
								.tab_index(0)
								.h(gpui::px(28.))
								.px_3()
								.flex()
								.items_center()
								.justify_center()
								.cursor_pointer()
								.font_weight(FontWeight::MEDIUM)
								.on_click(cx.listener(|s, _, _, cx| {
									s.expansion = s.expansion.clicked();
									cx.notify();
								}))
								.on_key_down(cx.listener(|s, e: &KeyDownEvent, _, cx| {
									if ["enter", "space"].contains(&e.keystroke.key.as_str()) {
										s.expansion = s.expansion.clicked();
										cx.notify();
										cx.stop_propagation();
									} else if e.keystroke.key == "escape" {
										s.expansion = Expansion::Closed;
										cx.notify();
										cx.stop_propagation();
									}
								}))
								.child(label),
						)
						.when(fraction > 0.001, |d| {
							d.child(gpui::div().opacity(fraction).child(self.details(window, cx)))
						}),
				)
				.with_priority(2),
			)
			.into_any_element()
	}
}

fn timing(ms: f64, samples: u64) -> String {
	if samples == 0 { "—".into() } else { format!("{ms:.1} ms") }
}
