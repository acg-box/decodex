//! Content owns its measured footprint; the activity host owns allocation and motion.
use std::sync::{Arc, OnceLock};

use gpui::{
	AnyElement, App, Div, Font, FontFeatures, FontWeight, IntoElement, Role, SharedString,
	Stateful, TextRun, UniformListScrollHandle, Window,
	prelude::{
		FluentBuilder as _, InteractiveElement as _, ParentElement as _,
		StatefulInteractiveElement as _, Styled as _,
	},
};

use crate::{
	ui_live_activity::{CAPSULE_HEIGHT, Sizing},
	ui_motion,
	ui_performance::{self, PerformanceIsland, chart},
	ui_theme::{self, BLUE, TEXT_MUTED, WARNING},
	ui_work_metrics::{self, Icon, Live, Summary},
};

pub(super) const TEXT_SIZE: f32 = 11.;

const INSET: f32 = 12.;
const FRAME: f32 = 2. * INSET + 2.;

pub(super) struct Work {
	label: String,
	total: String,
	live: bool,
	estimated: bool,
	rate: String,
	rows: Arc<[Live]>,
	columns: [f32; 2],
	sizing: Sizing,
}
impl Work {
	fn unit(&self) -> &'static str {
		if self.live || self.estimated || self.rate == "—" { "tok/s" } else { "avg tok/s" }
	}
}

pub(super) struct Debug {
	label: String,
	fps: String,
	gpu: String,
	cpu: String,
	memory: String,
	latency: String,
	draw: String,
	columns: [f32; 3],
	sizing: Sizing,
}

struct Measured {
	text: String,
	font: Font,
	size: f32,
	width: f32,
}

struct ActionSwap {
	current: String,
	previous: Option<String>,
	started: std::time::Instant,
}

pub(super) enum Content {
	Work(Work),
	Debug(Debug),
}
impl Content {
	pub(super) fn work(work: &Summary, window: &mut Window, cx: &mut App) -> Self {
		let rate = work.display_rate_text();
		let total = work
			.total_tokens()
			.map_or_else(|| "—".into(), crate::shell::agent_surface::compact_tokens);
		let mut data = Work {
			label: format!(
				"{total} recorded tokens, including cached input. Output: {} tok/s. Last measured rate is kept while idle; tool and user waits excluded.",
				rate
			),
			total,
			live: work.working > 0 && work.generating(),
			estimated: work.rate_estimated(),
			rate,
			rows: work.live.clone().into(),
			columns: [0.; 2],
			sizing: Sizing { minimal: 0., compact: 0., expanded: 0. },
		};
		let mut width = |id, text: &str, size, weight| measure(id, text, size, weight, window, cx);
		let minimal = width("activity-width-live", &data.rate, TEXT_SIZE, FontWeight::MEDIUM)
			+ 3.
			+ width("activity-width-live-unit", data.unit(), TEXT_SIZE, FontWeight::NORMAL)
			+ 22.;
		let compact = width("activity-width-rate", &data.rate, TEXT_SIZE, FontWeight::MEDIUM)
			+ 4.
			+ width("activity-width-rate-unit", data.unit(), TEXT_SIZE, FontWeight::NORMAL)
			+ FRAME;
		let longest = |get: fn(&Live) -> String| {
			data.rows.iter().map(get).max_by_key(String::len).unwrap_or_default()
		};

		data.columns = [
			width(
				"activity-width-agent",
				&longest(|row| row.agent.clone()),
				TEXT_SIZE,
				FontWeight::MEDIUM,
			),
			width(
				"activity-width-elapsed",
				&longest(|row| ui_work_metrics::time_label(row.sample.active_time())),
				TEXT_SIZE,
				FontWeight::NORMAL,
			)
			.max(width(
				"activity-width-clock",
				"88m 88s",
				TEXT_SIZE,
				FontWeight::NORMAL,
			)),
		];

		let action = width(
			"activity-width-action",
			&longest(|row| row.action.clone()),
			TEXT_SIZE,
			FontWeight::NORMAL,
		)
		.min(180.);
		let details = if data.rows.is_empty() {
			0.
		} else {
			data.columns.iter().sum::<f32>() + action + 16. + FRAME
		};
		let total_width =
			width("activity-width-total-label", "Recorded tokens", TEXT_SIZE, FontWeight::NORMAL)
				+ width("activity-width-total-value", &data.total, TEXT_SIZE, FontWeight::MEDIUM)
				+ 28.
				+ FRAME;

		data.sizing = Sizing {
			minimal,
			compact: compact.max(minimal),
			expanded: details.max(compact).max(minimal).max(total_width),
		};

		Self::Work(data)
	}

