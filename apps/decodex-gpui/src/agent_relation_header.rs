//! Fixed graph-header geometry, with the same overflow and work-indicator motion.
use crate::{
	ui_motion::{self, OverflowLine},
	ui_theme::TEXT_MUTED,
};
use gpui::{App, IntoElement, RenderOnce, Window, prelude::*};
use std::time::Instant;

#[derive(IntoElement)]
pub(super) struct Header {
	pub key: String,
	pub title: String,
	pub label: Option<String>,
	pub task: Option<String>,
	pub scale: f32,
	pub color: u32,
	pub running: bool,
	pub clock: Instant,
}
impl RenderOnce for Header {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let mut style = window.text_style();
		style.font_size = gpui::px(12. * self.scale).into();
		let title = OverflowLine::new(
			format!("relation-title-{}", self.key).into(),
			self.title.into(),
			style.clone(),
			window,
			cx,
		);
		style.font_size = gpui::px(10. * self.scale).into();
		style.color = gpui::rgb(TEXT_MUTED).into();
		let label_height = style.line_height_in_pixels(window.rem_size());
		let label = self.label.map(|text| {
			let run = gpui::TextRun {
				len: text.len(),
				font: style.font(),
				color: style.color,
				background_color: None,
				underline: None,
				strikethrough: None,
			};
			window.text_system().shape_line(text.into(), gpui::px(10. * self.scale), &[run], None)
		});
		style.font_size = gpui::px(11. * self.scale).into();
		let task = self.task.map(|text| {
			OverflowLine::new(
				format!("relation-task-{}", self.key).into(),
				text.into(),
				style,
				window,
				cx,
			)
		});
		let row_height = title.height.max(gpui::px(10. * self.scale));
		let task_top = row_height + gpui::px(3. * self.scale);
		let height = task.as_ref().map_or(row_height, |task| task_top + task.height);
		gpui::canvas(
			|_, _, _| (),
			move |bounds, _, window, cx| {
				let mut x = bounds.left();
				if let Some(label) = &label {
					window.paint_layer(bounds, |window| {
						let _ = label.paint(
							gpui::point(x, bounds.top() + (row_height - label_height) / 2.),
							label_height,
							gpui::TextAlign::Left,
							None,
							window,
							cx,
						);
					});
					x += label.width + gpui::px(8. * self.scale);
				}
				let lamp = gpui::Bounds::new(
					gpui::point(x, bounds.top() + (row_height - gpui::px(10. * self.scale)) / 2.),
					gpui::size(gpui::px(10. * self.scale), gpui::px(10. * self.scale)),
				);
				paint_lamp(lamp, self.color, self.running, self.scale, self.clock, window, cx);
				x += gpui::px(18. * self.scale);
				title.paint(
					gpui::Bounds::new(
						gpui::point(x, bounds.top()),
						gpui::size((bounds.right() - x).max(gpui::px(0.)), row_height),
					),
					window,
					cx,
				);
				if let Some(task) = &task {
					task.paint(
						gpui::Bounds::new(
							bounds.origin + gpui::point(gpui::px(0.), task_top),
							gpui::size(bounds.size.width, task.height),
						),
						window,
						cx,
					);
				}
			},
		)
		.w_full()
		.h(height)
		.flex_none()
	}
}

fn paint_lamp(
	bounds: gpui::Bounds<gpui::Pixels>,
	color: u32,
	running: bool,
	scale: f32,
	clock: Instant,
	window: &mut Window,
	cx: &mut App,
) {
	if running && !ui_motion::reduced() {
		let phase = (clock.elapsed().as_secs_f32() / 2.8).fract();
		let alpha = (24. + 32. * (phase * std::f32::consts::TAU).sin().abs()) as u32;
		window.paint_quad(
			gpui::fill(bounds, gpui::rgba((color << 8) | alpha)).corner_radii(gpui::px(5. * scale)),
		);
		ui_motion::request_frame(window, cx);
	}
	let dot = gpui::Bounds::new(
		bounds.origin + gpui::point(gpui::px(2. * scale), gpui::px(2. * scale)),
		gpui::size(gpui::px(6. * scale), gpui::px(6. * scale)),
	);
	window.paint_quad(gpui::fill(dot, gpui::rgb(color)).corner_radii(gpui::px(3. * scale)));
}
