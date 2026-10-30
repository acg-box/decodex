//! Content owns its measured footprint; the activity host owns allocation and motion.
use std::sync::Arc;

use gpui::{
	AnyElement, App, Div, Font, FontWeight, IntoElement, Role, SharedString, Stateful, TextRun,
	UniformListScrollHandle, Window,
	prelude::{
		FluentBuilder as _, InteractiveElement as _, ParentElement as _,
		StatefulInteractiveElement as _, Styled as _,
	},
};

use crate::{
	ui_live_activity::{CAPSULE_HEIGHT, Sizing},
	ui_motion,
	ui_performance::{self, PerformanceIsland, chart},
	ui_theme::{self, BLUE, TEXT, TEXT_MUTED, WARNING},
	ui_work_metrics::{self, Live, Summary},
};

pub(super) const TEXT_SIZE: f32 = 11.;

const INSET: f32 = 12.;
const FRAME: f32 = 2. * INSET + 2.;
const METRIC_HEIGHT: f32 = 28.;
const BODY_TOP: f32 = 4.;
const BODY_BOTTOM: f32 = 8.;
const METRIC_GAP: f32 = 8.;
const COLUMN_GAP: f32 = 16.;

pub(super) struct Work {
	label: String,
	total: String,
	working: bool,
	live: bool,
	estimated: bool,
	rate: String,
	rows: Arc<[Live]>,
	columns: [f32; 2],
	breakdown: [String; 4],
	breakdown_columns: [f32; 2],
	sizing: Sizing,
}
impl Work {
	fn idle_total(&self) -> bool {
		!self.working && self.total != "—"
	}

	fn status(&self) -> Option<&'static str> {
		activity_status(self.working, self.live, self.rate != "—")
	}

	fn unit(&self) -> &'static str {
		if self.live || self.estimated || self.rate == "—" { "tok/s" } else { "avg tok/s" }
	}
}

fn activity_status(working: bool, generating: bool, has_rate: bool) -> Option<&'static str> {
	if !working {
		Some("Idle")
	} else if !generating {
		Some("Working")
	} else if !has_rate {
		Some("Generating")
	} else {
		None
	}
}

fn tokens(value: &str) -> String {
	if value == "—" { value.into() } else { format!("{value} tok") }
}

