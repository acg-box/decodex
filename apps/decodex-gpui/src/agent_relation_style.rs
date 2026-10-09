//! One visual grammar for graph edges. Line patterns denote relation types, not certainty.
use super::*;

impl AgentSurface {
	pub(super) fn relation_legend_toggle(&self, cx: &mut Context<Self>) -> AnyElement {
		let hidden = self.work_board.view.legend_hidden;
		let label = if hidden { "Show legend" } else { "Hide legend" };
		gpui::div()
			.id("graph-legend-toggle")
			.debug_selector(|| "graph-legend-toggle".into())
			.role(Role::Button)
			.aria_label(label)
			.tab_index(0)
			.size(gpui::px(24.))
			.flex_none()
			.flex()
			.items_center()
			.justify_center()
			.rounded(gpui::px(5.))
			.cursor_pointer()
			.occlude()
			.hover(|d| d.bg(gpui::rgba(0xffffff10)))
			.tooltip(move |_, cx| cx.new(|_| RelationTip(label.into())).into())
			.child(crate::shell::workspace_symbols::icon_sized(
				if hidden {
					crate::shell::workspace_symbols::Symbol::Eye
				} else {
					crate::shell::workspace_symbols::Symbol::EyeSlash
				},
				14.,
			))
			.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
			.on_click(cx.listener(|s, _, _, cx| {
				s.work_board.view.legend_hidden = !s.work_board.view.legend_hidden;
				cx.notify();
			}))
			.on_key_down(cx.listener(|s, event: &gpui::KeyDownEvent, _, cx| {
				if ["enter", "space"].contains(&event.keystroke.key.as_str()) {
					s.work_board.view.legend_hidden = !s.work_board.view.legend_hidden;
					cx.stop_propagation();
					cx.notify();
				}
			}))
			.into_any_element()
	}

	pub(in super::super) fn relation_legend(&self) -> AnyElement {
		let mut links = gpui::div().flex().flex_wrap().items_center().gap(gpui::px(14.));
		for (kind, label) in [
			(Kind::Message, "Collaboration"),
			(Kind::Resource, "Reference"),
			(Kind::Wait, "Dependency"),
		] {
			links = links.child(
				gpui::div()
					.flex()
					.items_center()
					.flex_none()
					.whitespace_nowrap()
					.gap(gpui::px(5.))
					.child(
						gpui::canvas(
							|_, _, _| (),
							move |bounds, _, window, _| {
								let route = Route {
									start: (1., 5.),
									end: (21., 5.),
									first: (7., 5.),
									second: (14., 5.),
									label: (11., 5.),
								};
								if let Ok(path) =
									route.stroke(kind, bounds.origin, 1., (0., 0.), 1.2).build()
								{
									window.paint_path(path, gpui::rgb(kind.color()));
								}
							},
						)
						.w(gpui::px(22.))
						.h(gpui::px(10.)),
					)
					.child(label),
			);
		}
		let mut lights = gpui::div().flex().flex_wrap().items_center().gap(gpui::px(12.));
		for (color, label) in [
			(crate::ui_theme::BLUE, "Active"),
			(GREEN, "Complete"),
			(AMBER, "Waiting"),
			(crate::ui_theme::ERROR, "Error"),
			(TEXT_MUTED, "Inactive / unknown"),
		] {
			lights = lights.child(
				gpui::div()
					.flex()
					.items_center()
					.flex_none()
					.whitespace_nowrap()
					.gap(gpui::px(5.))
					.child(gpui::div().size(gpui::px(5.)).rounded_full().bg(gpui::rgb(color)))
					.child(label),
			);
		}
		gpui::div()
			.flex_none()
			.flex()
			.flex_col()
			.items_end()
			.gap(gpui::px(5.))
			.text_size(gpui::px(10.))
			.text_color(gpui::rgb(TEXT_MUTED))
			.child(links)
			.child(lights)
			.into_any_element()
	}
}

impl Kind {
	pub(super) fn pattern(self) -> Option<(f32, f32)> {
		match self {
			Self::Context | Self::Resource => Some((8., 5.)),
			Self::Dependency | Self::Wait => Some((2., 5.)),
			_ => None,
		}
	}
}