	pub(super) fn debug(island: &PerformanceIsland, window: &mut Window, cx: &mut App) -> Self {
		let fps = if island.metrics.draw_frames == 0 {
			"—".into()
		} else {
			format!("{:.0}", island.metrics.fps)
		};
		let mut data = Debug {
			label: format!("Performance details: {fps} FPS · {}", island.gpu_label()),
			fps,
			gpu: island.host.gpu_percent.map_or_else(|| "—".into(), |v| format!("{v:.0}%")),
			cpu: island.cpu_percent.map_or_else(|| "—".into(), |v| format!("{v:.0}%")),
			memory: ui_performance::bytes(island.host.peak_memory_bytes),
			latency: ui_performance::timing(island.metrics.submit_ms, island.metrics.ready_samples),
			draw: ui_performance::timing(island.metrics.draw_ms, island.metrics.draw_frames),
			columns: [0.; 3],
			sizing: Sizing { minimal: 0., compact: 0., expanded: 0. },
		};
		let mut width = |id, text: &str, size, weight| measure(id, text, size, weight, window, cx);
		let fps = width("activity-width-fps", &data.fps, TEXT_SIZE, FontWeight::MEDIUM);
		let minimal = fps
			+ 3.
			+ width("activity-width-fps-small", "FPS", TEXT_SIZE, FontWeight::NORMAL)
			+ 22.;
		let compact = width("activity-width-fps-label", "FPS", TEXT_SIZE, FontWeight::NORMAL)
			+ 4.
			+ fps
			+ 16.
			+ width("activity-width-gpu-label", "GPU", TEXT_SIZE, FontWeight::NORMAL)
			+ 4.
			+ width("activity-width-gpu", &data.gpu, TEXT_SIZE, FontWeight::MEDIUM)
			+ FRAME;

		data.columns = [
			width("activity-width-cpu-label", "CPU", TEXT_SIZE, FontWeight::NORMAL)
				.max(width("activity-width-cpu", &data.cpu, TEXT_SIZE, FontWeight::MEDIUM))
				.max(width("activity-width-draw-label", "Draw p95", TEXT_SIZE, FontWeight::NORMAL))
				.max(width("activity-width-draw", &data.draw, TEXT_SIZE, FontWeight::MEDIUM)),
			width("activity-width-memory-label", "Peak RAM", TEXT_SIZE, FontWeight::NORMAL)
				.max(width("activity-width-memory", &data.memory, TEXT_SIZE, FontWeight::MEDIUM)),
			width("activity-width-latency-label", "Latency p95", TEXT_SIZE, FontWeight::NORMAL)
				.max(width("activity-width-latency", &data.latency, TEXT_SIZE, FontWeight::MEDIUM)),
		];

		let details = data.columns.iter().sum::<f32>() + 24. + FRAME;

		data.sizing = Sizing {
			minimal,
			compact: compact.max(minimal),
			expanded: details.max(compact).max(minimal),
		};

		Self::Debug(data)
	}

	pub(super) fn sizing(&self) -> Sizing {
		match self {
			Self::Work(v) => v.sizing,
			Self::Debug(v) => v.sizing,
		}
	}

