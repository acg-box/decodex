//! Compact feedback for a first read. Refreshes with retained content stay quiet.
use gpui::{prelude::*, *};
use std::time::Instant;

#[derive(IntoElement)]
pub(crate) struct Loading {
	label: &'static str,
}

pub(crate) fn loading(label: &'static str) -> Loading {
	Loading { label }
}

impl RenderOnce for Loading {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let clock = window.use_keyed_state(
			SharedString::from(format!("loading-{}", self.label)),
			cx,
			|_, _| Instant::now(),
		);
		let elapsed = clock.read(cx).elapsed().as_secs_f32();
		let reduced = crate::ui_motion::reduced();
		let phase = if reduced { 0. } else { (elapsed - 0.15).max(0.) * std::f32::consts::TAU };
		if !reduced {
			crate::ui_motion::request_frame(window, cx);
		}
		div()
			.id(SharedString::from(format!("loading-row-{}", self.label)))
			.role(Role::Status)
			.aria_label(if self.label.is_empty() { "Loading" } else { self.label })
			.debug_selector(move || format!("loading-feedback-{}", self.label))
			.h(px(24.))
			.flex_none()
			.flex()
			.items_center()
			.gap(px(8.))
			.text_size(px(11.))
			.text_color(rgb(crate::ui_theme::TEXT_MUTED))
			.child(
				canvas(
					|_, _, _| (),
					move |bounds, _, window, _| {
						let mut path = PathBuilder::stroke(px(1.25));
						for i in 0..=24 {
							let angle = phase + i as f32 / 24. * std::f32::consts::TAU * 0.72;
							let p = bounds.center()
								+ point(px(angle.cos() * 4.5), px(angle.sin() * 4.5));
							if i == 0 {
								path.move_to(p);
							} else {
								path.line_to(p);
							}
						}
						if let Ok(path) = path.build() {
							window.paint_path(path, rgb(crate::ui_theme::TEXT_MUTED));
						}
					},
				)
				.size(px(12.))
				.flex_none(),
			)
			.when(!self.label.is_empty(), |row| row.child(self.label))
	}
}