impl Route {
	pub(super) fn reversed(&self) -> Self {
		Self {
			start: self.end,
			end: self.start,
			first: self.second,
			second: self.first,
			label: self.label,
		}
	}

	pub(super) fn paint_flow(
		&self,
		origin: gpui::Point<gpui::Pixels>,
		scale: f32,
		pan: (f32, f32),
		phase: f32,
		color: u32,
		window: &mut gpui::Window,
	) {
		// Sample arc length so the pulse does not accelerate through curved sections.
		let samples: Vec<_> = (0..=64).map(|i| self.at(i as f32 / 64.)).collect();
		let mut lengths = vec![0.];
		for pair in samples.windows(2) {
			lengths.push(
				lengths.last().copied().unwrap_or(0.)
					+ (pair[1].0 - pair[0].0).hypot(pair[1].1 - pair[0].1),
			);
		}
		let total = *lengths.last().unwrap_or(&0.);
		let head = phase * (total + 48.);
		let point =
			|(x, y)| origin + gpui::point(gpui::px(x * scale + pan.0), gpui::px(y * scale + pan.1));
		let point_at_distance = |distance: f32| {
			let index = lengths.partition_point(|value| *value < distance).clamp(1, 64);
			let span = lengths[index] - lengths[index - 1];
			let fraction = if span > 0. { (distance - lengths[index - 1]) / span } else { 0. };
			let a = samples[index - 1];
			let b = samples[index];
			point((a.0 + (b.0 - a.0) * fraction, a.1 + (b.1 - a.1) * fraction))
		};
		let tail = (head - 48.).clamp(0., total);
		let head = head.min(total);
		for (width, alpha) in [(6., 0x20), (2.4, 0xc0)] {
			let mut trail = PathBuilder::stroke(gpui::px(width * scale));
			trail.move_to(point_at_distance(tail));
			for (&sample, distance) in samples.iter().zip(&lengths) {
				if *distance > tail && *distance < head {
					trail.line_to(point(sample));
				}
			}
			trail.line_to(point_at_distance(head));
			if let Ok(path) = trail.build() {
				window.paint_path(path, gpui::rgba((color << 8) | alpha));
			}
		}
	}

	pub(super) fn at(&self, t: f32) -> (f32, f32) {
		let u = 1. - t;
		let axis =
			|a, b, c, d| u * u * u * a + 3. * u * u * t * b + 3. * u * t * t * c + t * t * t * d;
		(
			axis(self.start.0, self.first.0, self.second.0, self.end.0),
			axis(self.start.1, self.first.1, self.second.1, self.end.1),
		)
	}

	pub(super) fn stroke(
		&self,
		kind: Kind,
		origin: gpui::Point<gpui::Pixels>,
		scale: f32,
		pan: (f32, f32),
		width: f32,
	) -> PathBuilder {
		let point =
			|(x, y)| origin + gpui::point(gpui::px(x * scale + pan.0), gpui::px(y * scale + pan.1));
		let mut path = PathBuilder::stroke(gpui::px(width * scale));
		path.move_to(point(self.start));
		if let Some((dash, gap)) = kind.pattern() {
			let mut last = point(self.start);
			let mut distance = 0_f32;
			for i in 1..=160 {
				let next = point(self.at(i as f32 / 160.));
				let dx = f32::from(next.x - last.x);
				let dy = f32::from(next.y - last.y);
				distance += dx.hypot(dy) / scale;
				if distance % (dash + gap) < dash {
					path.line_to(next);
				} else {
					path.move_to(next);
				}
				last = next;
			}
		} else {
			path.cubic_bezier_to(point(self.end), point(self.first), point(self.second));
		}
		if kind != Kind::Resource {
			let end = point(self.end);
			let dx = self.end.0 - self.second.0;
			let dy = self.end.1 - self.second.1;
			let norm = dx.hypot(dy).max(0.001);
			let (ux, uy) = (dx / norm, dy / norm);
			path.move_to(
				end + gpui::point(
					gpui::px((-ux * 6. - uy * 3.) * scale),
					gpui::px((-uy * 6. + ux * 3.) * scale),
				),
			);
			path.line_to(end);
			path.line_to(
				end + gpui::point(
					gpui::px((-ux * 6. + uy * 3.) * scale),
					gpui::px((-uy * 6. - ux * 3.) * scale),
				),
			);
		}
		path
	}
}