	pub(super) fn height(&self) -> f32 {
		match self {
			Self::Work(v) => CAPSULE_HEIGHT + 38. + v.rows.len().min(4) as f32 * 22.,
			Self::Debug(_) => CAPSULE_HEIGHT + 68.,
		}
	}

	pub(super) fn label(&self) -> &str {
		match self {
			Self::Work(v) => &v.label,
			Self::Debug(v) => &v.label,
		}
	}

	pub(super) fn expandable(&self) -> bool {
		self.height() > CAPSULE_HEIGHT
	}

	pub(super) fn header(&self, fraction: f32) -> AnyElement {
		match self {
			Self::Work(v) => presentations(work_minimal(v), work_header(v), fraction),
			Self::Debug(v) => presentations(debug_minimal(v), debug_header(v), fraction),
		}
	}

	pub(super) fn details(
		&self,
		island: &PerformanceIsland,
		window: &mut Window,
		cx: &mut App,
	) -> AnyElement {
		match self {
			Self::Work(v) => work_details(v, window, cx),
			Self::Debug(v) => debug_details(v, island, window, cx),
		}
	}
}

pub(super) fn figures() -> FontFeatures {
	static FEATURES: OnceLock<FontFeatures> = OnceLock::new();

	FEATURES.get_or_init(|| FontFeatures(Arc::new(vec![("tnum".into(), 1)]))).clone()
}

// One entry per field, not a growing cache of sampled numbers. Chart frames do
// not shape the same text again. Measure with the font and figures we render.
fn measure(
	id: &'static str,
	text: &str,
	size: f32,
	weight: FontWeight,
	window: &mut Window,
	cx: &mut App,
) -> f32 {
	let mut font = window.text_style().font();

	font.weight = weight;
	font.features = figures();

	let state = window.use_keyed_state(id, cx, |_, _| None::<Measured>);

	if let Some(last) = state.read(cx)
		&& last.text == text
		&& last.font == font
		&& last.size == size
	{
		return last.width;
	}

	let width = f32::from(
		window
			.text_system()
			.shape_line(
				text.to_owned().into(),
				gpui::px(size),
				&[TextRun { len: text.len(), font: font.clone(), ..Default::default() }],
				None,
			)
			.width,
	)
	.ceil();

	state.update(cx, |state, _| *state = Some(Measured { text: text.into(), font, size, width }));

	width
}

fn number(id: &'static str, value: String) -> Div {
	gpui::div()
		.debug_selector(move || format!("performance-{id}"))
		.flex_none()
		.text_right()
		.whitespace_nowrap()
		.font_weight(FontWeight::MEDIUM)
		.child(value)
}

fn debug_header(data: &Debug) -> AnyElement {
	let field = |id, label, value| {
		gpui::div()
			.flex()
			.items_baseline()
			.gap_1()
			.child(
				gpui::div()
					.debug_selector(move || format!("performance-{id}-label"))
					.text_color(ui_theme::rgb(TEXT_MUTED))
					.child(label),
			)
			.child(number(id, value))
	};

	gpui::div()
		.w_full()
		.px(gpui::px(INSET))
		.flex()
		.items_center()
		.justify_between()
		.child(field("fps", "FPS", data.fps.clone()))
		.child(field("gpu", "GPU", data.gpu.clone()))
		.into_any_element()
}

fn work_header(data: &Work) -> AnyElement {
	gpui::div()
		.w_full()
		.px(gpui::px(INSET))
		.flex()
		.items_baseline()
		.justify_center()
		.gap_1()
		.child(number("work-rate", data.rate.clone()).text_color(ui_theme::rgb(if data.live {
			BLUE
		} else {
			TEXT_MUTED
		})))
		.child(gpui::div().text_color(ui_theme::rgb(TEXT_MUTED)).child(data.unit()))
		.into_any_element()
}

