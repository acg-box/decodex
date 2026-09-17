//! Discrete reasoning slider. Values come from the selected model's capabilities.
use super::*;
use gpui::{MouseButton, canvas, relative};
use std::{cell::Cell, rc::Rc};

fn index_at(position: f32, count: usize) -> usize {
	(position.clamp(0.0, 1.0) * count.saturating_sub(1) as f32).round() as usize
}

impl ChiefSurface {
	pub(crate) fn set_effort_position(&mut self, position: f32, cx: &mut Context<Self>) {
		let levels = self.model_efforts(cx);
		if let Some(level) = levels.get(index_at(position, levels.len())) {
			self.effort = *level;
			cx.notify();
		}
	}

	pub(crate) fn effort_scale(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
		let levels = self.model_efforts(cx);
		let count = levels.len();
		let index = levels.iter().position(|v| *v == self.effort).unwrap_or(0);
		let fraction = index as f32 / count.saturating_sub(1).max(1) as f32;
		let measured = Rc::new(Cell::new((0.0_f32, 1.0_f32)));
		let capture = measured.clone();
		div()
			.p(px(7.))
			.flex()
			.items_center()
			.gap(px(14.))
			.child(
				div()
					.id("reasoning-slider")
					.role(Role::Slider)
					.tab_index(0)
					.aria_label(format!(
						"Reasoning: {}. Use Left and Right to adjust.",
						level_label(self.effort.as_str())
					))
					.flex_1()
					.h(px(24.))
					.relative()
					.cursor_pointer()
					.on_mouse_down(
						MouseButton::Left,
						cx.listener(move |s, e: &gpui::MouseDownEvent, _, cx| {
							let (left, width) = measured.get();
							s.effort_drag = Some((left, width));
							s.set_effort_position((f32::from(e.position.x) - left) / width, cx);
							cx.stop_propagation();
						}),
					)
					.on_key_down(cx.listener(move |s, e: &gpui::KeyDownEvent, _, cx| {
						let levels = s.model_efforts(cx);
						let current = levels.iter().position(|v| *v == s.effort).unwrap_or(0);
						let next = match e.keystroke.key.as_str() {
							"left" | "down" => current.saturating_sub(1),
							"right" | "up" => (current + 1).min(levels.len().saturating_sub(1)),
							"home" => 0,
							"end" => levels.len().saturating_sub(1),
							_ => return,
						};
						if let Some(level) = levels.get(next) {
							s.effort = *level;
						}
						cx.stop_propagation();
						cx.notify();
					}))
					.child(
						canvas(
							move |bounds, _, _| {
								capture.set((
									f32::from(bounds.origin.x),
									f32::from(bounds.size.width).max(1.),
								));
							},
							|_, _, _, _| {},
						)
						.absolute()
						.inset_0(),
					)
					.child(SliderTrack { fraction, count }),
			)
			.child(
				div()
					.w(px(42.))
					.text_size(px(11.))
					.text_color(rgb(ui_theme::TEXT))
					.child(level_label(self.effort.as_str())),
			)
			.into_any_element()
	}
}

#[derive(gpui::IntoElement)]
struct SliderTrack {
	fraction: f32,
	count: usize,
}
impl gpui::RenderOnce for SliderTrack {
	fn render(self, window: &mut gpui::Window, cx: &mut gpui::App) -> impl IntoElement {
		let fraction = crate::ui_motion::value("reasoning-thumb", self.fraction, window, cx);
		let count = self.count;
		div()
			.absolute()
			.inset_0()
			.child(
				div()
					.absolute()
					.left_0()
					.right_0()
					.top(px(11.))
					.h(px(3.))
					.rounded_full()
					.bg(rgba(0xffffff18)),
			)
			.child(
				div()
					.absolute()
					.left_0()
					.top(px(11.))
					.w(relative(fraction))
					.h(px(3.))
					.rounded_full()
					.bg(rgba(0xc8c4daaa)),
			)
			.children((0..count).map(|i| {
				div()
					.absolute()
					.left(relative(i as f32 / count.saturating_sub(1).max(1) as f32))
					.top(px(10.))
					.ml(px(-1.))
					.size(px(4.))
					.rounded_full()
					.bg(rgba(0xc8c4da55))
			}))
			.child(
				div()
					.absolute()
					.left(relative(fraction))
					.top(px(6.))
					.ml(px(-6.))
					.size(px(13.))
					.rounded_full()
					.bg(rgb(0xd9d7e0))
					.border_1()
					.border_color(rgba(0xffffff80)),
			)
	}
}
#[cfg(test)]
mod tests {
	use super::index_at;
	#[test]
	fn slider_snaps_and_clamps_to_supported_stops() {
		assert_eq!(index_at(-1., 5), 0);
		assert_eq!(index_at(0.37, 5), 1);
		assert_eq!(index_at(0.38, 5), 2);
		assert_eq!(index_at(1.5, 5), 4);
		assert_eq!(index_at(0.9, 1), 0);
		assert_eq!(index_at(0.9, 0), 0);
	}
}