fn breakdown_columns(
	data: &Work,
	width: &mut impl FnMut(&'static str, &str, f32, FontWeight) -> f32,
) -> [f32; 2] {
	let labels = [
		("activity-input-label", "activity-input-value", "Input"),
		("activity-output-label", "activity-output-value", "Output"),
		("activity-cached-label", "activity-cached-value", "Cached input"),
		("activity-reasoning-label", "activity-reasoning-value", "Reasoning"),
	];

	std::array::from_fn(|column| {
		[column, column + 2].into_iter().fold(0_f32, |maximum, index| {
			let (label_id, value_id, label) = labels[index];

			maximum.max(width(label_id, label, TEXT_SIZE, FontWeight::NORMAL)).max(width(
				value_id,
				&tokens(&data.breakdown[index]),
				TEXT_SIZE,
				FontWeight::MEDIUM,
			))
		})
	})
}

pub(super) struct Debug {
	label: String,
	fps: String,
	gpu: String,
	cpu: String,
	memory: String,
	latency: String,
	draw: String,
	columns: [f32; 2],
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
		let live = work.working > 0 && work.generating();
		let rate = if live { work.display_rate_text() } else { "—".into() };
		let total = work
			.total_tokens()
			.map_or_else(|| "—".into(), crate::shell::agent_surface::compact_tokens);
		let breakdown = work.token_breakdown();
		let count = |value: Option<u64>| {
			value.map_or_else(|| "—".into(), crate::shell::agent_surface::compact_tokens)
		};
		let mut data = Work {
			label: String::new(),
			total,
			working: work.working > 0,
			live,
			estimated: work.rate_estimated(),
			rate,
			rows: work.live.clone().into(),
			columns: [0.; 2],
			breakdown: [
				count(breakdown.map(|v| v.input_tokens)),
				count(breakdown.map(|v| v.output_tokens)),
				count(breakdown.map(|v| v.cached_input_tokens)),
				count(breakdown.map(|v| v.reasoning_output_tokens)),
			],
			breakdown_columns: [0.; 2],
			sizing: Sizing { minimal: 0., compact: 0., expanded: 0. },
		};
		let activity = if let Some(status) = data.status() {
			status.to_owned()
		} else {
			format!(
				"{} {}. Estimated generation rate excludes tool and user waits.",
				data.rate,
				data.unit()
			)
		};

		data.label = format!(
			"{activity}. Current graph agents: {} cumulative tokens, including cached input. Adding or archiving agents changes this amount.",
			data.total
		);
		let mut width = |id, text: &str, size, weight| measure(id, text, size, weight, window, cx);
		let primary = if let Some(status) = data.status() {
			width("activity-width-status", status, TEXT_SIZE, FontWeight::MEDIUM)
		} else {
			width("activity-width-rate", &data.rate, TEXT_SIZE, FontWeight::MEDIUM)
				+ 4.
				+ width("activity-width-rate-unit", data.unit(), TEXT_SIZE, FontWeight::NORMAL)
		};
		let minimal = if data.idle_total() {
			width("activity-width-idle-total", &data.total, TEXT_SIZE, FontWeight::MEDIUM)
				+ 3.
				+ width("activity-width-idle-unit", "tok", TEXT_SIZE, FontWeight::NORMAL)
				+ 22.
		} else {
			primary + 22.
		};
		let compact = primary
			+ 16.
			+ width("activity-width-total-label", "Total", TEXT_SIZE, FontWeight::NORMAL)
			+ 4.
			+ width(
				"activity-width-total-value",
				&tokens(&data.total),
				TEXT_SIZE,
				FontWeight::MEDIUM,
			)
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
		data.breakdown_columns = breakdown_columns(&data, &mut width);
		let breakdown_width = data.breakdown_columns.iter().sum::<f32>() + COLUMN_GAP + FRAME;

		data.sizing = Sizing {
			minimal,
			compact: compact.max(minimal),
			expanded: details.max(compact).max(minimal).max(breakdown_width),
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
			columns: [0.; 2],
			sizing: Sizing { minimal: 0., compact: 0., expanded: 0. },
		};
		let mut width = |id, text: &str, size, weight| measure(id, text, size, weight, window, cx);
		let fps = width("activity-width-fps", &data.fps, TEXT_SIZE, FontWeight::MEDIUM);
		let minimal = fps
			+ 3.
			+ width("activity-width-fps-small", "FPS", TEXT_SIZE, FontWeight::NORMAL)
			+ 22.;
		let compact =
			width("activity-width-fps-label", "Frame rate", TEXT_SIZE, FontWeight::NORMAL)
				+ 4.
				+ width(
					"activity-width-fps-value",
					&format!("{} FPS", data.fps),
					TEXT_SIZE,
					FontWeight::MEDIUM,
				)
				+ FRAME;

		data.columns = [
			width("activity-width-gpu-label", "System GPU", TEXT_SIZE, FontWeight::NORMAL)
				.max(width("activity-width-gpu", &data.gpu, TEXT_SIZE, FontWeight::MEDIUM))
				.max(width(
					"activity-width-memory-label",
					"UI peak RAM",
					TEXT_SIZE,
					FontWeight::NORMAL,
				))
				.max(width("activity-width-memory", &data.memory, TEXT_SIZE, FontWeight::MEDIUM))
				.max(width("activity-width-draw-label", "Draw p95", TEXT_SIZE, FontWeight::NORMAL))
				.max(width("activity-width-draw", &data.draw, TEXT_SIZE, FontWeight::MEDIUM)),
			width("activity-width-cpu-label", "UI CPU", TEXT_SIZE, FontWeight::NORMAL)
				.max(width("activity-width-cpu", &data.cpu, TEXT_SIZE, FontWeight::MEDIUM))
				.max(width(
					"activity-width-latency-label",
					"Latency p95",
					TEXT_SIZE,
					FontWeight::NORMAL,
				))
				.max(width("activity-width-latency", &data.latency, TEXT_SIZE, FontWeight::MEDIUM)),
		];

		// The sparkline gets enough space to show its measured history.
		let details = data.columns[0] + data.columns[1].max(96.) + COLUMN_GAP + FRAME;

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
			Self::Work(v) =>
				CAPSULE_HEIGHT
					+ BODY_TOP
					+ 2. * METRIC_HEIGHT
					+ METRIC_GAP
					+ BODY_BOTTOM
					+ if v.rows.is_empty() {
						0.
					} else {
						METRIC_GAP + v.rows.len().min(4) as f32 * 22.
					},
			Self::Debug(_) =>
				CAPSULE_HEIGHT + BODY_TOP + 3. * METRIC_HEIGHT + 2. * METRIC_GAP + BODY_BOTTOM,
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
	font.features = ui_theme::tabular_figures();

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

fn measurement(id: &'static str, value: String) -> Div {
	let unit = [" tok", " FPS", " ms", " GB", "%"]
		.into_iter()
		.find_map(|suffix| value.strip_suffix(suffix).map(|number| (number, suffix)));
	let (number, unit) = unit.map_or_else(
		|| (value.clone(), None),
		|(number, suffix)| {
			(number.to_owned(), Some((suffix.trim_start(), suffix.starts_with(' '))))
		},
	);

	gpui::div()
		.debug_selector(move || format!("performance-{id}"))
		.flex_none()
		.flex()
		.items_baseline()
		.whitespace_nowrap()
		.child(gpui::div().font_weight(FontWeight::MEDIUM).child(number))
		.when_some(unit, |d, (unit, spaced)| {
			d.child(
				gpui::div()
					.debug_selector(move || format!("performance-{id}-unit"))
					.ml(gpui::px(if spaced { 3. } else { 0. }))
					.font_weight(FontWeight::NORMAL)
					.text_color(ui_theme::rgb(TEXT_MUTED))
					.child(unit),
			)
		})
}

fn debug_header(data: &Debug) -> AnyElement {
	gpui::div()
		.w_full()
		.px(gpui::px(INSET))
		.flex()
		.items_center()
		.gap_1()
		.child(
			gpui::div()
				.debug_selector(|| "performance-fps-label".into())
				.text_color(ui_theme::rgb(TEXT_MUTED))
				.child("Frame rate"),
		)
		.child(measurement("fps", format!("{} FPS", data.fps)))
		.into_any_element()
}

fn work_header(data: &Work) -> AnyElement {
	gpui::div()
		.w_full()
		.px(gpui::px(INSET))
		.flex()
		.items_center()
		.justify_between()
		.gap_3()
		.child(work_primary(data, false))
		.child(
			gpui::div()
				.debug_selector(|| "activity-work-total".into())
				.flex()
				.items_baseline()
				.gap_1()
				.child(gpui::div().text_color(ui_theme::rgb(TEXT_MUTED)).child("Total"))
				.child(measurement("activity-total-value", tokens(&data.total))),
		)
		.into_any_element()
}

fn work_minimal(data: &Work) -> AnyElement {
	let content = if data.idle_total() {
		gpui::div()
			.flex()
			.items_baseline()
			.gap(gpui::px(3.))
			.whitespace_nowrap()
			.child(number("live-total", data.total.clone()).text_color(ui_theme::rgb(TEXT)))
			.child(
				gpui::div()
					.debug_selector(|| "performance-live-total-unit".into())
					.text_color(ui_theme::rgb(TEXT_MUTED))
					.child("tok"),
			)
			.into_any_element()
	} else {
		work_primary(data, true)
	};

	gpui::div().debug_selector(|| "activity-live-rate".into()).child(content).into_any_element()
}

fn work_primary(data: &Work, minimal: bool) -> AnyElement {
	if let Some(status) = data.status() {
		return number(if minimal { "live-status" } else { "work-status" }, status.into())
			.when(!minimal && !data.working, |view| {
				view.font_weight(FontWeight::NORMAL).text_color(ui_theme::rgb(TEXT_MUTED))
			})
			.into_any_element();
	}

	gpui::div()
		.flex()
		.items_baseline()
		.gap(gpui::px(if minimal { 3. } else { 4. }))
		.whitespace_nowrap()
		.child(
			number(if minimal { "live-rate" } else { "work-rate" }, data.rate.clone())
				.text_color(ui_theme::rgb(TEXT)),
		)
		.child(gpui::div().text_color(ui_theme::rgb(TEXT_MUTED)).child(data.unit()))
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
	let fields = [
		("work-input", "Input", "Input tokens"),
		("work-output", "Output", "Output tokens"),
		("work-cached", "Cached input", "Cached input, included in input"),
		("work-reasoning", "Reasoning", "Reasoning tokens, included in output"),
	];
	let field = |index: usize| {
		let (id, label, description) = fields[index];
		let value = tokens(&data.breakdown[index]);

		metric(id, label, value.clone())
			.w(gpui::px(data.breakdown_columns[index % 2]))
			.role(Role::Group)
			.aria_label(format!("{description}: {value}"))
	};
	let row = |first| {
		gpui::div()
			.w_full()
			.flex()
			.justify_between()
			.gap(gpui::px(COLUMN_GAP))
			.child(field(first))
			.child(field(first + 1))
	};

	gpui::div()
		.w_full()
		.px(gpui::px(INSET))
		.pt(gpui::px(BODY_TOP))
		.pb(gpui::px(BODY_BOTTOM))
		.flex()
		.flex_col()
		.gap(gpui::px(METRIC_GAP))
		.child(
			gpui::div()
				.debug_selector(|| "activity-work-breakdown".into())
				.w_full()
				.flex()
				.flex_col()
				.gap(gpui::px(METRIC_GAP))
				.child(row(0))
				.child(row(2)),
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
	let columns = ["activity-debug-column-left", "activity-debug-column-right"]
		.into_iter()
		.zip(data.columns)
		.map(|(id, width)| ui_motion::value(id, width, window, cx))
		.collect::<Vec<_>>();
	let row = |left: (&'static str, &'static str, String),
	           right: (&'static str, &'static str, String)| {
		gpui::div()
			.w_full()
			.flex()
			.justify_between()
			.gap(gpui::px(COLUMN_GAP))
			.child(metric(left.0, left.1, left.2).w(gpui::px(columns[0])))
			.child(metric(right.0, right.1, right.2).w(gpui::px(columns[1])))
	};

	gpui::div()
		.debug_selector(|| "performance-island-details".into())
		.w_full()
		.px(gpui::px(INSET))
		.pt(gpui::px(BODY_TOP))
		.pb(gpui::px(BODY_BOTTOM))
		.flex_none()
		.flex()
		.flex_col()
		.gap(gpui::px(METRIC_GAP))
		.child(row(("gpu", "System GPU", data.gpu.clone()), ("cpu", "UI CPU", data.cpu.clone())))
		.child(row(
			("memory", "UI peak RAM", data.memory.clone()),
			("latency", "Latency p95", data.latency.clone()),
		))
		.child(
			gpui::div()
				.w_full()
				.h(gpui::px(METRIC_HEIGHT))
				.flex_none()
				.flex()
				.items_center()
				.gap(gpui::px(COLUMN_GAP))
				.child(metric("draw", "Draw p95", data.draw.clone()).w(gpui::px(columns[0])).when(
					island.display_hz.is_some_and(|hz| island.metrics.draw_ms > 1_000. / hz),
					|d| d.text_color(ui_theme::rgb(WARNING)),
				))
				.child(chart::render(&island.history, window, cx)),
		)
		.into_any_element()
}

fn metric(id: &'static str, label: &'static str, value: String) -> Stateful<Div> {
	gpui::div()
		.id(id)
		.debug_selector(move || format!("performance-{id}-row"))
		.role(Role::Group)
		.aria_label(format!("{label}: {value}"))
		.min_w_0()
		.h(gpui::px(METRIC_HEIGHT))
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
		.child(measurement(id, value).w_full().text_left())
}

#[cfg(test)]
mod tests {
	use super::{activity_status, tokens};

	#[test]
	fn activity_status_never_presents_an_old_rate_as_live() {
		assert_eq!(activity_status(false, false, true), Some("Idle"));
		assert_eq!(activity_status(true, false, true), Some("Working"));
		assert_eq!(activity_status(true, true, false), Some("Generating"));
		assert_eq!(activity_status(true, true, true), None);
	}

	#[test]
	fn token_values_include_a_unit_when_available() {
		assert_eq!(tokens("2.6M"), "2.6M tok");
		assert_eq!(tokens("0"), "0 tok");
		assert_eq!(tokens("—"), "—");
	}
}