fn work_minimal(data: &Work) -> AnyElement {
	gpui::div()
		.debug_selector(|| "activity-live-rate".into())
		.flex()
		.items_baseline()
		.gap(gpui::px(3.))
		.whitespace_nowrap()
		.text_color(ui_theme::rgb(if data.live { BLUE } else { TEXT_MUTED }))
		.child(number("live-rate", data.rate.clone()))
		.child(
			gpui::div()
				.text_size(gpui::px(TEXT_SIZE))
				.text_color(ui_theme::rgb(TEXT_MUTED))
				.child(data.unit()),
		)
		.into_any_element()
}

fn debug_minimal(data: &Debug) -> AnyElement {
	gpui::div()
		.flex()
		.items_baseline()
		.gap(gpui::px(3.))
		.whitespace_nowrap()
		.child(number("fps", data.fps.clone()))
		.child(
			gpui::div()
				.text_size(gpui::px(TEXT_SIZE))
				.text_color(ui_theme::rgb(TEXT_MUTED))
				.child("FPS"),
		)
		.into_any_element()
}

fn presentations(minimal: AnyElement, compact: AnyElement, fraction: f32) -> AnyElement {
	gpui::div()
		.relative()
		.w_full()
		.h_full()
		.when(fraction < 0.999, |d| {
			d.child(
				gpui::div()
					.absolute()
					.size_full()
					.flex()
					.items_center()
					.justify_center()
					.opacity(1. - fraction)
					.child(minimal),
			)
		})
		.when(fraction > 0.001, |d| {
			d.child(
				gpui::div()
					.absolute()
					.size_full()
					.flex()
					.items_center()
					.justify_center()
					.opacity(fraction)
					.child(compact),
			)
		})
		.into_any_element()
}

fn live_row(row: &Live, columns: [f32; 2], window: &mut Window, cx: &mut App) -> Stateful<Div> {
	let state = window.use_keyed_state(
		SharedString::from(format!("activity-action-{}", row.key)),
		cx,
		|_, _| ActionSwap {
			current: row.action.clone(),
			previous: None,
			started: std::time::Instant::now(),
		},
	);
	let reduced = ui_motion::reduced();
	let (current, previous, progress) = state.update(cx, |state, _| {
		if state.current != row.action {
			state.previous = (!reduced).then(|| state.current.clone());
			state.current = row.action.clone();
			state.started = std::time::Instant::now();
		}

		let progress =
			if reduced { 1. } else { (state.started.elapsed().as_secs_f32() / 0.18).min(1.) };

		if progress >= 1. {
			state.previous = None;
		}

		(state.current.clone(), state.previous.clone(), progress)
	});

	if previous.is_some() {
		ui_motion::request_frame(window, cx);
	}

	let action = |text: String, opacity| {
		gpui::div()
			.absolute()
			.inset_0()
			.overflow_hidden()
			.text_ellipsis()
			.whitespace_nowrap()
			.opacity(opacity)
			.child(text)
	};
	let changing = previous.is_some();
	let key = row.key.clone();
	let elapsed = ui_work_metrics::time_label(row.sample.active_time());

	gpui::div()
		.id(SharedString::from(format!("activity-live-{}", row.key)))
		.debug_selector(move || format!("activity-live-{key}"))
		.role(Role::Group)
		.aria_label(format!("{}: {}. Turn elapsed {}.", row.agent, row.action, elapsed))
		.w_full()
		.h(gpui::px(22.))
		.flex_none()
		.flex()
		.items_center()
		.gap(gpui::px(8.))
		.child(
			gpui::div()
				.w(gpui::px(columns[0]))
				.flex_none()
				.font_weight(FontWeight::MEDIUM)
				.text_color(ui_theme::rgb(BLUE))
				.child(row.agent.clone()),
		)
		.child(
			gpui::div()
				.relative()
				.flex_1()
				.min_w_0()
				.h(gpui::px(16.))
				.line_height(gpui::px(16.))
				.when_some(previous, |view, previous| view.child(action(previous, 1. - progress)))
				.child(action(current, if changing { progress } else { 1. })),
		)
		.child(
			gpui::div()
				.w(gpui::px(columns[1]))
				.flex_none()
				.text_right()
				.text_color(ui_theme::rgb(TEXT_MUTED))
				.child(elapsed),
		)
}

fn work_details(data: &Work, window: &mut Window, cx: &mut App) -> AnyElement {
	let scroll =
		window.use_keyed_state("activity-live-scroll", cx, |_, _| UniformListScrollHandle::new());
	let rows = data.rows.clone();
	let columns = data.columns;
	let list = gpui::uniform_list("activity-live-rows", rows.len(), move |range, window, cx| {
		range.map(|index| live_row(&rows[index], columns, window, cx)).collect()
	})
	.debug_selector(|| "activity-work-details".into())
	.track_scroll(scroll.read(cx))
	.w_full()
	.h(gpui::px(data.rows.len().min(4) as f32 * 22.));

	gpui::div()
		.w_full()
		.px(gpui::px(INSET))
		.pb(gpui::px(8.))
		.child(
			gpui::div()
				.debug_selector(|| "activity-work-total".into())
				.h(gpui::px(30.))
				.flex()
				.items_center()
				.justify_between()
				.gap_3()
				.child(
					gpui::div()
						.flex()
						.items_center()
						.gap_1()
						.child(Icon::Tokens.element(11., TEXT_MUTED))
						.child("Recorded tokens"),
				)
				.child(number("activity-total-value", data.total.clone())),
		)
		.when(!data.rows.is_empty(), |body| body.child(list))
		.into_any_element()
}

fn debug_details(
	data: &Debug,
	island: &PerformanceIsland,
	window: &mut Window,
	cx: &mut App,
) -> AnyElement {
	let columns = ["activity-column-cpu", "activity-column-memory", "activity-column-latency"]
		.into_iter()
		.zip(data.columns)
		.map(|(id, width)| ui_motion::value(id, width, window, cx))
		.collect::<Vec<_>>();
	let rows = [
		("cpu", "CPU", data.cpu.clone()),
		("memory", "Peak RAM", data.memory.clone()),
		("latency", "Latency p95", data.latency.clone()),
	];

	gpui::div()
		.debug_selector(|| "performance-island-details".into())
		.w_full()
		.px(gpui::px(INSET))
		.pb(gpui::px(6.))
		.flex_none()
		.flex()
		.flex_col()
		.gap_1()
		.child(
			gpui::div().w_full().flex().justify_between().gap_3().children(
				rows.into_iter().zip(&columns).map(|((id, label, value), width)| {
					metric(id, label, value).w(gpui::px(*width))
				}),
			),
		)
		.child(
			gpui::div()
				.w_full()
				.h(gpui::px(28.))
				.flex_none()
				.flex()
				.items_center()
				.gap_2()
				.child(metric("draw", "Draw p95", data.draw.clone()).w(gpui::px(columns[0])).when(
					island.display_hz.is_some_and(|hz| island.metrics.draw_ms > 1_000. / hz),
					|d| d.text_color(ui_theme::rgb(WARNING)),
				))
				.child(chart::render(&island.history, window, cx)),
		)
		.into_any_element()
}

fn metric(id: &'static str, label: &'static str, value: String) -> Div {
	gpui::div()
		.debug_selector(move || format!("performance-{id}-row"))
		.min_w_0()
		.h(gpui::px(28.))
		.flex_none()
		.flex()
		.flex_col()
		.whitespace_nowrap()
		.child(
			gpui::div()
				.debug_selector(move || format!("performance-{id}-label"))
				.text_size(gpui::px(TEXT_SIZE))
				.line_height(gpui::px(12.))
				.text_color(ui_theme::rgb(if id == "draw" { BLUE } else { TEXT_MUTED }))
				.child(label),
		)
		.child(number(id, value).w_full().text_left())
}
